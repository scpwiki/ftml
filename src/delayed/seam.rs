use super::{DelayedElement, DelayedError, DelayedNode, GeneratedValue, SlotId};
use crate::parsing::wikidot_dash_run_elements;
use crate::preproc::typography::{
    WikidotSeamEdit, native_seam_edits, wikidot_seam_edits,
};
use crate::tree::{ContainerType, Element, ListItem, PartialElement};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::ops::Range;

pub(super) fn resolve_static_suppressions(
    elements: &mut Vec<Element<'_>>,
    apply_typography: bool,
) {
    rewrite_elements(elements, apply_typography, &mut |delayed| {
        Ok(matches!(delayed.node, DelayedNode::TypographyBoundary)
            || matches!(
                delayed.node,
                DelayedNode::Suppressed { ref slots } if slots.is_empty()
            ))
    })
    .expect("static suppression classification is infallible");
}

pub(super) fn resolve_bound_suppressions(
    elements: &mut Vec<Element<'static>>,
    bindings: &BTreeMap<SlotId, GeneratedValue<'_>>,
    resolved_occurrences: &mut usize,
    apply_typography: bool,
) -> Result<(), DelayedError> {
    rewrite_elements(elements, apply_typography, &mut |delayed| {
        if matches!(delayed.node, DelayedNode::TypographyBoundary) {
            return Ok(true);
        }
        let DelayedNode::Suppressed { slots } = &delayed.node else {
            return Ok(false);
        };
        for (id, kind) in slots {
            let Some(value) = bindings.get(id) else {
                return Err(DelayedError::BindingSchemaMismatch);
            };
            if value.kind() != *kind {
                return Err(DelayedError::BindingSchemaMismatch);
            }
        }
        *resolved_occurrences += slots.len();
        Ok(true)
    })
}

fn rewrite_elements<'t, F>(
    elements: &mut Vec<Element<'t>>,
    apply_typography: bool,
    classify: &mut F,
) -> Result<(), DelayedError>
where
    F: FnMut(&DelayedElement<'_>) -> Result<bool, DelayedError>,
{
    for element in elements.iter_mut() {
        visit_children_mut(element, apply_typography, classify)?;
    }

    let mut output = Vec::with_capacity(elements.len());
    let mut group = Vec::new();
    for element in elements.drain(..) {
        let is_text = matches!(element, Element::Text(_));
        let is_suppression = match &element {
            Element::Delayed(delayed) => classify(delayed)?,
            _ => false,
        };
        if is_text || is_suppression {
            group.push((element, is_suppression));
        } else {
            let suppression_only = flush_group(&mut group, &mut output, apply_typography);
            if suppression_only
                && element == Element::LineBreak
                && output.last() == Some(&Element::LineBreak)
            {
                continue;
            }
            output.push(element);
        }
    }
    let suppression_only = flush_group(&mut group, &mut output, apply_typography);
    if apply_typography && suppression_only && output.last() == Some(&Element::LineBreak)
    {
        output.pop();
    }
    *elements = output;
    Ok(())
}

fn visit_children_mut<'t, F>(
    element: &mut Element<'t>,
    apply_typography: bool,
    classify: &mut F,
) -> Result<(), DelayedError>
where
    F: FnMut(&DelayedElement<'_>) -> Result<bool, DelayedError>,
{
    match element {
        Element::Container(container) => {
            let was_nonempty_synthetic_owner = matches!(
                container.ctype(),
                ContainerType::Paragraph | ContainerType::Blockquote
            ) && !container.elements().is_empty();
            rewrite_elements(container.elements_mut(), apply_typography, classify)?;
            if was_nonempty_synthetic_owner && container.elements().is_empty() {
                *element = Element::Delayed(DelayedElement::typography_boundary());
            }
        }
        Element::Table(table) => {
            for row in &mut table.rows {
                for cell in &mut row.cells {
                    rewrite_elements(&mut cell.elements, apply_typography, classify)?;
                }
            }
        }
        Element::TabView(tabs) => {
            for tab in tabs {
                rewrite_elements(&mut tab.elements, apply_typography, classify)?;
            }
        }
        Element::List { items, .. } => {
            for item in items {
                match item {
                    ListItem::Elements { elements, .. } => {
                        rewrite_elements(elements, apply_typography, classify)?;
                    }
                    ListItem::SubList { element } => {
                        visit_children_mut(element, apply_typography, classify)?;
                    }
                }
            }
        }
        Element::DefinitionList(items) => {
            for item in items {
                rewrite_elements(&mut item.key_elements, apply_typography, classify)?;
                rewrite_elements(&mut item.value_elements, apply_typography, classify)?;
            }
        }
        Element::Anchor { elements, .. }
        | Element::Collapsible { elements, .. }
        | Element::Color { elements, .. }
        | Element::Include { elements, .. } => {
            rewrite_elements(elements, apply_typography, classify)?;
        }
        Element::Partial(partial) => match partial {
            PartialElement::ListItem(ListItem::Elements { elements, .. }) => {
                rewrite_elements(elements, apply_typography, classify)?;
            }
            PartialElement::ListItem(ListItem::SubList { element }) => {
                visit_children_mut(element, apply_typography, classify)?;
            }
            PartialElement::TableRow(row) => {
                for cell in &mut row.cells {
                    rewrite_elements(&mut cell.elements, apply_typography, classify)?;
                }
            }
            PartialElement::TableCell(cell) => {
                rewrite_elements(&mut cell.elements, apply_typography, classify)?;
            }
            PartialElement::Tab(tab) => {
                rewrite_elements(&mut tab.elements, apply_typography, classify)?;
            }
            PartialElement::RubyText(text) => {
                rewrite_elements(&mut text.elements, apply_typography, classify)?;
            }
            PartialElement::WikidotEmptyInlineOwner
            | PartialElement::InlineSizeOpen(_)
            | PartialElement::InlineSizeClose(_)
            | PartialElement::InlineSpanOpen(_)
            | PartialElement::InlineSpanClose(_) => {}
        },
        _ => {}
    }
    Ok(())
}

fn flush_group<'t>(
    group: &mut Vec<(Element<'t>, bool)>,
    output: &mut Vec<Element<'t>>,
    apply_typography: bool,
) -> bool {
    let preceded_by_url = last_element_is_url_owner(output);
    if group.is_empty() {
        return false;
    }
    let has_suppression = group.iter().any(|(_, suppression)| *suppression);
    if !has_suppression {
        output.extend(group.drain(..).map(|(element, _)| element));
        return false;
    }

    let mut pieces = Vec::new();
    let mut seams = Vec::new();
    let mut source = String::new();
    for (element, suppression) in group.drain(..) {
        if suppression {
            seams.push(source.len());
            continue;
        }
        let Element::Text(text) = element else {
            unreachable!("suppression groups contain only text and markers");
        };
        let start = source.len();
        source.push_str(&text);
        pieces.push(TextPiece {
            range: start..source.len(),
            text,
        });
    }
    seams.sort_unstable();
    seams.dedup();

    if pieces.is_empty() {
        output.extend(pieces.into_iter().map(|piece| Element::Text(piece.text)));
        return true;
    }

    let text_edits = if apply_typography {
        wikidot_seam_edits(&source, &seams)
    } else {
        native_seam_edits(&source, &seams)
    };
    let mut edits = text_edits
        .into_iter()
        .map(SeamEdit::Text)
        .collect::<Vec<_>>();
    collect_dash_edits(&source, &seams, &mut edits, preceded_by_url);
    collect_space_edits(&source, &seams, &mut edits);
    edits.sort_unstable_by_key(|edit| {
        let range = edit.range();
        (range.start, range.end)
    });

    if edits.is_empty() {
        output.extend(pieces.into_iter().map(|piece| Element::Text(piece.text)));
        return false;
    }

    let mut cursor = 0;
    let mut piece_index = 0;
    for edit in edits {
        let range = edit.range().clone();
        emit_source_range(&pieces, cursor..range.start, &mut piece_index, output);
        match edit {
            SeamEdit::Text(edit) => {
                output.push(Element::Text(Cow::Borrowed(edit.replacement)));
            }
            SeamEdit::Dash { run_len, .. } => {
                output.extend(wikidot_dash_run_elements(run_len));
            }
        }
        cursor = range.end;
    }
    emit_source_range(&pieces, cursor..source.len(), &mut piece_index, output);
    false
}

struct TextPiece<'t> {
    range: Range<usize>,
    text: Cow<'t, str>,
}

enum SeamEdit {
    Text(WikidotSeamEdit),
    Dash { range: Range<usize>, run_len: usize },
}

impl SeamEdit {
    fn range(&self) -> &Range<usize> {
        match self {
            Self::Text(edit) => &edit.range,
            Self::Dash { range, .. } => range,
        }
    }
}

fn last_element_is_url_owner(elements: &[Element<'_>]) -> bool {
    match elements.last() {
        Some(Element::Link {
            link: crate::tree::LinkLocation::Url(_),
            ..
        }) => true,
        Some(Element::Text(text)) => dash_run_is_url_owned(text, text.len()),
        _ => false,
    }
}

fn dash_run_is_url_owned(source: &str, run_start: usize) -> bool {
    let head = &source[..run_start];
    let Some(scheme_start) = ["http://", "https://", "ftp://", "mailto:"]
        .iter()
        .filter_map(|scheme| head.rfind(scheme))
        .max()
    else {
        return false;
    };
    !source[scheme_start..run_start]
        .char_indices()
        .any(|(offset, ch)| {
            let index = scheme_start + offset;
            ch.is_whitespace()
                || ch.is_control()
                || matches!(ch, '"' | '\'' | '|' | '[' | ']')
                || source[index..].starts_with(">@")
                || source[index..].starts_with("@@")
        })
}

fn collect_dash_edits(
    source: &str,
    seams: &[usize],
    edits: &mut Vec<SeamEdit>,
    preceded_by_url: bool,
) {
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'-' {
            index += 1;
            continue;
        }
        let start = index;
        while bytes.get(index) == Some(&b'-') {
            index += 1;
        }
        let range = start..index;
        if range.len() >= 2
            && range_crosses_seam(&range, seams)
            && !dash_run_is_url_owned(source, range.start)
            && !(range.start == 0 && preceded_by_url)
        {
            edits.push(SeamEdit::Dash {
                run_len: range.len(),
                range,
            });
        }
    }
}

fn collect_space_edits(source: &str, seams: &[usize], edits: &mut Vec<SeamEdit>) {
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b' ' {
            index += 1;
            continue;
        }
        let start = index;
        while bytes.get(index) == Some(&b' ') {
            index += 1;
        }
        let range = start..index;
        if range.len() >= 2 && range_crosses_seam(&range, seams) {
            edits.push(SeamEdit::Text(WikidotSeamEdit {
                range,
                replacement: " ",
            }));
        }
    }
}

fn range_crosses_seam(range: &Range<usize>, seams: &[usize]) -> bool {
    let candidate = seams.partition_point(|seam| *seam <= range.start);
    seams.get(candidate).is_some_and(|seam| *seam < range.end)
}

fn emit_source_range<'t>(
    pieces: &[TextPiece<'t>],
    range: Range<usize>,
    piece_index: &mut usize,
    output: &mut Vec<Element<'t>>,
) {
    while pieces
        .get(*piece_index)
        .is_some_and(|piece| piece.range.end <= range.start)
    {
        *piece_index += 1;
    }

    while let Some(piece) = pieces.get(*piece_index) {
        if piece.range.start >= range.end {
            break;
        }
        let start = range.start.max(piece.range.start);
        let end = range.end.min(piece.range.end);
        if start < end {
            let local = start - piece.range.start..end - piece.range.start;
            let text = match &piece.text {
                Cow::Borrowed(text) => Cow::Borrowed(&text[local]),
                Cow::Owned(text) => Cow::Owned(text[local].to_owned()),
            };
            output.push(Element::Text(text));
        }
        if end < piece.range.end {
            break;
        }
        *piece_index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_typography_boundary_is_consumed_like_static_suppression() {
        let mut elements = vec![
            Element::Text(Cow::Borrowed("A")),
            Element::Delayed(DelayedElement::typography_boundary()),
            Element::Text(Cow::Borrowed("B")),
        ];
        let mut resolved = 0;

        resolve_bound_suppressions(&mut elements, &BTreeMap::new(), &mut resolved, true)
            .expect("typography boundary is a bound suppression seam");

        assert!(
            elements
                .iter()
                .all(|element| !matches!(element, Element::Delayed(_))),
            "{elements:#?}",
        );
    }
}
