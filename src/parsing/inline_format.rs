use crate::parsing::rule::impls::{
    is_wikidot_only_autolink_url, normalize_wikidot_color,
};
use crate::tree::{
    AnchorTarget, AttributeMap, Bibliography, Container, ContainerType, Element,
    InlineFormatKind, LinkLabel, LinkLocation, ListItem, PartialElement, TableType,
    WikidotBlockShell,
};
use std::borrow::Cow;
use std::mem;
use std::ops::Range;

const FORMAT_KIND_COUNT: usize = 8;
const MAX_NORMALIZATION_PASSES: usize =
    crate::parsing::parser::DEFAULT_MAX_RECURSION_DEPTH;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FormatKind {
    Bold,
    Italics,
    Underline,
    Strike,
    Subscript,
    Superscript,
    Monospace,
    Color,
}

impl FormatKind {
    const ALL: [Self; FORMAT_KIND_COUNT] = [
        Self::Bold,
        Self::Italics,
        Self::Underline,
        Self::Strike,
        Self::Subscript,
        Self::Superscript,
        Self::Monospace,
        Self::Color,
    ];

    const fn index(self) -> usize {
        match self {
            Self::Bold => 0,
            Self::Italics => 1,
            Self::Underline => 2,
            Self::Strike => 3,
            Self::Subscript => 4,
            Self::Superscript => 5,
            Self::Monospace => 6,
            Self::Color => 7,
        }
    }

    const fn close_marker(self) -> &'static str {
        match self {
            Self::Bold => "**",
            Self::Italics => "//",
            Self::Underline => "__",
            Self::Strike => "--",
            Self::Subscript => ",,",
            Self::Superscript => "^^",
            Self::Monospace => "}}",
            Self::Color => "##",
        }
    }

    const fn same_family_recovery(self) -> bool {
        matches!(
            self,
            Self::Bold
                | Self::Italics
                | Self::Underline
                | Self::Strike
                | Self::Subscript
                | Self::Superscript
        )
    }
}

#[derive(Clone, Debug)]
enum FormatShell<'t> {
    Container {
        kind: FormatKind,
        ctype: ContainerType,
        attributes: AttributeMap<'t>,
    },
    Color {
        color: Cow<'t, str>,
        background: bool,
    },
}

impl<'t> FormatShell<'t> {
    fn kind(&self) -> FormatKind {
        match self {
            FormatShell::Container { kind, .. } => *kind,
            FormatShell::Color { .. } => FormatKind::Color,
        }
    }

    fn wikidot_tag(&self) -> &'static str {
        match self.kind() {
            FormatKind::Bold => "strong",
            FormatKind::Italics => "em",
            FormatKind::Underline | FormatKind::Strike | FormatKind::Color => "span",
            FormatKind::Subscript => "sub",
            FormatKind::Superscript => "sup",
            FormatKind::Monospace => "tt",
        }
    }

    fn build(&self, elements: Vec<Element<'t>>) -> Element<'t> {
        match self {
            FormatShell::Container {
                ctype, attributes, ..
            } => Element::Container(Container::new(*ctype, elements, attributes.clone())),
            FormatShell::Color { color, background } => Element::Color {
                color: color.clone(),
                background: *background,
                elements,
            },
        }
    }

    fn from_element(element: &Element<'t>) -> Option<Self> {
        match element {
            Element::Container(container) => {
                let kind = kind_from_container(container.ctype())?;
                Some(FormatShell::Container {
                    kind,
                    ctype: container.ctype(),
                    attributes: container.attributes().clone(),
                })
            }
            Element::Color {
                color, background, ..
            } => Some(FormatShell::Color {
                color: color.clone(),
                background: *background,
            }),
            _ => None,
        }
    }

    fn into_parts(element: Element<'t>) -> Option<(Self, Vec<Element<'t>>)> {
        match element {
            Element::Container(container) => {
                let kind = kind_from_container(container.ctype())?;
                let ctype = container.ctype();
                let attributes = container.attributes().clone();
                let elements = container.into();
                Some((
                    FormatShell::Container {
                        kind,
                        ctype,
                        attributes,
                    },
                    elements,
                ))
            }
            Element::Color {
                color,
                background,
                elements,
            } => Some((FormatShell::Color { color, background }, elements)),
            _ => None,
        }
    }
}

struct OuterCandidate<'t> {
    shell: FormatShell<'t>,
    opener_len: usize,
    literal_marker: &'static str,
}

pub(crate) fn normalize_wikidot_inline_formats(elements: &mut Vec<Element<'_>>) {
    normalize_sequence(elements);
}

fn normalize_sequence<'t>(elements: &mut Vec<Element<'t>>) {
    normalize_sequence_scoped(elements, false);
}

fn normalize_sequence_inside_monospace<'t>(elements: &mut Vec<Element<'t>>) {
    normalize_sequence_scoped(elements, true);
}

fn normalize_sequence_scoped<'t>(
    elements: &mut Vec<Element<'t>>,
    inside_monospace: bool,
) {
    // Parser-emitted events mean the pairing scan already resolved this sequence,
    // so a marker left as text here has no partner for a repair pass to find.
    if !elements.iter().any(is_format_event) {
        normalize_sequence_passes(elements, inside_monospace);
    }
    hoist_kept_leading_space_pass(elements);
    normalize_sequence_children(elements);
    lower_dash_runs(elements);
    substitute_number_unit_spaces(elements);
}

fn is_format_event(element: &Element<'_>) -> bool {
    matches!(
        element,
        Element::Partial(
            PartialElement::InlineFormatOpen(_)
                | PartialElement::InlineFormatClose(_)
                | PartialElement::InlineColorOpen(..)
                | PartialElement::InlineColorClose
        )
    )
}

fn normalize_sequence_children<'t>(elements: &mut Vec<Element<'t>>) {
    let defer = foster_deferral_indices(elements);
    let mut normalized = Vec::with_capacity(elements.len());
    let mut index = 0;
    for mut element in mem::take(elements) {
        // Held raw; the advanced table's foster join (or its gate-fail
        // path, which normalizes them like the visit would) owns them.
        if defer[index] {
            normalized.push(element);
            index += 1;
            continue;
        }
        if is_segment_container(&element) {
            if !normalize_segment_container_with_foster(&mut normalized, &mut element)
                && !normalize_segment_container_with_run(&mut normalized, &mut element)
            {
                normalize_segment_container(&mut element);
            }
        } else if matches!(
            &element,
            Element::Container(container)
                if container.ctype() == ContainerType::Monospace
        ) {
            visit_children_mut(&mut element, normalize_sequence_inside_monospace);
        } else {
            visit_children_mut(&mut element, normalize_sequence);
        }
        normalized.push(element);
        index += 1;
    }
    *elements = normalized;
}

/// Indices of `WikidotFoster` partials that form the contiguous run an
/// advanced table's foster join consumes (its backward scan only matches
/// fosters and line breaks). Those tails must reach the join
/// un-normalized: their own `--` is claimed by pairing first, and a
/// standalone visit would dash-convert it before the join runs.
fn foster_deferral_indices<'t>(elements: &[Element<'t>]) -> Vec<bool> {
    let mut defer = vec![false; elements.len()];
    for index in 0..elements.len() {
        let Element::Table(table) = &elements[index] else {
            continue;
        };
        if table.table_type != TableType::Advanced {
            continue;
        }
        let mut cursor = index;
        while cursor > 0 {
            cursor -= 1;
            match &elements[cursor] {
                Element::Partial(PartialElement::WikidotFoster(_)) => {
                    defer[cursor] = true;
                }
                Element::LineBreak => {}
                _ => break,
            }
        }
    }
    defer
}

fn normalize_segment_container_with_run<'t>(
    preceding: &mut Vec<Element<'t>>,
    container: &mut Element<'t>,
) -> bool {
    let joinable = match container {
        Element::Table(table) => table.table_type == TableType::Simple,
        Element::DefinitionList(_) => true,
        _ => false,
    };
    if !joinable {
        return false;
    }

    let mut run_start = preceding.len();
    while run_start > 0 {
        let candidate = &preceding[run_start - 1];
        if matches!(candidate, Element::LineBreaks(_)) || !candidate.paragraph_safe() {
            break;
        }
        run_start -= 1;
    }
    if run_start == preceding.len() {
        return false;
    }

    let mut run = preceding.split_off(run_start);
    let mut segments: Vec<&mut Vec<Element<'t>>> = vec![&mut run];
    segments.extend(segment_vecs_mut(container));
    normalize_segments(&mut segments, 0);
    preceding.append(&mut run);
    true
}

fn is_segment_container(element: &Element<'_>) -> bool {
    matches!(
        element,
        Element::List { .. } | Element::DefinitionList(_) | Element::Table(_)
    )
}

fn hoist_kept_leading_space_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let mut changed = false;
    let mut index = 0;
    while index < elements.len() {
        if hoist_kept_leading_space_at(elements, index) {
            changed = true;
            if kept_space_separator_before(elements, index) {
                elements.insert(index, Element::Text(Cow::Borrowed(" ")));
                index += 1;
            }
        }
        index += 1;
    }
    changed
}

fn hoist_kept_leading_space_at<'t>(
    elements: &mut Vec<Element<'t>>,
    index: usize,
) -> bool {
    if matches!(
        &elements[index],
        Element::Partial(PartialElement::InlineFormatOpen(
            InlineFormatKind::Monospace
        )) | Element::Partial(PartialElement::InlineColorOpen(..))
    ) {
        return strip_partial_leading_run(elements, index);
    }
    match &mut elements[index] {
        Element::Container(container)
            if container.ctype() == ContainerType::Monospace =>
        {
            strip_kept_leading_run(container.elements_mut())
        }
        Element::Color { elements, .. } => strip_kept_leading_run(elements),
        _ => false,
    }
}

fn strip_kept_leading_run(children: &mut Vec<Element<'_>>) -> bool {
    let Some(Element::Text(first)) = children.first() else {
        return false;
    };
    let run = leading_pad_run_len(first);
    if run == 0 {
        return false;
    }
    if run < first.len() {
        if !first[run..].starts_with("--") {
            return false;
        }
        let rest = first[run..].to_owned();
        children[0] = Element::Text(Cow::Owned(rest));
        return true;
    }
    if !children.get(1).is_some_and(kept_space_dash_follows) {
        return false;
    }
    children.remove(0);
    true
}

fn strip_partial_leading_run<'t>(elements: &mut Vec<Element<'t>>, index: usize) -> bool {
    let Some(Element::Text(next)) = elements.get(index + 1) else {
        return false;
    };
    let run = leading_pad_run_len(next);
    if run == 0 {
        return false;
    }
    if run < next.len() {
        let rest = next[run..].to_owned();
        elements[index + 1] = Element::Text(Cow::Owned(rest));
    } else {
        elements.remove(index + 1);
    }
    true
}

fn leading_pad_run_len(text: &str) -> usize {
    text.bytes()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count()
}

fn kept_space_dash_follows(element: &Element<'_>) -> bool {
    if is_plain_strike(element) {
        return true;
    }
    matches!(element, Element::Text(text) if text.starts_with("--"))
}

fn kept_space_separator_before(elements: &[Element<'_>], index: usize) -> bool {
    let Some(previous) = index.checked_sub(1).and_then(|prev| elements.get(prev)) else {
        return false;
    };
    match previous {
        Element::Text(text) => text
            .chars()
            .last()
            .is_some_and(|ch| !ch.is_ascii_whitespace()),
        Element::LineBreak | Element::LineBreaks(_) => false,
        _ => true,
    }
}

fn normalize_sequence_passes<'t>(
    elements: &mut Vec<Element<'t>>,
    inside_monospace: bool,
) -> bool {
    if !sequence_has_inline_repair_material(elements) {
        return false;
    }
    let mut any_changed = false;
    for _ in 0..MAX_NORMALIZATION_PASSES {
        let mut changed = repair_literal_marker_pairs_pass(elements, inside_monospace)
            | repair_fifo_triples_pass(elements)
            | repair_same_family_pass(elements)
            | repair_crossed_pass(elements)
            | repair_strike_split_close_pass(elements)
            | repair_raw_strike_pairs_pass(elements)
            | repair_strike_shed_pairs_pass(elements)
            | repair_script_split_close_pass(elements)
            | repair_script_pairs_pass(elements);
        // A monospace container's children are already owned content:
        // literal brace text there is content, never an unclosed opener
        // waiting for a link close, so the link-close repair must not
        // re-pair braces inside it.
        if !inside_monospace {
            changed |= repair_trapped_opener_pass(elements);
            changed |= repair_monospace_split_close_pass(elements);
            changed |= repair_monospace_link_close_pass(elements);
        }
        changed |=
            repair_wikidot_scheme_star_pass(elements) | repair_dash_runs_pass(elements);
        any_changed |= changed;
        if !changed {
            break;
        }
    }
    any_changed
}

fn sequence_has_inline_repair_material(elements: &[Element<'_>]) -> bool {
    if elements
        .iter()
        .any(|element| !matches!(element, Element::Text(_)))
    {
        return true;
    }

    let mut previous = None::<char>;
    let mut before_previous = None::<char>;
    for (index, element) in elements.iter().enumerate() {
        let Element::Text(value) = element else {
            unreachable!("the sequence was checked to be all text");
        };
        let value = value.as_ref();
        if matches!(value, "**" | "//" | "__" | "{{" | "}}" | "##") {
            return true;
        }
        if matches!(value, "--" | ",," | "^^") {
            let next_starts_with_space = elements
                .get(index + 1)
                .is_some_and(element_starts_with_ascii_space);
            if !next_starts_with_space {
                return true;
            }
        }
        if value.contains('}') {
            return true;
        }
        for character in value.chars() {
            if before_previous == Some('-')
                && previous == Some('-')
                && !character.is_whitespace()
            {
                return true;
            }
            before_previous = previous;
            previous = Some(character);
        }
    }
    false
}

fn substitute_number_unit_spaces<'t>(elements: &mut [Element<'t>]) {
    let mut start = 0;
    while start < elements.len() {
        if !matches!(elements[start], Element::Text(_)) {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < elements.len() && matches!(elements[end], Element::Text(_)) {
            end += 1;
        }
        let mut joined = String::new();
        for element in &elements[start..end] {
            let Element::Text(text) = element else {
                unreachable!();
            };
            joined.push_str(text);
        }
        let positions = crate::preproc::typography::wikidot_nbsp_space_positions(&joined);
        if !positions.is_empty() {
            let mut position_iter = positions.into_iter().peekable();
            let mut base = 0usize;
            for element in &mut elements[start..end] {
                let Element::Text(text) = element else {
                    unreachable!();
                };
                let span_end = base + text.len();
                let mut local = Vec::new();
                while let Some(&position) = position_iter.peek() {
                    if position >= span_end {
                        break;
                    }
                    position_iter.next();
                    if position >= base {
                        local.push(position - base);
                    }
                }
                if !local.is_empty() {
                    let mut substituted = String::with_capacity(text.len() + local.len());
                    let mut last = 0usize;
                    for position in local {
                        debug_assert_eq!(text.as_bytes()[position], b' ');
                        substituted.push_str(&text[last..position]);
                        substituted.push('\u{a0}');
                        last = position + 1;
                    }
                    substituted.push_str(&text[last..]);
                    *text = Cow::Owned(substituted);
                }
                base = span_end;
            }
        }
        start = end;
    }
}

fn repair_fifo_triples_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let candidates = outer_candidates(elements);
    let next_candidates = next_candidate_indices(&candidates);
    let next_formatters = next_any_formatter_indices(elements);
    let mut input = take_elements(elements);
    let mut output = Vec::with_capacity(input.len());
    let mut changed = false;
    let mut index = 0;

    while index < input.len() {
        let Some(outer) = candidates[index].as_ref() else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };
        let outer_end = index + outer.opener_len - 1;
        let Some(middle_index) = next_candidates[outer_end] else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };
        let middle = candidates[middle_index]
            .as_ref()
            .expect("candidate index points to a candidate");
        let middle_end = middle_index + middle.opener_len - 1;
        let Some(inner_index) = next_formatters[middle_end] else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };
        let Some(inner_shell) = input[inner_index]
            .as_ref()
            .and_then(FormatShell::from_element)
        else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };
        let outer_kind = outer.shell.kind();
        let middle_kind = middle.shell.kind();
        let inner_kind = inner_shell.kind();
        if outer_kind == middle_kind
            || outer_kind == inner_kind
            || middle_kind == inner_kind
        {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        }
        let Some(children) = input[inner_index].as_ref().and_then(formatter_children)
        else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };
        let Some(outer_close) = find_text_marker(children, outer_kind.close_marker())
        else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };
        let Some(middle_close) = children[outer_close + 1..]
            .iter()
            .position(|element| is_exact_text(element, middle_kind.close_marker()))
            .map(|relative| outer_close + 1 + relative)
        else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };

        discard_range(&mut input, index..index + outer.opener_len);
        let outer_prefix = take_range(&mut input, index + outer.opener_len..middle_index);
        discard_range(&mut input, middle_index..middle_index + middle.opener_len);
        let middle_prefix =
            take_range(&mut input, middle_index + middle.opener_len..inner_index);
        let inner = take_element(&mut input, inner_index);
        let (inner_shell, mut inner_children) =
            FormatShell::into_parts(inner).expect("formatter index has a shell");
        let after_middle = inner_children.split_off(middle_close + 1);
        inner_children.pop();
        let mut between_closes = inner_children.split_off(outer_close + 1);
        inner_children.pop();

        let initial_inner = inner_shell.build(inner_children);
        let mut initial_middle_children = middle_prefix;
        initial_middle_children.push(initial_inner);
        let outer_matches_inner = outer.shell.wikidot_tag() == inner_shell.wikidot_tag();
        let outer_matches_middle =
            outer.shell.wikidot_tag() == middle.shell.wikidot_tag();
        let middle_matches_inner =
            middle.shell.wikidot_tag() == inner_shell.wikidot_tag();
        let mut outer_children = outer_prefix;

        if outer_matches_inner {
            initial_middle_children.append(&mut between_closes);
            outer_children.push(middle.shell.build(initial_middle_children));
            outer_children.extend(after_middle);
        } else {
            outer_children.push(middle.shell.build(initial_middle_children));
            let separator = take_leading_ascii_space(&mut between_closes);
            if separator {
                outer_children.push(Element::Text(Cow::Borrowed(" ")));
            }
            if middle_matches_inner {
                if !between_closes.is_empty() {
                    outer_children.push(middle.shell.build(between_closes));
                }
                outer_children.extend(after_middle);
            } else if outer_matches_middle {
                if !between_closes.is_empty() {
                    outer_children.push(inner_shell.build(between_closes));
                }
                outer_children.extend(after_middle);
            } else {
                let mut continued_inner_children = Vec::new();
                if !between_closes.is_empty() {
                    continued_inner_children.push(middle.shell.build(between_closes));
                }
                continued_inner_children.extend(after_middle);
                if !continued_inner_children.is_empty() {
                    outer_children.push(inner_shell.build(continued_inner_children));
                }
            }
        }

        output.push(outer.shell.build(outer_children));
        index = inner_index + 1;
        changed = true;
    }

    *elements = output;
    changed
}

fn repair_same_family_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let candidates = outer_candidates(elements);
    let next_formatters = next_formatter_indices(elements);
    let mut input = take_elements(elements);
    let mut output = Vec::with_capacity(input.len());
    let mut changed = false;
    let mut index = 0;

    while index < input.len() {
        let Some(outer) = candidates[index].as_ref() else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };
        let kind = outer.shell.kind();
        if !kind.same_family_recovery() {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        }
        let opener_end = index + outer.opener_len - 1;
        let Some(inner_index) = next_formatters[opener_end][kind.index()] else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };

        discard_range(&mut input, index..index + outer.opener_len);
        let mut outer_elements =
            take_range(&mut input, index + outer.opener_len..inner_index);
        let inner = take_element(&mut input, inner_index);
        let (_, inner_elements) =
            FormatShell::into_parts(inner).expect("formatter index has a shell");
        outer_elements.push(Element::Text(Cow::Borrowed(outer.literal_marker)));
        outer_elements.extend(inner_elements);
        output.push(outer.shell.build(outer_elements));
        index = inner_index + 1;
        changed = true;
    }

    *elements = output;
    changed
}

#[derive(Clone, Copy)]
struct CrossTarget {
    inner_index: usize,
    marker_index: usize,
}

fn repair_trapped_opener_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let mut changed = false;
    let mut index = 0;
    while index < elements.len() {
        if !trapped_opener_at(elements, index) {
            index += 1;
            continue;
        }
        let element = elements.remove(index);
        let (_, children) =
            FormatShell::into_parts(element).expect("formatter index has a shell");
        let mut replacement = Vec::with_capacity(children.len() + 2);
        replacement.push(Element::Partial(PartialElement::InlineFormatOpen(
            InlineFormatKind::Strikethrough,
        )));
        replacement.extend(children);
        replacement.push(Element::Partial(PartialElement::InlineFormatClose(
            InlineFormatKind::Strikethrough,
        )));
        elements.splice(index..index, replacement);
        changed = true;
        index += 1;
    }
    changed
}

fn trapped_opener_at(elements: &[Element<'_>], index: usize) -> bool {
    let Some(FormatShell::Container {
        kind: FormatKind::Strike,
        attributes,
        ..
    }) = FormatShell::from_element(&elements[index])
    else {
        return false;
    };
    if !attributes.get().is_empty() {
        return false;
    }
    let Some(children) = formatter_children(&elements[index]) else {
        return false;
    };
    if children
        .iter()
        .any(|child| pairing_barrier(child) || element_contains_link(child))
    {
        return false;
    }
    let candidates = outer_candidates(children);
    let Some(kind) =
        candidates
            .iter()
            .enumerate()
            .find_map(|(child_index, candidate)| {
                let candidate = candidate.as_ref()?;
                let kind = candidate.shell.kind();
                (kind != FormatKind::Strike
                    && trapped_candidate_orphaned(children, child_index, candidate))
                .then_some(kind)
            })
    else {
        return false;
    };
    let close_marker = kind.close_marker();
    for element in &elements[index + 1..] {
        if is_exact_text(element, close_marker) {
            return true;
        }
        if pairing_barrier(element) {
            return false;
        }
    }
    false
}

fn trapped_candidate_orphaned(
    children: &[Element<'_>],
    index: usize,
    candidate: &OuterCandidate<'_>,
) -> bool {
    let kind = candidate.shell.kind();
    let rest = &children[index + candidate.opener_len..];
    match kind {
        FormatKind::Color => !rest.iter().any(|element| is_exact_text(element, "##")),
        FormatKind::Subscript | FormatKind::Superscript => {
            let marker = kind.close_marker();
            !rest.iter().enumerate().any(|(offset, element)| {
                is_exact_text(element, marker)
                    && !element_ends_with_ascii_space(
                        &children[index + candidate.opener_len + offset - 1],
                    )
            })
        }
        _ => {
            let marker = kind.close_marker();
            !rest.iter().any(|element| is_exact_text(element, marker))
        }
    }
}

fn repair_crossed_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let candidates = outer_candidates(elements);
    let next_crossed = next_crossed_indices(elements);
    let mut input = take_elements(elements);
    let mut output = Vec::with_capacity(input.len());
    let mut changed = false;
    let mut index = 0;

    while index < input.len() {
        let Some(outer) = candidates[index].as_ref() else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };
        let opener_end = index + outer.opener_len - 1;
        let Some(target) = next_crossed[opener_end][outer.shell.kind().index()] else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };

        discard_range(&mut input, index..index + outer.opener_len);
        let mut outer_elements =
            take_range(&mut input, index + outer.opener_len..target.inner_index);
        let inner = take_element(&mut input, target.inner_index);
        let (inner_shell, mut inner_elements) =
            FormatShell::into_parts(inner).expect("formatter index has a shell");
        let mut continuation = inner_elements.split_off(target.marker_index + 1);
        inner_elements.pop();
        outer_elements.push(inner_shell.build(inner_elements));

        if outer.shell.wikidot_tag() == inner_shell.wikidot_tag() {
            outer_elements.append(&mut continuation);
            output.push(outer.shell.build(outer_elements));
        } else {
            let separator = take_leading_ascii_space(&mut continuation);
            output.push(outer.shell.build(outer_elements));
            if separator {
                output.push(Element::Text(Cow::Borrowed(" ")));
            }
            if !continuation.is_empty() {
                output.push(inner_shell.build(continuation));
            }
        }
        index = target.inner_index + 1;
        changed = true;
    }

    *elements = output;
    changed
}

fn repair_raw_strike_pairs_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let next_closes = next_valid_dash_closes(elements);
    let mut input = take_elements(elements);
    let mut output = Vec::with_capacity(input.len());
    let mut changed = false;
    let mut index = 0;

    while index < input.len() {
        let valid_open = input[index].as_ref().is_some_and(|element| {
            is_exact_text(element, "--")
                && !dash_marker_occupies_its_line_at(&input, index)
        }) && input
            .get(index + 1)
            .and_then(Option::as_ref)
            .is_none_or(|element| !element_starts_with_ascii_space(element));
        let Some(close) = valid_open.then(|| next_closes[index]).flatten() else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };
        let run_len = input[close]
            .as_ref()
            .and_then(dash_run_len)
            .expect("dash close index points to a dash run");
        discard_range(&mut input, index..index + 1);
        let contents = take_range(&mut input, index + 1..close);
        discard_range(&mut input, close..close + 1);
        output.push(strike_shell().build(contents));
        if run_len > 2 {
            output.push(Element::Text(Cow::Owned("-".repeat(run_len - 2))));
        }
        index = close + 1;
        changed = true;
    }

    *elements = output;
    changed
}

fn strike_close_split(value: &str) -> Option<(&str, &str)> {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'-' {
            index += 1;
            continue;
        }
        let run_end = index
            + bytes[index..]
                .iter()
                .take_while(|byte| **byte == b'-')
                .count();
        if run_end - index >= 2 {
            let before = &value[..index];
            if before.is_empty() || is_shed_text(before) {
                return Some((before, &value[index + 2..]));
            }
            return None;
        }
        index = run_end;
    }
    None
}

fn dash_split_pieces(value: &str) -> Vec<String> {
    let bytes = value.as_bytes();
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'-' {
            let run_end = index
                + bytes[index..]
                    .iter()
                    .take_while(|byte| **byte == b'-')
                    .count();
            if run_end - index >= 2 {
                if start < index {
                    pieces.push(value[start..index].to_string());
                }
                pieces.push(value[index..run_end].to_string());
                start = run_end;
            }
            index = run_end;
        } else {
            index += 1;
        }
    }
    if start < value.len() {
        pieces.push(value[start..].to_string());
    }
    pieces
}

fn push_dash_split_text<'t>(output: &mut Vec<Element<'t>>, value: &str) {
    for piece in dash_split_pieces(value) {
        output.push(Element::Text(Cow::Owned(piece)));
    }
}

fn repair_strike_split_close_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let mut output = Vec::with_capacity(elements.len());
    let mut changed = false;

    for element in mem::take(elements) {
        let is_strike = matches!(
            &element,
            Element::Container(container) if container.ctype() == ContainerType::Strikethrough
        );
        if !is_strike {
            output.push(element);
            continue;
        }
        let (shell, mut children) =
            FormatShell::into_parts(element).expect("formatter index has a shell");
        match strike_split_close(&children) {
            Some((child_index, prefix, suffix)) => {
                let mut tail: Vec<Element<'t>> =
                    children.drain(child_index + 1..).collect();
                children.truncate(child_index);
                if !prefix.is_empty() {
                    children.push(Element::Text(Cow::Owned(prefix)));
                }
                output.push(shell.build(children));
                push_dash_split_text(&mut output, &suffix);
                output.append(&mut tail);
                output.push(Element::Text(FormatKind::Strike.close_marker().into()));
                changed = true;
            }
            None => output.push(shell.build(children)),
        }
    }

    *elements = output;
    changed
}

fn strike_split_close(children: &[Element<'_>]) -> Option<(usize, String, String)> {
    if children.len() < 3 {
        return None;
    }
    let links = link_prefix_counts(children);
    for child_index in 1..children.len() - 1 {
        let Element::Text(value) = &children[child_index] else {
            continue;
        };
        let Some((prefix, suffix)) = strike_close_split(value.as_ref()) else {
            continue;
        };
        if prefix.is_empty() && element_ends_with_ascii_space(&children[child_index - 1])
        {
            continue;
        }
        if links[child_index] == links[0] {
            continue;
        }
        return Some((child_index, prefix.to_string(), suffix.to_string()));
    }
    None
}

fn repair_strike_shed_pairs_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let mut input = take_elements(elements);
    // Both repeated forward searches are answered from one suffix sweep each.
    let next_close = strike_shed_close_candidates(&input);
    let next_link =
        next_matching_indices(&input, |_, element| element_contains_link(element));
    let mut output = Vec::with_capacity(input.len());
    let mut changed = false;
    let mut index = 0;

    while index < input.len() {
        let Some((opener_len, excess)) = strike_shed_opener(&input, index) else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };
        let Some((close, prefix, suffix)) =
            strike_shed_close(&input, index, opener_len, &next_close, &next_link)
        else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };
        discard_range(&mut input, index..index + opener_len);
        let mut contents = Vec::new();
        if excess > 0 {
            contents.push(Element::Text(Cow::Owned("-".repeat(excess))));
        }
        contents.extend(take_range(&mut input, index + opener_len..close));
        if !prefix.is_empty() {
            contents.push(Element::Text(Cow::Owned(prefix)));
        }
        discard_range(&mut input, close..close + 1);
        output.push(strike_shell().build(contents));
        push_dash_split_text(&mut output, &suffix);
        index = close + 1;
        changed = true;
    }

    *elements = output;
    changed
}

fn strike_shed_opener(
    input: &[Option<Element<'_>>],
    index: usize,
) -> Option<(usize, usize)> {
    let element = input[index].as_ref()?;
    if is_exact_text(element, "--") {
        let next = input.get(index + 1).and_then(Option::as_ref);
        if next.is_some_and(element_starts_with_ascii_space)
            || dash_marker_occupies_its_line_at(input, index)
        {
            return None;
        }
        return Some((1, 0));
    }
    let run_len = dash_run_len(element)?;
    if !(3..=4).contains(&run_len) {
        return None;
    }
    Some((1, run_len - 2))
}

/// Suffix array of positions whose text can close a shed strike.
fn strike_shed_close_candidates(input: &[Option<Element<'_>>]) -> Vec<Option<usize>> {
    next_matching_indices(input, |index, element| {
        if index == 0 || dash_marker_occupies_its_line_at(input, index) {
            return false;
        }
        let Element::Text(value) = element else {
            return false;
        };
        let Some((prefix, _)) = strike_close_split(value.as_ref()) else {
            return false;
        };
        !prefix.is_empty()
            || !input[index - 1]
                .as_ref()
                .is_some_and(|previous| element_ends_with_ascii_space(previous))
    })
}

fn strike_shed_close(
    input: &[Option<Element<'_>>],
    opener: usize,
    opener_len: usize,
    next_close: &[Option<usize>],
    next_link: &[Option<usize>],
) -> Option<(usize, String, String)> {
    let first = opener + opener_len;
    // The single forward scan the original performed is now two suffix
    // lookups: the first candidate close, then the first link at or after the
    // opener. Only that first close is considered: when no link precedes it
    // the original gives up instead of looking further.
    let mut close = next_close.get(first).copied().flatten()?;
    let mut pieces = strike_shed_close_at(input, close)?;
    if close == first && pieces.0.is_empty() {
        close = next_close.get(close + 1).copied().flatten()?;
        pieces = strike_shed_close_at(input, close)?;
    }
    let link = next_link.get(first).copied().flatten()?;
    if link >= close {
        return None;
    }
    Some((close, pieces.0.to_string(), pieces.1.to_string()))
}

fn strike_shed_close_at<'a, 't>(
    input: &'a [Option<Element<'t>>],
    index: usize,
) -> Option<(&'a str, &'a str)> {
    let Element::Text(value) = input.get(index)?.as_ref()? else {
        return None;
    };
    strike_close_split(value.as_ref())
}

pub(crate) fn element_contains_link(element: &Element<'_>) -> bool {
    matches!(element, Element::Link { .. })
        || formatter_children(element)
            .is_some_and(|children| children.iter().any(element_contains_link))
}

/// Prefix counts of "element contains a link", so a range query
/// elements[start..end].iter().any(element_contains_link) costs O(1)
/// instead of rescanning the range for every candidate the repair passes
/// consider. Without this, a container holding many marker candidates but
/// no link walks its whole child list once per candidate.
fn link_prefix_counts(elements: &[Element<'_>]) -> Vec<usize> {
    let mut counts = Vec::with_capacity(elements.len() + 1);
    counts.push(0);
    let mut total = 0usize;
    for element in elements {
        total += usize::from(element_contains_link(element));
        counts.push(total);
    }
    counts
}

/// link_prefix_counts over an input whose slots are taken as repairs run.
fn taken_link_prefix_counts(elements: &[Option<Element<'_>>]) -> Vec<usize> {
    let mut counts = Vec::with_capacity(elements.len() + 1);
    counts.push(0);
    let mut total = 0usize;
    for element in elements {
        if let Some(element) = element {
            total += usize::from(element_contains_link(element));
        }
        counts.push(total);
    }
    counts
}

/// For every position, the next index at or after it whose element matches
/// the predicate. One O(n) sweep replaces the per-opener forward scans that
/// repeated repair passes would otherwise replay over the same suffix.
fn next_matching_indices<'t>(
    elements: &[Option<Element<'t>>],
    matches: impl Fn(usize, &Element<'t>) -> bool,
) -> Vec<Option<usize>> {
    let mut result = vec![None; elements.len()];
    let mut next = None;
    for index in (0..elements.len()).rev() {
        if elements[index]
            .as_ref()
            .is_some_and(|element| matches(index, element))
        {
            next = Some(index);
        }
        result[index] = next;
    }
    result
}

fn is_shed_char(c: char) -> bool {
    matches!(
        c,
        ',' | '*'
            | '-'
            | '}'
            | '{'
            | '.'
            | ';'
            | ':'
            | '!'
            | ')'
            | ']'
            | '>'
            | '^'
            | '`'
            | '|'
            | '#'
    )
}

fn is_shed_text(value: &str) -> bool {
    !value.is_empty() && value.chars().all(is_shed_char)
}

fn is_script_shed_text(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| is_shed_char(c) || (!c.is_ascii() && !c.is_whitespace()))
}

fn shed_text_pair_split<'a>(value: &'a str, marker: &str) -> Option<(&'a str, &'a str)> {
    if !is_script_shed_text(value) {
        return None;
    }
    let start = value.find(marker)?;
    Some((&value[..start], &value[start + marker.len()..]))
}

pub(crate) fn monospace_close_split(value: &str) -> Option<(&str, &str)> {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'}' {
            index += 1;
            continue;
        }
        let run_end = index
            + bytes[index..]
                .iter()
                .take_while(|byte| **byte == b'}')
                .count();
        if run_end - index >= 2 {
            let before = &value[..index];
            if before.is_empty() || is_shed_text(before) {
                return Some((&value[..run_end - 2], &value[run_end..]));
            }
            return None;
        }
        index = run_end;
    }
    None
}

fn script_shell<'t>(kind: FormatKind) -> FormatShell<'t> {
    let ctype = match kind {
        FormatKind::Superscript => ContainerType::Superscript,
        FormatKind::Subscript => ContainerType::Subscript,
        _ => unreachable!("script shell requested for non-script kind"),
    };
    FormatShell::Container {
        kind,
        ctype,
        attributes: AttributeMap::new(),
    }
}

fn repair_script_split_close_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let mut output = Vec::with_capacity(elements.len());
    let mut changed = false;

    for element in mem::take(elements) {
        let kind = match &element {
            Element::Container(container) => match container.ctype() {
                ContainerType::Superscript => FormatKind::Superscript,
                ContainerType::Subscript => FormatKind::Subscript,
                _ => {
                    output.push(element);
                    continue;
                }
            },
            _ => {
                output.push(element);
                continue;
            }
        };
        let (shell, mut children) =
            FormatShell::into_parts(element).expect("formatter index has a shell");
        match script_split_close(&children, kind) {
            Some((child_index, prefix, suffix)) => {
                let mut tail: Vec<Element<'t>> =
                    children.drain(child_index + 1..).collect();
                children.truncate(child_index);
                if !prefix.is_empty() {
                    children.push(Element::Text(Cow::Owned(prefix)));
                }
                output.push(shell.build(children));
                if !suffix.is_empty() {
                    output.push(Element::Text(Cow::Owned(suffix)));
                }
                output.append(&mut tail);
                output.push(Element::Text(kind.close_marker().into()));
                changed = true;
            }
            None => output.push(shell.build(children)),
        }
    }

    *elements = output;
    changed
}

fn script_split_close(
    children: &[Element<'_>],
    kind: FormatKind,
) -> Option<(usize, String, String)> {
    let marker = kind.close_marker();
    let links = link_prefix_counts(children);
    for child_index in 1..children.len() {
        let Element::Text(value) = &children[child_index] else {
            continue;
        };
        let Some((prefix, suffix)) = shed_text_pair_split(value.as_ref(), marker) else {
            continue;
        };
        if prefix.is_empty() && element_ends_with_ascii_space(&children[child_index - 1])
        {
            continue;
        }
        if links[child_index] == links[0] {
            continue;
        }
        return Some((child_index, prefix.to_string(), suffix.to_string()));
    }
    None
}

fn repair_script_pairs_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let mut input = take_elements(elements);
    // Every opener would otherwise rescan the same suffix for a closer; one
    // suffix sweep per script marker answers all of them.
    let next_superscript = script_pair_candidates(&input, FormatKind::Superscript);
    let next_subscript = script_pair_candidates(&input, FormatKind::Subscript);
    let mut output = Vec::with_capacity(input.len());
    let mut changed = false;
    let mut index = 0;

    while index < input.len() {
        let kind = match input[index].as_ref() {
            Some(element) if is_exact_text(element, "^^") => FormatKind::Superscript,
            Some(element) if is_exact_text(element, ",,") => FormatKind::Subscript,
            _ => {
                output.push(take_element(&mut input, index));
                index += 1;
                continue;
            }
        };
        let candidates = match kind {
            FormatKind::Superscript => &next_superscript,
            _ => &next_subscript,
        };
        let Some((close, prefix, suffix)) =
            script_pair_close(&input, index, kind, candidates)
        else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };
        discard_range(&mut input, index..index + 1);
        let mut contents = take_range(&mut input, index + 1..close);
        if !prefix.is_empty() {
            contents.push(Element::Text(Cow::Owned(prefix)));
        }
        discard_range(&mut input, close..close + 1);
        output.push(script_shell(kind).build(contents));
        if !suffix.is_empty() {
            output.push(Element::Text(Cow::Owned(suffix)));
        }
        index = close + 1;
        changed = true;
    }

    *elements = output;
    changed
}

/// Suffix array of positions whose text can close a script span for `kind`.
fn script_pair_candidates(
    input: &[Option<Element<'_>>],
    kind: FormatKind,
) -> Vec<Option<usize>> {
    let marker = kind.close_marker();
    next_matching_indices(input, |index, element| {
        if index == 0 {
            return false;
        }
        let Element::Text(value) = element else {
            return false;
        };
        let Some((prefix, _)) = shed_text_pair_split(value.as_ref(), marker) else {
            return false;
        };
        !prefix.is_empty()
            || !input[index - 1]
                .as_ref()
                .is_some_and(|previous| element_ends_with_ascii_space(previous))
    })
}

fn shed_text_pair_at<'a>(
    element: &'a Element<'_>,
    marker: &str,
) -> Option<(&'a str, &'a str)> {
    let Element::Text(value) = element else {
        return None;
    };
    shed_text_pair_split(value.as_ref(), marker)
}

fn script_pair_close(
    input: &[Option<Element<'_>>],
    index: usize,
    kind: FormatKind,
    next_candidates: &[Option<usize>],
) -> Option<(usize, String, String)> {
    let next = input.get(index + 1)?.as_ref()?;
    if element_starts_with_ascii_space(next) {
        return None;
    }
    let marker = kind.close_marker();
    let mut close = next_candidates.get(index + 1).copied().flatten()?;
    let mut pieces = shed_text_pair_at(input.get(close)?.as_ref()?, marker)?;
    if close == index + 1 && pieces.0.is_empty() {
        close = next_candidates.get(close + 1).copied().flatten()?;
        pieces = shed_text_pair_at(input.get(close)?.as_ref()?, marker)?;
    }
    Some((close, pieces.0.to_string(), pieces.1.to_string()))
}

fn repair_monospace_split_close_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let mut output = Vec::with_capacity(elements.len());
    let mut changed = false;

    for element in mem::take(elements) {
        let is_monospace = matches!(
            &element,
            Element::Container(container) if container.ctype() == ContainerType::Monospace
        );
        if !is_monospace {
            output.push(element);
            continue;
        }
        let (shell, mut children) =
            FormatShell::into_parts(element).expect("formatter index has a shell");
        match monospace_split_close(&children) {
            Some((child_index, prefix, suffix)) => {
                let mut tail: Vec<Element<'t>> =
                    children.drain(child_index + 1..).collect();
                children.truncate(child_index);
                if !prefix.is_empty() {
                    children.push(Element::Text(Cow::Owned(prefix)));
                }
                output.push(shell.build(children));
                if !suffix.is_empty() {
                    output.push(Element::Text(Cow::Owned(suffix)));
                }
                output.append(&mut tail);
                output.push(Element::Text(FormatKind::Monospace.close_marker().into()));
                changed = true;
            }
            None => output.push(shell.build(children)),
        }
    }

    *elements = output;
    changed
}

fn monospace_split_close(children: &[Element<'_>]) -> Option<(usize, String, String)> {
    if children.len() < 3 {
        return None;
    }
    let links = link_prefix_counts(children);
    for child_index in 1..children.len() - 1 {
        let Element::Text(value) = &children[child_index] else {
            continue;
        };
        let Some((prefix, suffix)) = monospace_close_split(value.as_ref()) else {
            continue;
        };
        if prefix.is_empty() && element_ends_with_ascii_space(&children[child_index - 1])
        {
            continue;
        }
        if links[child_index] == links[0] {
            continue;
        }
        return Some((child_index, prefix.to_string(), suffix.to_string()));
    }
    None
}

fn repair_monospace_link_close_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let mut input = take_elements(elements);
    // Both repeated searches of this pass are answered from one sweep each:
    // the next child that can close a monospace span, and link presence in
    // the span between an opener and that close.
    let links = taken_link_prefix_counts(&input);
    let next_close = next_matching_indices(&input, |_, element| {
        text(element).is_some_and(|value| monospace_close_split(value).is_some())
    });
    let mut output = Vec::with_capacity(input.len());
    let mut changed = false;
    let mut index = 0;

    while index < input.len() {
        let is_opener = input[index]
            .as_ref()
            .is_some_and(|element| is_exact_text(element, "{{"));
        let close = is_opener
            .then(|| monospace_link_close(&input, index, &next_close))
            .flatten()
            .filter(|(close, _, _)| {
                // This repair exists to close a monospace span that the
                // base parser left open over a URL (issue #833). Brace
                // text around anything else is already owned content.
                links[*close] != links[index + 1]
            });
        let Some((close, prefix, suffix)) = close else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };
        discard_range(&mut input, index..index + 1);
        let mut contents = take_range(&mut input, index + 1..close);
        let padding = take_leading_monospace_padding(&mut contents);
        discard_range(&mut input, close..close + 1);
        if !prefix.is_empty() {
            contents.push(Element::Text(Cow::Owned(prefix)));
        }
        if let Some(padding) = padding
            && !output
                .last()
                .is_some_and(|element| element_ends_with_ascii_space(element))
            && !output.is_empty()
        {
            output.push(padding);
        }
        output.push(monospace_shell().build(contents));
        if !suffix.is_empty() {
            output.push(Element::Text(Cow::Owned(suffix)));
        }
        index = close + 1;
        changed = true;
    }

    *elements = output;
    changed
}

fn monospace_link_close(
    input: &[Option<Element<'_>>],
    opener: usize,
    next_close: &[Option<usize>],
) -> Option<(usize, String, String)> {
    let index = next_close.get(opener + 1).copied().flatten()?;
    let element = input.get(index)?.as_ref()?;
    let (prefix, suffix) = monospace_close_split(text(element)?)?;
    Some((index, prefix.to_string(), suffix.to_string()))
}

fn take_leading_monospace_padding<'t>(
    contents: &mut Vec<Element<'t>>,
) -> Option<Element<'t>> {
    let is_padding = contents.first().is_some_and(|element| {
        text(element).is_some_and(|value| {
            !value.is_empty() && value.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
        })
    });
    is_padding.then(|| contents.remove(0))
}

fn repair_wikidot_scheme_star_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let mut changed = false;
    let mut index = 1;

    while index < elements.len() {
        let is_scheme_link = match &elements[index] {
            Element::Link {
                link: LinkLocation::Url(url),
                label: LinkLabel::Url,
                target,
                ..
            } => target.is_none() && is_wikidot_only_autolink_url(url),
            _ => false,
        };
        let has_star =
            matches!(&elements[index - 1], Element::Text(value) if value.ends_with('*'));
        if is_scheme_link && has_star {
            if let Element::Text(value) = &mut elements[index - 1]
                && let Some(stripped) = value.strip_suffix('*')
            {
                *value = Cow::Owned(stripped.to_owned());
            }
            if matches!(&elements[index - 1], Element::Text(value) if value.is_empty()) {
                elements.remove(index - 1);
                index -= 1;
            }
            if let Element::Link { target, .. } = &mut elements[index] {
                *target = Some(AnchorTarget::NewTab);
            }
            changed = true;
        }
        index += 1;
    }

    changed
}

enum PairRole<'t> {
    FormatOpen(InlineFormatKind),
    FormatClose(InlineFormatKind),
    ColorOpen(Cow<'t, str>, bool),
    ColorClose,
    Drop,
    LiteralMarker(InlineFormatKind),
    LiteralText(&'static str),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProjItem<'a> {
    Text(&'a str),
    PartialLike,
    ScopeClose,
    Content,
    Barrier,
    EdgeOpen(InlineFormatKind),
    EdgeClose(InlineFormatKind),
    LinkContent,
    FormatEventOpen(InlineFormatKind),
    FormatEventClose(InlineFormatKind),
    ColorEventOpen,
    ColorEventClose,
    ColorEdgeOpen,
    ColorEdgeClose,
}

#[derive(Clone, Copy)]
enum Expansion {
    Format(InlineFormatKind),
    Color,
}

/// Format kinds whose parsed containers are re-paired as marker edges.
/// Bold, italics and underline are the symmetric literal markers that always
/// participate; strikethrough joins them because the parser can steal its
/// open or close marker into a nested container, leaving the real partner as
/// literal dash text.
const EDGE_MARKERS: [InlineFormatKind; 4] = [
    InlineFormatKind::Bold,
    InlineFormatKind::Italics,
    InlineFormatKind::Underline,
    InlineFormatKind::Strikethrough,
];

const MARKER_TEXTS: [&str; 6] = ["**", "//", "__", "{{", "}}", "##"];
const MONO_EXPANSION_MARKER_TEXTS: [&str; 4] = ["**", "//", "__", "##"];

pub(crate) fn normalize_wikidot_bibliography<'t>(bibliography: &mut Bibliography<'t>) {
    let mut segments: Vec<&mut Vec<Element<'t>>> = bibliography
        .slice_mut()
        .iter_mut()
        .map(|(_, elements)| elements)
        .collect();
    normalize_segments(&mut segments, 0);
}

fn normalize_segment_container<'t>(element: &mut Element<'t>) {
    // Segments of a NATIVE list are that list's items: marker pairing
    // treats them as INSIDE the list's top-level chunk (live pairs a
    // marker from an item into the list's nested items), so the
    // projection seeds them with the group's own family scope instead
    // of the outside scope.
    let seed_scope = match element {
        Element::List {
            wikidot_explicit_block: false,
            ..
        } => 1,
        _ => 0,
    };
    let mut segments = segment_vecs_mut(element);
    normalize_segments(&mut segments, seed_scope);
}

/// When an advanced table's fostered tail (the text a cell splice ejects in
/// front of the table) contains the closer of a marker opened inside one of
/// the table's cells, live still pairs them: the opener's scope flows across
/// the cell's list items and the closer is consumed. The tail reaches this
/// point wrapped in contiguous trailing `WikidotFoster` partials, so join
/// those tails behind the cell segments (their source order) and let the
/// marker pairing see the whole span. Only the fosters immediately before
/// the table join; anything else (a block frame, genuine preceding text)
/// keeps the historical cells-only grouping.
fn normalize_segment_container_with_foster<'t>(
    preceding: &mut Vec<Element<'t>>,
    container: &mut Element<'t>,
) -> bool {
    let Element::Table(table) = &container else {
        return false;
    };
    if table.table_type != TableType::Advanced {
        return false;
    }
    let mut run_start = preceding.len();
    while run_start > 0
        && matches!(
            preceding[run_start - 1],
            Element::Partial(PartialElement::WikidotFoster(_)) | Element::LineBreak
        )
    {
        run_start -= 1;
    }
    let fosters = preceding
        .iter()
        .skip(run_start)
        .filter(|element| {
            matches!(element, Element::Partial(PartialElement::WikidotFoster(_)))
        })
        .count();
    if fosters == 0 {
        return false;
    }
    let mut fosters_split: Vec<Element<'t>> = preceding.split_off(run_start);
    let mut segments = segment_vecs_mut(container);
    let cell_count = segments.len();
    for foster in fosters_split.iter_mut() {
        if let Element::Partial(PartialElement::WikidotFoster(tail)) = foster {
            segments.push(tail);
        }
    }
    if !segments
        .iter()
        .any(|segment| stream_has_pairing_material(segment))
    {
        // Deferred fosters were held raw so the tail markers could be
        // paired first; with no pairing material anywhere, normalize
        // them exactly as the standalone visit would have.
        for element in &mut fosters_split {
            visit_children_mut(element, normalize_sequence);
        }
        preceding.append(&mut fosters_split);
        normalize_segment_container(container);
        return true;
    }
    normalize_segments_foster_join(&mut segments, cell_count);
    preceding.append(&mut fosters_split);
    true
}

fn normalize_segments<'t>(segments: &mut [&mut Vec<Element<'t>>], seed_scope: usize) {
    normalize_segments_inner(segments, None, seed_scope);
}

fn normalize_segments_foster_join<'t>(
    segments: &mut [&mut Vec<Element<'t>>],
    cell_count: usize,
) {
    normalize_segments_inner(segments, Some(cell_count), 0);
}

fn normalize_segments_inner<'t>(
    segments: &mut [&mut Vec<Element<'t>>],
    foster_first_segment: Option<usize>,
    seed_scope: usize,
) {
    for _ in 0..MAX_NORMALIZATION_PASSES {
        let mut changed = match foster_first_segment {
            Some(first) => repair_literal_marker_pairs_segments_with_foster(
                segments, false, first, seed_scope,
            ),
            None => repair_literal_marker_pairs_segments(segments, false, seed_scope),
        };
        changed |= repair_dash_runs_segments(segments);
        for segment in segments.iter_mut() {
            changed |= normalize_sequence_passes(segment, false);
        }
        if !changed {
            break;
        }
    }
    for segment in segments.iter_mut() {
        hoist_kept_leading_space_pass(segment);
        normalize_sequence_children(segment);
        lower_dash_runs(segment);
        substitute_number_unit_spaces(segment);
    }
}

fn segment_vecs_mut<'a, 't>(
    element: &'a mut Element<'t>,
) -> Vec<&'a mut Vec<Element<'t>>> {
    let mut segments = Vec::new();
    match element {
        Element::List { items, .. } => collect_list_segments(items, &mut segments),
        Element::DefinitionList(items) => {
            for item in items {
                segments.push(&mut item.key_elements);
                segments.push(&mut item.value_elements);
            }
        }
        Element::Table(table) => {
            for row in &mut table.rows {
                for cell in &mut row.cells {
                    segments.push(table_cell_segment(&mut cell.elements));
                }
            }
        }
        _ => {}
    }
    segments
}

fn collect_list_segments<'a, 't>(
    items: &'a mut [ListItem<'t>],
    segments: &mut Vec<&'a mut Vec<Element<'t>>>,
) {
    for item in items {
        match item {
            ListItem::Elements { elements, .. } => segments.push(elements),
            ListItem::SubList { element } => {
                if let Element::List { items, .. } = &mut **element {
                    collect_list_segments(items, segments);
                }
            }
        }
    }
}

fn table_cell_segment<'a, 't>(
    elements: &'a mut Vec<Element<'t>>,
) -> &'a mut Vec<Element<'t>> {
    if elements.len() == 1
        && matches!(
            &elements[0],
            Element::Container(container)
                if container.ctype() == ContainerType::Paragraph
        )
    {
        match &mut elements[0] {
            Element::Container(container) => container.elements_mut(),
            _ => unreachable!(),
        }
    } else {
        elements
    }
}

#[derive(Clone, Copy, Default)]
struct ProjFlagCtx {
    in_native_list: bool,
    /// Whether the current position sits inside an explicit-list item, and
    /// whether that (nearest) item contains a native list.
    explicit_item_has_native: Option<bool>,
    /// Identity of the native-list chunk (0 = outside every native list).
    /// A native list's chunk is its whole nesting family: nested lists
    /// inside its items inherit the same scope (live pairs a marker from
    /// an item into its own nested list's items), and only entering a
    /// native list from outside starts a new chunk. Pairing across items
    /// is only performed within ONE native-list chunk: markers outside a
    /// native list do not pair with markers inside it. Explicit lists are
    /// transparent to the chunk.
    list_scope: usize,
    /// Allocator for native-list chunk identities, threaded across the
    /// segments of one segment group so distinct native lists get
    /// distinct identities.
    next_list_scope: usize,
}

#[derive(Clone, Copy)]
struct ProjFlags {
    in_native_list: bool,
    explicit_item_has_native: Option<bool>,
    list_scope: usize,
}

struct ProjSink<'b, 'el> {
    projection: &'b mut Vec<ProjItem<'el>>,
    flags: Option<&'b mut Vec<ProjFlags>>,
}

impl<'b, 'el> ProjSink<'b, 'el> {
    fn push(&mut self, item: ProjItem<'el>, ctx: &ProjFlagCtx) {
        if let Some(flags) = &mut self.flags {
            flags.push(ProjFlags {
                in_native_list: ctx.in_native_list,
                explicit_item_has_native: ctx.explicit_item_has_native,
                list_scope: ctx.list_scope,
            });
        }
        self.projection.push(item);
    }
}

fn project_marker_stream<'a, 't>(
    elements: &'a [Element<'t>],
    sink: &mut ProjSink<'_, 'a>,
    ctx: &mut ProjFlagCtx,
) {
    for element in elements {
        match element {
            Element::List {
                wikidot_explicit_block,
                items,
                ..
            } => {
                let saved_scope = ctx.list_scope;
                // The pairing chunk of a native list is its whole
                // nesting family: nested lists inside its items inherit
                // the same scope (live pairs a marker from an item into
                // its own nested list's items), and only entering a
                // native list from outside starts a new chunk.
                if !*wikidot_explicit_block && ctx.list_scope == 0 {
                    ctx.next_list_scope += 1;
                    ctx.list_scope = ctx.next_list_scope;
                }
                for item in items {
                    match item {
                        ListItem::Elements { elements, .. } => {
                            let saved_native = ctx.in_native_list;
                            let saved_explicit = ctx.explicit_item_has_native;
                            if *wikidot_explicit_block {
                                ctx.explicit_item_has_native = Some(
                                    elements.iter().any(crate::parsing::inline_scope::subtree_contains_native_list),
                                );
                            } else {
                                ctx.in_native_list = true;
                            }
                            project_marker_stream(elements, sink, ctx);
                            ctx.in_native_list = saved_native;
                            ctx.explicit_item_has_native = saved_explicit;
                        }
                        ListItem::SubList { element } => {
                            project_marker_stream(
                                std::slice::from_ref(element.as_ref()),
                                sink,
                                ctx,
                            );
                        }
                    }
                }
                ctx.list_scope = saved_scope;
            }
            _ => project_marker_sequence(std::slice::from_ref(element), sink, ctx),
        }
    }
}

fn apply_marker_stream<'t>(
    elements: Vec<Element<'t>>,
    roles: &[Option<PairRole<'t>>],
    cursor: &mut usize,
) -> Vec<Element<'t>> {
    let mut output = Vec::with_capacity(elements.len());
    let mut run: Vec<Element<'t>> = Vec::new();
    for element in elements {
        if matches!(element, Element::List { .. }) {
            if !run.is_empty() {
                output.extend(apply_marker_roles(
                    std::mem::take(&mut run),
                    roles,
                    cursor,
                ));
            }
            output.push(apply_marker_stream_list(element, roles, cursor));
        } else {
            run.push(element);
        }
    }
    if !run.is_empty() {
        output.extend(apply_marker_roles(run, roles, cursor));
    }
    output
}

fn apply_marker_stream_list<'t>(
    element: Element<'t>,
    roles: &[Option<PairRole<'t>>],
    cursor: &mut usize,
) -> Element<'t> {
    match element {
        Element::List {
            ltype,
            attributes,
            items,
            wikidot_explicit_block,
            wikidot_unclosed,
        } => Element::List {
            ltype,
            attributes,
            wikidot_explicit_block,
            wikidot_unclosed,
            items: items
                .into_iter()
                .map(|item| match item {
                    ListItem::Elements {
                        attributes,
                        elements,
                    } => ListItem::Elements {
                        attributes,
                        elements: apply_marker_stream(elements, roles, cursor),
                    },
                    ListItem::SubList { element } => ListItem::SubList {
                        element: Box::new(apply_marker_stream_list(
                            *element, roles, cursor,
                        )),
                    },
                })
                .collect(),
        },
        _ => element,
    }
}

fn stream_has_pairing_material(elements: &[Element<'_>]) -> bool {
    elements.iter().any(|element| match element {
        Element::List { items, .. } => items.iter().any(|item| match item {
            ListItem::Elements { elements, .. } => stream_has_pairing_material(elements),
            ListItem::SubList { element } => {
                stream_has_pairing_material(std::slice::from_ref(element.as_ref()))
            }
        }),
        _ => stream_marker_text_family(element),
    })
}

/// Whether a single stream element counts as segment pairing material:
/// the usual sequence material, plus the raw `--`/`^^`/`,,` markers,
/// which the segment (not sequence) pairing must see to claim them
/// across items before dash conversion or the script-level passes.
fn stream_marker_text_family(element: &Element<'_>) -> bool {
    if sequence_has_pairing_material(std::slice::from_ref(element)) {
        return true;
    }
    matches!(element, Element::Text(value) if matches!(value.as_ref(), "--" | ",," | "^^"))
}

fn dash_segment_placeholder<'t>() -> Element<'t> {
    Element::Container(Container::new(
        ContainerType::Italics,
        vec![Element::Text(Cow::Borrowed("\u{FFFC}"))],
        AttributeMap::new(),
    ))
}

fn is_dash_segment_placeholder(element: &Element<'_>) -> bool {
    match element {
        Element::Container(container) => {
            container.ctype() == ContainerType::Italics
                && container.attributes().get().is_empty()
                && match container.elements() {
                    [Element::Text(text)] => text.as_ref() == "\u{FFFC}",
                    _ => false,
                }
        }
        _ => false,
    }
}

fn repair_dash_runs_segments<'t>(segments: &mut [&mut Vec<Element<'t>>]) -> bool {
    if segments.len() < 2
        || !segments
            .iter()
            .any(|segment| segment.iter().any(subtree_has_dash_pair))
    {
        return false;
    }
    release_item_leading_strike_for_earlier_dash(segments);
    let mut joined: Vec<Element<'t>> = Vec::new();
    let mut placeholders: Vec<usize> = Vec::new();
    for (index, segment) in segments.iter_mut().enumerate() {
        if index > 0 {
            placeholders.push(joined.len());
            joined.push(dash_segment_placeholder());
        }
        joined.append(segment);
    }
    let mut changed = truncate_dash_run_containers(&mut joined);
    let paired = pair_crossing_dash_runs(&mut joined, &placeholders);
    changed |= paired;
    let mut split: Vec<Vec<Element<'t>>> =
        (0..segments.len()).map(|_| Vec::new()).collect();
    split_dash_joined(joined, &mut split);
    if paired {
        strip_crossed_dash_closer_space(&mut split);
    }
    for (segment, elements) in segments.iter_mut().zip(split) {
        **segment = elements;
    }
    changed
}

fn strip_crossed_dash_closer_space<'t>(split: &mut [Vec<Element<'t>>]) {
    for index in 0..split.len() {
        let close_then_space = split[index].first().is_some_and(is_strike_close)
            && split[index].get(1).is_some_and(is_ascii_space_text);
        if close_then_space {
            split[index].remove(1);
            continue;
        }
        let space_after_previous_close = index > 0
            && split[index - 1].last().is_some_and(is_strike_close)
            && split[index].first().is_some_and(is_ascii_space_text);
        if space_after_previous_close {
            split[index].remove(0);
        }
    }
}

fn release_item_leading_strike_for_earlier_dash<'t>(
    segments: &mut [&mut Vec<Element<'t>>],
) {
    let mut earlier_dash = false;
    for segment in segments.iter_mut() {
        if earlier_dash && segment.first().is_some_and(is_plain_strike) {
            let strike = segment.remove(0);
            let (_, children) =
                FormatShell::into_parts(strike).expect("plain strike has a body");
            let rest = std::mem::take(*segment);
            segment.push(Element::Text(Cow::Borrowed("--")));
            segment.extend(children);
            segment.push(Element::Text(Cow::Borrowed("--")));
            segment.extend(rest);
        } else if segment.first().is_some_and(is_dash_text) {
            earlier_dash = false;
        }
        if segment.last().is_some_and(is_dash_text) {
            earlier_dash = true;
        }
    }
}

fn is_dash_text(element: &Element<'_>) -> bool {
    matches!(element, Element::Text(text) if text.as_ref() == "--")
}

fn is_strike_close(element: &Element<'_>) -> bool {
    matches!(
        element,
        Element::Partial(PartialElement::InlineFormatClose(
            InlineFormatKind::Strikethrough,
        ))
    )
}

fn is_ascii_space_text(element: &Element<'_>) -> bool {
    matches!(
        element,
        Element::Text(text)
            if !text.is_empty() && text.bytes().all(|byte| byte == b' ' || byte == b'\t')
    )
}

fn pair_crossing_dash_runs<'t>(
    elements: &mut Vec<Element<'t>>,
    placeholders: &[usize],
) -> bool {
    let mut input = take_elements(elements);
    let mut output = Vec::with_capacity(input.len());
    let mut changed = false;
    let mut index = 0;

    while index < input.len() {
        let scan = build_dash_scan(
            &|element_index| {
                input
                    .get(element_index)
                    .and_then(|element| element.as_ref())
            },
            input.len(),
            index,
            // A link is one atom. A `--` after the link still pairs with a
            // later list item, which is what live does.
            true,
        );
        if scan.end == index {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        }
        let boundary_positions: Vec<usize> = scan
            .units
            .iter()
            .filter_map(|unit| match unit {
                DashScanUnit::Atom { element, pos } if placeholders.contains(element) => {
                    Some(*pos)
                }
                _ => None,
            })
            .collect();
        let pairs: Vec<(usize, usize)> = dash_stream_pairs(&scan.stream)
            .into_iter()
            .filter(|&(open, close)| {
                boundary_positions
                    .iter()
                    .any(|&pos| open < pos && pos < close)
            })
            .collect();
        if pairs.is_empty() {
            for element_index in index..scan.end {
                output.push(take_element(&mut input, element_index));
            }
            index = scan.end;
            continue;
        }
        changed = true;
        emit_dash_pairs(&mut input, &scan, &pairs, &mut output);
        index = scan.end;
    }

    *elements = output;
    changed
}

fn split_dash_joined<'t>(joined: Vec<Element<'t>>, segments: &mut [Vec<Element<'t>>]) {
    let mut current = 0;
    for element in joined {
        if is_dash_segment_placeholder(&element) {
            current += 1;
            continue;
        }
        let splits = matches!(
            &element,
            Element::Container(container)
                if container.ctype() == ContainerType::Strikethrough
                    && container
                        .elements()
                        .iter()
                        .any(is_dash_segment_placeholder)
        );
        if splits {
            let Element::Container(container) = element else {
                unreachable!();
            };
            let children = Vec::from(container);
            let mut pieces: Vec<Vec<Element<'t>>> = vec![Vec::new()];
            for child in children {
                if is_dash_segment_placeholder(&child) {
                    pieces.push(Vec::new());
                } else {
                    pieces.last_mut().expect("piece exists").push(child);
                }
            }
            let last = pieces.len() - 1;
            for (index, piece) in pieces.into_iter().enumerate() {
                let target = &mut segments[current + index];
                if index == 0 {
                    target.push(Element::Partial(PartialElement::InlineFormatOpen(
                        InlineFormatKind::Strikethrough,
                    )));
                }
                target.extend(piece);
                if index == last {
                    target.push(Element::Partial(PartialElement::InlineFormatClose(
                        InlineFormatKind::Strikethrough,
                    )));
                }
            }
            current += last;
            continue;
        }
        segments[current].push(element);
    }
}

fn repair_literal_marker_pairs_segments<'t>(
    segments: &mut [&mut Vec<Element<'t>>],
    inside_monospace: bool,
    seed_scope: usize,
) -> bool {
    if !segments
        .iter()
        .any(|segment| stream_has_pairing_material(segment))
    {
        return false;
    }
    let mut originals: Vec<Vec<Element<'t>>> = Vec::with_capacity(segments.len());
    for segment in segments.iter_mut() {
        originals.push(
            take_elements(segment)
                .into_iter()
                .map(Option::unwrap)
                .collect(),
        );
    }
    let mut projection = Vec::new();
    let mut flags = Vec::new();
    let mut next_scope = seed_scope;
    for original in &originals {
        let mut sink = ProjSink {
            projection: &mut projection,
            flags: Some(&mut flags),
        };
        let mut ctx = ProjFlagCtx {
            next_list_scope: next_scope,
            list_scope: seed_scope,
            ..ProjFlagCtx::default()
        };
        project_marker_stream(original, &mut sink, &mut ctx);
        next_scope = ctx.next_list_scope;
    }
    let mut roles: Vec<Option<PairRole<'t>>> = Vec::new();
    roles.resize_with(projection.len(), || None);
    let mut found = false;
    for kind in EDGE_MARKERS {
        found |= pair_symmetric_markers(
            &projection,
            &mut roles,
            kind,
            kind == InlineFormatKind::Strikethrough,
            None,
            None,
            Some(&flags),
        );
    }
    // Script formats cross items the same way (live pairs them across
    // nesting), but they are not EDGE_MARKERS: their containers are not
    // re-paired as marker edges.
    for kind in [InlineFormatKind::Subscript, InlineFormatKind::Superscript] {
        found |= pair_symmetric_markers(
            &projection,
            &mut roles,
            kind,
            false,
            None,
            None,
            Some(&flags),
        );
    }
    if !inside_monospace {
        found |=
            pair_monospace_markers(&projection, &mut roles, None, Some(&flags), None);
    }
    found |= pair_color_markers(&projection, &mut roles);
    if !found {
        for (segment, original) in segments.iter_mut().zip(originals) {
            **segment = original;
        }
        return false;
    }
    let mut cursor = 0;
    for (segment, original) in segments.iter_mut().zip(originals) {
        **segment = apply_marker_stream(original, &roles, &mut cursor);
    }
    debug_assert_eq!(cursor, roles.len());
    true
}

fn repair_literal_marker_pairs_segments_with_foster<'t>(
    segments: &mut [&mut Vec<Element<'t>>],
    inside_monospace: bool,
    foster_first_segment: usize,
    seed_scope: usize,
) -> bool {
    if !segments
        .iter()
        .any(|segment| stream_has_pairing_material(segment))
    {
        return false;
    }
    let mut originals: Vec<Vec<Element<'t>>> = Vec::with_capacity(segments.len());
    for segment in segments.iter_mut() {
        originals.push(
            take_elements(segment)
                .into_iter()
                .map(Option::unwrap)
                .collect(),
        );
    }
    let mut projection = Vec::new();
    let mut flags = Vec::new();
    let mut ranges = Vec::with_capacity(originals.len());
    let mut next_scope = seed_scope;
    for original in &originals {
        let start = projection.len();
        {
            let mut sink = ProjSink {
                projection: &mut projection,
                flags: Some(&mut flags),
            };
            let mut ctx = ProjFlagCtx {
                next_list_scope: next_scope,
                list_scope: seed_scope,
                ..ProjFlagCtx::default()
            };
            project_marker_stream(original, &mut sink, &mut ctx);
            next_scope = ctx.next_list_scope;
        }
        ranges.push((start, projection.len()));
    }
    let foster_start = ranges
        .get(foster_first_segment)
        .map_or(projection.len(), |(start, _)| *start);
    let mut roles: Vec<Option<PairRole<'t>>> = Vec::new();
    roles.resize_with(projection.len(), || None);
    let mut found = false;
    let mut pairs: Vec<(usize, usize, InlineFormatKind)> = Vec::new();
    for kind in EDGE_MARKERS {
        found |= pair_symmetric_markers(
            &projection,
            &mut roles,
            kind,
            kind == InlineFormatKind::Strikethrough,
            Some(&mut pairs),
            Some(foster_start),
            Some(&flags),
        );
    }
    // Script formats cross items and the splice the same way (live
    // pairs them across nesting and into the fostered tail).
    for kind in [InlineFormatKind::Subscript, InlineFormatKind::Superscript] {
        found |= pair_symmetric_markers(
            &projection,
            &mut roles,
            kind,
            false,
            Some(&mut pairs),
            Some(foster_start),
            Some(&flags),
        );
    }
    if !inside_monospace {
        found |= pair_monospace_markers(
            &projection,
            &mut roles,
            Some(&mut pairs),
            Some(&flags),
            Some(foster_start),
        );
    }
    found |= pair_color_markers(&projection, &mut roles);
    if !found {
        for (segment, original) in segments.iter_mut().zip(originals) {
            **segment = original;
        }
        return false;
    }
    // On the foster path a marker pair whose closer lives in the fostered
    // tail (source-after the cells) cannot be kept as open/close partials:
    // lowering walks the tail before the table, so it would never see the
    // opener first. Live still consumes the closer and keeps the scope
    // flowing across the cell's list items, so instead the closer marker is
    // dropped and a close partial is appended at the end of the cell
    // segment the opener lives in. Pairs whose opener does not satisfy the
    // cell-list context (live's rule: the opener must be inside a native
    // list, or inside an explicit-list item with no native list) revert to
    // literal, exactly as if the tail had never joined the segment group.
    let mut spliced_closes: Vec<(InlineFormatKind, usize)> = Vec::new();
    for (open, close, kind) in &pairs {
        if *close < foster_start || *open >= foster_start {
            continue;
        }
        if !matches!(projection[*open], ProjItem::Text(_)) {
            roles[*open] = None;
            roles[*close] = None;
            continue;
        }
        let flag = flags[*open];
        // Live's cell-list context: the opener must live inside an
        // explicit-list item in the cell (the li body is the pairing
        // chunk), either inside a native list there or in an item with no
        // native list at all. A native list directly in the cell is
        // truncated by the cell splice before the tail starts, so its
        // opener has no explicit context and stays literal.
        let opener_in_cell_list_context = flag.explicit_item_has_native.is_some()
            && (flag.in_native_list || flag.explicit_item_has_native == Some(false));
        // The fostered scope only flows when the opener's native list
        // reaches the splice: any real content outside a native list
        // between the opener and the end of the cell (a plain non-list
        // line such as `zzz` after the nested list) ends the native
        // list's chunk, and its fostered closer never joins it.
        let flow_broken = projection[*open + 1..foster_start].iter().enumerate().any(
            |(offset, item)| {
                let index_in_stream = *open + 1 + offset;
                flags[index_in_stream].list_scope == 0
                    && matches!(
                        item,
                        ProjItem::Text(value) if !value.trim().is_empty()
                    )
            },
        );
        if opener_in_cell_list_context && !flow_broken {
            roles[*close] = Some(PairRole::Drop);
            spliced_closes.push((*kind, *open));
        } else {
            roles[*open] = None;
            roles[*close] = None;
        }
    }
    let mut cursor = 0;
    for (segment, original) in segments.iter_mut().zip(originals) {
        **segment = apply_marker_stream(original, &roles, &mut cursor);
    }
    debug_assert_eq!(cursor, roles.len());
    for (kind, open) in spliced_closes {
        let segment_index = ranges
            .iter()
            .position(|(start, end)| open >= *start && open < *end)
            .expect("pair open index is inside a segment range");
        trim_list_end_space(segments[segment_index]);
        segments[segment_index]
            .push(Element::Partial(PartialElement::InlineFormatClose(kind)));
    }
    true
}

/// The spliced closer's scope ends at the cell end, which is the end of the
/// list the splice cut. Live eats the whitespace text at the end of the cut
/// item (the space before the splice token), so the scope it wraps does not
/// carry that trailing space.
fn trim_list_end_space(elements: &mut Vec<Element<'_>>) {
    match elements.last_mut() {
        Some(Element::Text(text)) => {
            let trimmed_len = text.trim_end_matches([' ', '\t']).len();
            if trimmed_len == 0 {
                elements.pop();
                trim_list_end_space(elements);
            } else if trimmed_len < text.len() {
                *text = Cow::Owned(text[..trimmed_len].to_owned());
            }
        }
        Some(Element::List { items, .. }) => trim_list_end_space_items(items),
        _ => {}
    }
}

fn trim_list_end_space_items(items: &mut [ListItem<'_>]) {
    match items.last_mut() {
        Some(ListItem::Elements { elements, .. }) => trim_list_end_space(elements),
        Some(ListItem::SubList { element }) => {
            if let Element::List { items, .. } = &mut **element {
                trim_list_end_space_items(items);
            }
        }
        _ => {}
    }
}

fn repair_literal_marker_pairs_pass<'t>(
    elements: &mut Vec<Element<'t>>,
    inside_monospace: bool,
) -> bool {
    if !sequence_has_pairing_material(elements) {
        return false;
    }
    let original: Vec<Element<'t>> = take_elements(elements)
        .into_iter()
        .map(Option::unwrap)
        .collect();
    let mut projection = Vec::new();
    let mut sink = ProjSink {
        projection: &mut projection,
        flags: None,
    };
    let ctx = ProjFlagCtx::default();
    project_marker_sequence(&original, &mut sink, &ctx);
    let mut roles: Vec<Option<PairRole<'t>>> = Vec::new();
    roles.resize_with(projection.len(), || None);
    let mut found = false;
    for kind in EDGE_MARKERS {
        found |= pair_symmetric_markers(
            &projection,
            &mut roles,
            kind,
            kind == InlineFormatKind::Strikethrough,
            None,
            None,
            None,
        );
    }
    if !inside_monospace {
        found |= pair_monospace_markers(&projection, &mut roles, None, None, None);
    }
    found |= pair_color_markers(&projection, &mut roles);
    if !found {
        *elements = original;
        return false;
    }
    let mut cursor = 0;
    let output = apply_marker_roles(original, &roles, &mut cursor);
    debug_assert_eq!(cursor, roles.len());
    *elements = output;
    true
}

fn sequence_has_pairing_material(elements: &[Element<'_>]) -> bool {
    elements.iter().any(|element| {
        matches!(element, Element::Text(value) if MARKER_TEXTS.contains(&value.as_ref()))
            || formatter_children(element).is_some()
    })
}

fn project_marker_sequence<'a, 't>(
    elements: &'a [Element<'t>],
    sink: &mut ProjSink<'_, 'a>,
    ctx: &ProjFlagCtx,
) {
    for element in elements {
        if let Some(expansion) = expansion_of(element) {
            let children = formatter_children(element)
                .expect("expansion requires formatter children");
            match expansion {
                Expansion::Format(kind) if EDGE_MARKERS.contains(&kind) => {
                    sink.push(ProjItem::EdgeOpen(kind), ctx);
                    project_marker_sequence(children, sink, ctx);
                    sink.push(ProjItem::EdgeClose(kind), ctx);
                }
                Expansion::Color => {
                    sink.push(ProjItem::ColorEdgeOpen, ctx);
                    project_marker_sequence(children, sink, ctx);
                    sink.push(ProjItem::ColorEdgeClose, ctx);
                }
                _ => {
                    sink.push(ProjItem::PartialLike, ctx);
                    project_marker_sequence(children, sink, ctx);
                    sink.push(ProjItem::PartialLike, ctx);
                }
            }
        } else if pairing_barrier(element) {
            sink.push(ProjItem::Barrier, ctx);
        } else {
            match element {
                Element::Text(value) => sink.push(ProjItem::Text(value), ctx),
                Element::Partial(partial) => sink.push(
                    match partial {
                        PartialElement::InlineFormatOpen(kind) => {
                            ProjItem::FormatEventOpen(*kind)
                        }
                        PartialElement::InlineFormatClose(kind) => {
                            ProjItem::FormatEventClose(*kind)
                        }
                        PartialElement::InlineColorOpen(..) => ProjItem::ColorEventOpen,
                        PartialElement::InlineColorClose => ProjItem::ColorEventClose,
                        PartialElement::InlineSpanClose(_)
                        | PartialElement::InlineSizeClose(_) => ProjItem::ScopeClose,
                        _ => ProjItem::PartialLike,
                    },
                    ctx,
                ),
                _ if element_contains_link(element) => {
                    sink.push(ProjItem::LinkContent, ctx)
                }
                _ => sink.push(ProjItem::Content, ctx),
            }
        }
    }
}

fn apply_marker_roles<'t>(
    elements: Vec<Element<'t>>,
    roles: &[Option<PairRole<'t>>],
    cursor: &mut usize,
) -> Vec<Element<'t>> {
    let mut output = Vec::with_capacity(elements.len());
    for element in elements {
        if let Some(expansion) = expansion_of(&element) {
            let open_index = *cursor;
            *cursor += 1;
            let Some((shell, children)) = FormatShell::into_parts(element) else {
                unreachable!("expansion requires a formatter shell");
            };
            let children = apply_marker_roles(children, roles, cursor);
            let close_index = *cursor;
            *cursor += 1;
            let subtree_changed =
                roles[open_index..=close_index].iter().any(Option::is_some);
            if !subtree_changed {
                output.push(shell.build(children));
                continue;
            }
            push_edge_replacement(
                &mut output,
                &roles[open_index],
                expansion,
                &shell,
                true,
            );
            output.extend(children);
            push_edge_replacement(
                &mut output,
                &roles[close_index],
                expansion,
                &shell,
                false,
            );
            continue;
        }
        let index = *cursor;
        *cursor += 1;
        match &roles[index] {
            None | Some(PairRole::LiteralMarker(_)) => output.push(element),
            Some(PairRole::FormatOpen(kind)) => {
                output.push(Element::Partial(PartialElement::InlineFormatOpen(*kind)));
            }
            Some(PairRole::FormatClose(kind)) => {
                output.push(Element::Partial(PartialElement::InlineFormatClose(*kind)));
            }
            Some(PairRole::ColorOpen(color, background)) => {
                output.push(Element::Partial(PartialElement::InlineColorOpen(
                    color.clone(),
                    *background,
                )));
            }
            Some(PairRole::ColorClose) => {
                output.push(Element::Partial(PartialElement::InlineColorClose));
            }
            Some(PairRole::LiteralText(text)) => {
                output.push(Element::Text(Cow::Borrowed(text)));
            }
            Some(PairRole::Drop) => {}
        }
    }
    output
}

fn push_edge_replacement<'t>(
    output: &mut Vec<Element<'t>>,
    role: &Option<PairRole<'t>>,
    expansion: Expansion,
    shell: &FormatShell<'t>,
    open_edge: bool,
) {
    match role {
        Some(PairRole::FormatOpen(kind)) => {
            output.push(Element::Partial(PartialElement::InlineFormatOpen(*kind)));
        }
        Some(PairRole::FormatClose(kind)) => {
            output.push(Element::Partial(PartialElement::InlineFormatClose(*kind)));
        }
        Some(PairRole::ColorOpen(color, background)) => {
            output.push(Element::Partial(PartialElement::InlineColorOpen(
                color.clone(),
                *background,
            )));
        }
        Some(PairRole::ColorClose) => {
            output.push(Element::Partial(PartialElement::InlineColorClose));
        }
        Some(PairRole::Drop) => {}
        Some(PairRole::LiteralMarker(kind)) => {
            output.push(Element::Text(Cow::Borrowed(kind.open_marker())));
        }
        Some(PairRole::LiteralText(text)) => {
            output.push(Element::Text(Cow::Borrowed(text)));
        }
        None => match expansion {
            Expansion::Format(kind) => {
                output.push(Element::Partial(if open_edge {
                    PartialElement::InlineFormatOpen(kind)
                } else {
                    PartialElement::InlineFormatClose(kind)
                }));
            }
            Expansion::Color => {
                if let FormatShell::Color { color, background } = shell {
                    if open_edge {
                        output.push(Element::Partial(PartialElement::InlineColorOpen(
                            color.clone(),
                            *background,
                        )));
                    } else {
                        output.push(Element::Partial(PartialElement::InlineColorClose));
                    }
                }
            }
        },
    }
}

fn expansion_of(element: &Element<'_>) -> Option<Expansion> {
    if let Element::Container(container) = element
        && container.attributes().get().is_empty()
        && !subtree_has_barrier(element)
    {
        match kind_from_container(container.ctype()) {
            Some(FormatKind::Bold) => {
                return Some(Expansion::Format(InlineFormatKind::Bold));
            }
            Some(FormatKind::Italics) => {
                return Some(Expansion::Format(InlineFormatKind::Italics));
            }
            Some(FormatKind::Underline) => {
                return Some(Expansion::Format(InlineFormatKind::Underline));
            }
            _ => {}
        }
    }
    let marker_texts: &[&str] = match element {
        Element::Container(container)
            if container.ctype() == ContainerType::Monospace =>
        {
            &MONO_EXPANSION_MARKER_TEXTS
        }
        _ => &MARKER_TEXTS,
    };
    if !subtree_has_marker_text(element, marker_texts) {
        return None;
    }
    match element {
        Element::Color { .. } => Some(Expansion::Color),
        Element::Container(container) => {
            if !container.attributes().get().is_empty() {
                return None;
            }
            match kind_from_container(container.ctype())? {
                FormatKind::Bold => Some(Expansion::Format(InlineFormatKind::Bold)),
                FormatKind::Italics => Some(Expansion::Format(InlineFormatKind::Italics)),
                FormatKind::Underline => {
                    Some(Expansion::Format(InlineFormatKind::Underline))
                }
                FormatKind::Subscript => {
                    Some(Expansion::Format(InlineFormatKind::Subscript))
                }
                FormatKind::Superscript => {
                    Some(Expansion::Format(InlineFormatKind::Superscript))
                }
                FormatKind::Monospace => {
                    Some(Expansion::Format(InlineFormatKind::Monospace))
                }
                FormatKind::Strike => {
                    Some(Expansion::Format(InlineFormatKind::Strikethrough))
                }
                FormatKind::Color => None,
            }
        }
        _ => None,
    }
}

fn subtree_has_marker_text(element: &Element<'_>, marker_texts: &[&str]) -> bool {
    match element {
        Element::Text(value) => marker_texts.contains(&value.as_ref()),
        _ => formatter_children(element).is_some_and(|children| {
            children
                .iter()
                .any(|child| subtree_has_marker_text(child, marker_texts))
        }),
    }
}

fn subtree_has_barrier(element: &Element<'_>) -> bool {
    if pairing_barrier(element) {
        return true;
    }
    formatter_children(element)
        .is_some_and(|children| children.iter().any(subtree_has_barrier))
}

fn pairing_barrier(element: &Element<'_>) -> bool {
    match element {
        Element::Container(container) => kind_from_container(container.ctype()).is_none(),
        Element::Raw(_)
        | Element::Anchor { .. }
        | Element::Collapsible { .. }
        | Element::Table(_)
        | Element::TabView(_)
        | Element::List { .. }
        | Element::DefinitionList(_)
        | Element::Include { .. }
        | Element::HorizontalRule
        | Element::TableOfContents { .. }
        | Element::FootnoteBlock { .. } => true,
        Element::Partial(partial) => matches!(
            partial,
            PartialElement::ListItem(_)
                | PartialElement::TableRow(_)
                | PartialElement::TableCell(_)
                | PartialElement::Tab(_)
                | PartialElement::WikidotBlockOpen(
                    WikidotBlockShell::Div { .. }
                        | WikidotBlockShell::Note { .. }
                        | WikidotBlockShell::Collapsible { .. }
                )
        ),
        _ => false,
    }
}

fn proj_starts_with_pad(item: &ProjItem<'_>) -> bool {
    matches!(item, ProjItem::Text(value) if value.starts_with(' ') || value.starts_with('\t'))
}

fn proj_ends_with_pad(item: &ProjItem<'_>) -> bool {
    matches!(item, ProjItem::Text(value) if value.ends_with(' ') || value.ends_with('\t'))
}

fn projection_has_content(items: &[ProjItem<'_>]) -> bool {
    items.iter().any(|item| match item {
        ProjItem::Text(value) => !value.trim().is_empty(),
        ProjItem::Content | ProjItem::LinkContent | ProjItem::ScopeClose => true,
        ProjItem::PartialLike
        | ProjItem::Barrier
        | ProjItem::EdgeOpen(_)
        | ProjItem::EdgeClose(_)
        | ProjItem::FormatEventOpen(_)
        | ProjItem::FormatEventClose(_)
        | ProjItem::ColorEventOpen
        | ProjItem::ColorEventClose
        | ProjItem::ColorEdgeOpen
        | ProjItem::ColorEdgeClose => false,
    })
}

fn pair_symmetric_markers<'t>(
    projection: &[ProjItem<'_>],
    roles: &mut [Option<PairRole<'t>>],
    kind: InlineFormatKind,
    edges_required: bool,
    mut pairs: Option<&mut Vec<(usize, usize, InlineFormatKind)>>,
    foster_start: Option<usize>,
    scopes: Option<&[ProjFlags]>,
) -> bool {
    // A marker pair may only cross items of the SAME native list (live
    // treats each native list as its own pairing chunk; markers outside
    // a native list never pair with markers inside it). A pair whose
    // closer is in the fostered tail is exempt from the scope rule: the
    // caller's splice-context filter governs those pairs.
    let is_foster_cross = |open_index: usize, index: usize| -> bool {
        foster_start.is_some_and(|start| open_index < start && index >= start)
    };
    let same_scope = |open_index: usize, index: usize| -> bool {
        scopes.is_none_or(|flags| flags[open_index].list_scope == flags[index].list_scope)
    };
    let marker = kind.open_marker();
    let mut siblings: Vec<Option<usize>> = vec![None; projection.len()];
    let mut edge_stack: Vec<usize> = Vec::new();
    for (index, item) in projection.iter().enumerate() {
        match item {
            ProjItem::EdgeOpen(edge_kind) if *edge_kind == kind => {
                edge_stack.push(index);
            }
            ProjItem::EdgeClose(edge_kind) if *edge_kind == kind => {
                if let Some(open_index) = edge_stack.pop() {
                    siblings[open_index] = Some(index);
                    siblings[index] = Some(open_index);
                }
            }
            _ => {}
        }
    }
    let mut edge_active = vec![false; projection.len()];
    let mut segment_start = 0;
    let mut segment_has_literal = false;
    for index in 0..=projection.len() {
        if index == projection.len() || matches!(projection[index], ProjItem::Barrier) {
            if segment_has_literal {
                edge_active[segment_start..index].fill(true);
            }
            segment_start = index + 1;
            segment_has_literal = false;
        } else if matches!(projection[index], ProjItem::Text(value) if value == marker) {
            segment_has_literal = true;
        }
    }
    let is_candidate = |index: usize| -> bool {
        match projection[index] {
            ProjItem::Text(value) => value == marker,
            ProjItem::EdgeOpen(edge_kind) | ProjItem::EdgeClose(edge_kind) => {
                edge_kind == kind && edge_active[index]
            }
            _ => false,
        }
    };
    let mut found = false;
    let mut open: Option<usize> = None;
    let mut depth = 0usize;
    for (index, item) in projection.iter().enumerate() {
        if matches!(item, ProjItem::Barrier) {
            open = None;
            continue;
        }
        match item {
            ProjItem::FormatEventOpen(event_kind) if *event_kind == kind => {
                depth += 1;
                continue;
            }
            ProjItem::FormatEventClose(event_kind) if *event_kind == kind => {
                depth = depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }
        if depth > 0 || !is_candidate(index) {
            continue;
        }
        match open {
            None => {
                if projection
                    .get(index + 1)
                    .is_some_and(|next| !proj_starts_with_pad(next))
                {
                    open = Some(index);
                }
            }
            Some(open_index) => {
                // The native-list scope barrier: a closer in another
                // chunk than the opener was left for cannot close it,
                // exactly as when an explicit barrier intervenes.
                if !same_scope(open_index, index) && !is_foster_cross(open_index, index) {
                    open = None;
                    continue;
                }
                let pairs_edges = matches!(
                    projection[open_index],
                    ProjItem::EdgeOpen(_) | ProjItem::EdgeClose(_)
                ) || matches!(
                    projection[index],
                    ProjItem::EdgeOpen(_) | ProjItem::EdgeClose(_)
                );
                // On the foster path a raw `--` closer in the fostered
                // tail still pairs with a raw `--` opener in the cells
                // (live pairs across the splice before dash conversion
                // claims the `--`); the caller's splice-context filter
                // then keeps or reverts the pair. Tail-internal pairs
                // stay with the sequence-level strike recovery.
                let raw_foster_pair = foster_start.is_some_and(|start| {
                    open_index < start
                        && index >= start
                        && matches!(projection[open_index], ProjItem::Text(_))
                        && matches!(projection[index], ProjItem::Text(_))
                });
                if edges_required && !pairs_edges && !raw_foster_pair {
                    open = Some(index);
                    continue;
                }
                if !proj_ends_with_pad(&projection[index - 1])
                    && projection_has_content(&projection[open_index + 1..index])
                {
                    if siblings[open_index] != Some(index)
                        && roles[open_index].is_none()
                        && roles[index].is_none()
                    {
                        roles[open_index] = Some(PairRole::FormatOpen(kind));
                        roles[index] = Some(PairRole::FormatClose(kind));
                        if let Some(pairs) = &mut pairs {
                            pairs.push((open_index, index, kind));
                        }
                        for endpoint in [open_index, index] {
                            if let Some(sibling) = siblings[endpoint]
                                && roles[sibling].is_none()
                            {
                                roles[sibling] = Some(PairRole::LiteralMarker(kind));
                            }
                        }
                        for swept in open_index..=index {
                            let is_edge = matches!(
                                projection[swept],
                                ProjItem::EdgeOpen(edge_kind)
                                    | ProjItem::EdgeClose(edge_kind)
                                if edge_kind == kind
                            );
                            if is_edge && roles[swept].is_none() {
                                roles[swept] = Some(PairRole::LiteralMarker(kind));
                                if let Some(sibling) = siblings[swept]
                                    && roles[sibling].is_none()
                                {
                                    roles[sibling] = Some(PairRole::LiteralMarker(kind));
                                }
                            }
                        }
                        found = true;
                    }
                    open = None;
                }
            }
        }
    }
    found
}

fn pair_monospace_markers<'t>(
    projection: &[ProjItem<'_>],
    roles: &mut [Option<PairRole<'t>>],
    mut pairs: Option<&mut Vec<(usize, usize, InlineFormatKind)>>,
    scopes: Option<&[ProjFlags]>,
    foster_start: Option<usize>,
) -> bool {
    let mut found = false;
    let mut open: Option<usize> = None;
    let mut depth = 0usize;
    for (index, item) in projection.iter().enumerate() {
        if matches!(item, ProjItem::Barrier) {
            open = None;
            continue;
        }
        match item {
            ProjItem::FormatEventOpen(InlineFormatKind::Monospace) => {
                depth += 1;
                continue;
            }
            ProjItem::FormatEventClose(InlineFormatKind::Monospace) => {
                depth = depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }
        if depth > 0 {
            continue;
        }
        match open {
            None => {
                if matches!(item, ProjItem::Text(value) if *value == "{{") {
                    open = Some(index);
                }
            }
            Some(open_index) => {
                // Same native-list scope barrier as `pair_symmetric_markers`.
                if scopes.is_some_and(|flags| {
                    flags[open_index].list_scope != flags[index].list_scope
                }) && !foster_start
                    .is_some_and(|start| open_index < start && index >= start)
                {
                    open = None;
                    continue;
                }
                if matches!(item, ProjItem::Text(value) if *value == "}}")
                    && projection_has_content(&projection[open_index + 1..index])
                {
                    if projection[open_index + 1..index]
                        .iter()
                        .any(|item| matches!(item, ProjItem::LinkContent))
                    {
                        open = None;
                        continue;
                    }
                    if roles[open_index].is_none() && roles[index].is_none() {
                        roles[open_index] =
                            Some(PairRole::FormatOpen(InlineFormatKind::Monospace));
                        roles[index] =
                            Some(PairRole::FormatClose(InlineFormatKind::Monospace));
                        if let Some(pairs) = &mut pairs {
                            pairs.push((open_index, index, InlineFormatKind::Monospace));
                        }
                        found = true;
                    }
                    open = None;
                }
            }
        }
    }
    found
}

enum ColorHead<'t> {
    Literal(Cow<'t, str>, bool),
    Container(usize),
}

fn pair_color_markers<'t>(
    projection: &[ProjItem<'_>],
    roles: &mut [Option<PairRole<'t>>],
) -> bool {
    // Container edge siblings: a color container whose open edge is a head
    // knows the position of its parsed close marker.
    let mut color_edges: Vec<Option<usize>> = vec![None; projection.len()];
    let mut stack: Vec<usize> = Vec::new();
    for (index, item) in projection.iter().enumerate() {
        match item {
            ProjItem::ColorEdgeOpen => stack.push(index),
            ProjItem::ColorEdgeClose => {
                if let Some(open) = stack.pop() {
                    color_edges[open] = Some(index);
                    color_edges[index] = Some(open);
                }
            }
            _ => {}
        }
    }

    let mut found = false;
    let mut pending: Option<(usize, usize, ColorHead<'t>)> = None;
    let mut event_depth = 0usize;
    let mut edge_depth = 0usize;
    for index in 0..projection.len() {
        if matches!(projection[index], ProjItem::Barrier) {
            pending = None;
            continue;
        }
        if pending.is_none() {
            if event_depth == 0
                && edge_depth == 0
                && let Some((color, background, len)) = color_head_at(projection, index)
            {
                pending =
                    Some((index, index + len, ColorHead::Literal(color, background)));
            } else if event_depth == 0
                && edge_depth == 0
                && matches!(projection[index], ProjItem::ColorEdgeOpen)
            {
                pending = Some((index, index + 1, ColorHead::Container(index)));
            }
        } else if event_depth == 0 {
            let (head, content_start, head_kind) =
                pending.as_ref().expect("checked above");
            let (head, content_start) = (*head, *content_start);
            let inside_edge = matches!(head_kind, ColorHead::Container(_));
            let at_candidate_depth = if inside_edge {
                edge_depth == 1
            } else {
                edge_depth == 0
            };
            let has_content = index >= content_start
                && projection_has_content(&projection[content_start..index]);
            let text_marker =
                matches!(projection[index], ProjItem::Text(value) if value == "##");
            if at_candidate_depth && has_content && text_marker {
                let head_kind = pending.take().expect("checked above").2;
                if roles[head].is_none() && roles[index].is_none() {
                    match head_kind {
                        ColorHead::Literal(color, background) => {
                            roles[head] = Some(PairRole::ColorOpen(color, background));
                            for slot in &mut roles[head + 1..content_start] {
                                *slot = Some(PairRole::Drop);
                            }
                            roles[index] = Some(PairRole::ColorClose);
                            found = true;
                        }
                        ColorHead::Container(open) => {
                            roles[index] = Some(PairRole::ColorClose);
                            if let Some(close) = color_edges[open]
                                && roles[close].is_none()
                            {
                                roles[close] = Some(PairRole::LiteralText("##"));
                            }
                            found = true;
                        }
                    }
                }
            } else if matches!(head_kind, ColorHead::Container(_))
                && matches!(projection[index], ProjItem::ColorEdgeClose)
                && color_edges[head] == Some(index)
            {
                pending = None;
            }
        }
        match projection[index] {
            ProjItem::ColorEventOpen => event_depth += 1,
            ProjItem::ColorEventClose => event_depth = event_depth.saturating_sub(1),
            ProjItem::ColorEdgeOpen => edge_depth += 1,
            ProjItem::ColorEdgeClose => edge_depth = edge_depth.saturating_sub(1),
            _ => {}
        }
    }
    found
}

fn color_head_at<'t>(
    projection: &[ProjItem<'_>],
    start: usize,
) -> Option<(Cow<'t, str>, bool, usize)> {
    if !matches!(projection.get(start), Some(ProjItem::Text(value)) if *value == "##") {
        return None;
    }
    let mut color = String::new();
    for (offset, item) in projection[start + 1..].iter().enumerate() {
        let ProjItem::Text(value) = item else {
            return None;
        };
        if *value == "|" {
            let color = normalize_wikidot_color(&color)?.into_owned();
            return Some((Cow::Owned(color), false, offset + 2));
        }
        color.push_str(value);
    }
    None
}

fn repair_dash_runs_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    truncate_dash_run_containers(elements) | pair_dash_run_stream(elements)
}

const DASH_ATOM_CHAR: char = '\u{1}';

#[derive(Clone, Copy, PartialEq, Eq)]
enum DashUnit {
    Text,
    Atom(char),
    Partial,
    Barrier,
}

fn dash_unit(element: &Element<'_>, admit_link: bool) -> DashUnit {
    match element {
        Element::Text(_) => DashUnit::Text,
        Element::LineBreak => DashUnit::Atom('\n'),
        Element::Partial(partial) => match partial {
            PartialElement::WikidotEmptyInlineOwner
            | PartialElement::InlineSizeOpen(..)
            | PartialElement::InlineSizeClose(_)
            | PartialElement::InlineSpanOpen(..)
            | PartialElement::InlineSpanClose(_)
            | PartialElement::InlineFormatOpen(_)
            | PartialElement::InlineFormatClose(_)
            | PartialElement::InlineColorOpen(..)
            | PartialElement::InlineColorClose
            | PartialElement::InlineAnchorOpen(..)
            | PartialElement::InlineAnchorClose(_) => DashUnit::Partial,
            _ => DashUnit::Barrier,
        },
        Element::Link { .. } if admit_link => DashUnit::Atom(DASH_ATOM_CHAR),
        _ => {
            let Some(shell) = FormatShell::from_element(element) else {
                return DashUnit::Barrier;
            };
            // Triage2: mirrors the dash_flatten_shell material clause. Bold,
            // Italics, and Underline shells holding a sole nested
            // strikethrough are Barriers (flatten candidates), not
            // transparent Atoms, so `--a **--b--**` pairs across the shell.
            if shell.kind() == FormatKind::Strike
                || subtree_has_dash_pair(element)
                || (matches!(
                    shell.kind(),
                    FormatKind::Monospace
                        | FormatKind::Bold
                        | FormatKind::Italics
                        | FormatKind::Underline
                ) && subtree_has_plain_strike(element))
            {
                DashUnit::Barrier
            } else {
                DashUnit::Atom(DASH_ATOM_CHAR)
            }
        }
    }
}

fn subtree_has_dash_pair(element: &Element<'_>) -> bool {
    match element {
        Element::Text(value) => value.contains("--"),
        Element::Container(container) => {
            container.elements().iter().any(subtree_has_dash_pair)
        }
        Element::Color { elements, .. } => elements.iter().any(subtree_has_dash_pair),
        Element::Link { label, .. } => match label {
            LinkLabel::Text(value) | LinkLabel::Slug(value) => value.contains("--"),
            _ => false,
        },
        _ => false,
    }
}

fn subtree_has_plain_strike(element: &Element<'_>) -> bool {
    if is_plain_strike(element) {
        return true;
    }
    formatter_children(element)
        .is_some_and(|children| children.iter().any(subtree_has_plain_strike))
}

enum DashScanUnit {
    Text {
        element: usize,
        start: usize,
        end: usize,
    },
    Atom {
        element: usize,
        pos: usize,
    },
    Partial {
        element: usize,
        pos: usize,
    },
}

struct DashScan {
    stream: Vec<char>,
    char_offsets: Vec<usize>,
    units: Vec<DashScanUnit>,
    end: usize,
}

fn build_dash_scan<'t>(
    get: &dyn Fn(usize) -> Option<&'t Element<'t>>,
    count: usize,
    start: usize,
    admit_link: bool,
) -> DashScan {
    let mut scan = DashScan {
        stream: Vec::new(),
        char_offsets: Vec::new(),
        units: Vec::new(),
        end: start,
    };
    let mut index = start;
    while index < count {
        let Some(element) = get(index) else {
            break;
        };
        match dash_unit(element, admit_link) {
            DashUnit::Text => {
                let value = text(element).expect("text unit holds text");
                let start_pos = scan.stream.len();
                for (offset, ch) in value.char_indices() {
                    scan.stream.push(ch);
                    scan.char_offsets.push(offset);
                }
                scan.units.push(DashScanUnit::Text {
                    element: index,
                    start: start_pos,
                    end: scan.stream.len(),
                });
            }
            DashUnit::Atom(ch) => {
                scan.units.push(DashScanUnit::Atom {
                    element: index,
                    pos: scan.stream.len(),
                });
                scan.stream.push(ch);
                scan.char_offsets.push(0);
            }
            DashUnit::Partial => {
                scan.units.push(DashScanUnit::Partial {
                    element: index,
                    pos: scan.stream.len(),
                });
            }
            DashUnit::Barrier => break,
        }
        index += 1;
    }
    scan.end = index;
    scan
}

fn dash_stream_close(stream: &[char], content_start: usize) -> Option<usize> {
    let mut index = content_start + 1;
    while index + 1 < stream.len() {
        if stream[index] == '-'
            && stream[index + 1] == '-'
            && !stream[index - 1].is_whitespace()
        {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn dash_stream_opener(stream: &[char], from: usize) -> Option<usize> {
    (from..stream.len().saturating_sub(2)).find(|&pos| {
        stream[pos] == '-' && stream[pos + 1] == '-' && !stream[pos + 2].is_whitespace()
    })
}

fn dash_stream_pairs(stream: &[char]) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    let mut index = 0;
    while index + 4 < stream.len() {
        let Some(open) = dash_stream_opener(stream, index) else {
            break;
        };
        let Some(close) = dash_stream_close(stream, open + 2) else {
            break;
        };
        pairs.push((open, close));
        index = close + 2;
    }
    pairs
}

fn dash_stream_has_unmatched_opener(stream: &[char], pairs: &[(usize, usize)]) -> bool {
    let from = pairs.last().map(|&(_, close)| close + 2).unwrap_or(0);
    let Some(open) = dash_stream_opener(stream, from) else {
        return false;
    };
    dash_stream_close(stream, open + 2).is_none()
}

fn emit_dash_segments<'t>(
    input: &mut [Option<Element<'t>>],
    scan: &DashScan,
    char_segment: &dyn Fn(usize) -> Option<usize>,
    partial_segment: &dyn Fn(usize) -> usize,
    content: &[bool],
    segments: &mut [Vec<Element<'t>>],
) {
    for unit in &scan.units {
        match *unit {
            DashScanUnit::Text {
                element,
                start,
                end,
            } => {
                let value = text(input[element].as_ref().expect("unit element present"))
                    .expect("text unit holds text")
                    .to_owned();
                let mut pos = start;
                while pos < end {
                    let owner = char_segment(pos);
                    let mut next = pos + 1;
                    while next < end && char_segment(next) == owner {
                        next += 1;
                    }
                    if let Some(segment) = owner {
                        let byte_start = scan.char_offsets[pos];
                        let byte_end = if next < end {
                            scan.char_offsets[next]
                        } else {
                            value.len()
                        };
                        if pos == start && next == end && !content[segment] {
                            segments[segment].push(take_element(input, element));
                        } else {
                            push_dash_split_text(
                                &mut segments[segment],
                                &value[byte_start..byte_end],
                            );
                        }
                    }
                    pos = next;
                }
            }
            DashScanUnit::Atom { element, pos } => {
                let segment = char_segment(pos).expect("atom lands outside markers");
                segments[segment].push(take_element(input, element));
            }
            DashScanUnit::Partial { element, pos } => {
                let segment = partial_segment(pos);
                segments[segment].push(take_element(input, element));
            }
        }
    }
}

fn truncate_dash_run_containers<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let mut input = take_elements(elements);
    let mut output = Vec::with_capacity(input.len());
    let mut changed = false;
    let mut index = 0;

    while index < input.len() {
        let is_strike = matches!(
            input[index].as_ref(),
            Some(Element::Container(container))
                if container.ctype() == ContainerType::Strikethrough
        );
        if !is_strike {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        }
        let (mut replacement, truncated) =
            truncate_strike_visible(take_element(&mut input, index));
        if truncated {
            output.append(&mut replacement);
            changed = true;
            index += 1;
            continue;
        }
        input[index] = Some(
            replacement
                .pop()
                .expect("untruncated strike returns its element"),
        );
        if let Some((replacement, next)) = truncate_strike_deep(&mut input, index) {
            output.extend(replacement);
            changed = true;
            index = next;
            continue;
        }
        output.push(take_element(&mut input, index));
        index += 1;
    }

    *elements = output;
    changed
}

fn truncate_strike_visible<'t>(element: Element<'t>) -> (Vec<Element<'t>>, bool) {
    let (shell, children) =
        FormatShell::into_parts(element).expect("strike container has a shell");
    let mut input: Vec<Option<Element<'t>>> = children.into_iter().map(Some).collect();
    let scan = build_dash_scan(
        &|index| input.get(index).and_then(|element| element.as_ref()),
        input.len(),
        0,
        true,
    );
    let close = if scan.stream.first().is_some_and(|ch| !ch.is_whitespace()) {
        dash_stream_close(&scan.stream, 0)
    } else {
        None
    };
    let Some(close) = close else {
        return (
            vec![
                shell.build(
                    input
                        .into_iter()
                        .map(|element| element.expect("child present"))
                        .collect(),
                ),
            ],
            false,
        );
    };
    let mut segments = vec![Vec::new(), Vec::new()];
    emit_dash_segments(
        &mut input,
        &scan,
        &|pos| {
            if pos < close {
                Some(0)
            } else if pos < close + 2 {
                None
            } else {
                Some(1)
            }
        },
        &|pos| usize::from(pos >= close),
        &[true, false],
        &mut segments,
    );
    let mut segments = segments.into_iter();
    let content = segments.next().expect("content segment");
    let mut tail = segments.next().expect("tail segment");
    for element in input.into_iter().skip(scan.end) {
        tail.push(element.expect("child present"));
    }
    let mut replacement = vec![shell.build(content)];
    replacement.append(&mut tail);
    replacement.push(Element::Text(FormatKind::Strike.close_marker().into()));
    (replacement, true)
}

#[derive(Default)]
struct DeepStream {
    stream: Vec<char>,
    seams: Vec<usize>,
    markers: Vec<usize>,
    expanded: bool,
}

fn dash_deep_shell<'t>(element: &Element<'t>) -> Option<FormatShell<'t>> {
    if let Element::Container(container) = element
        && !container.attributes().get().is_empty()
    {
        return None;
    }
    let shell = FormatShell::from_element(element)?;
    if shell.kind() == FormatKind::Strike || subtree_has_barrier(element) {
        return None;
    }
    Some(shell)
}

fn is_plain_strike(element: &Element<'_>) -> bool {
    matches!(
        element,
        Element::Container(container)
            if container.ctype() == ContainerType::Strikethrough
                && container.attributes().get().is_empty()
    )
}

fn dash_deep_subtree(element: &Element<'_>) -> Option<DeepStream> {
    let children = formatter_children(element)?;
    let mut out = DeepStream::default();
    if is_plain_strike(element) {
        out.stream.extend(['-', '-']);
        dash_deep_seq(children, true, &mut out)?;
        out.stream.extend(['-', '-']);
    } else {
        dash_deep_seq(children, true, &mut out)?;
    }
    Some(out)
}

fn dash_deep_seq(
    elements: &[Element<'_>],
    inside: bool,
    out: &mut DeepStream,
) -> Option<usize> {
    let mut consumed = 0;
    for element in elements {
        let shell = dash_deep_shell(element);
        let strike = inside && is_plain_strike(element);
        if shell.is_some() || strike {
            let sub = dash_deep_subtree(element);
            if let Some(sub) = sub
                && (strike || sub.stream.windows(2).any(|pair| pair == ['-', '-']))
            {
                let base = out.stream.len();
                out.stream.extend(sub.stream.iter());
                out.seams.extend(sub.seams.iter().map(|pos| pos + base));
                out.markers.extend(sub.markers.iter().map(|pos| pos + base));
                if shell.is_some() {
                    if base > 0 {
                        out.seams.push(base - 1);
                    }
                    out.seams.push(base + sub.stream.len() - 1);
                    if sub.stream.first() == Some(&'-') && sub.stream.get(1) == Some(&'-')
                    {
                        out.markers.push(base);
                    }
                }
                out.expanded = true;
                consumed += 1;
                continue;
            }
        }
        match dash_unit(element, true) {
            DashUnit::Text => {
                out.stream
                    .extend(text(element).expect("text unit holds text").chars());
                consumed += 1;
            }
            DashUnit::Atom(ch) => {
                if subtree_has_dash_pair(element) {
                    if inside {
                        return None;
                    }
                    break;
                }
                out.stream.push(ch);
                consumed += 1;
            }
            DashUnit::Partial => {
                consumed += 1;
            }
            DashUnit::Barrier => {
                if inside {
                    return None;
                }
                break;
            }
        }
    }
    Some(consumed)
}

fn dash_deep_apply<'t>(
    elements: Vec<Element<'t>>,
    inside: bool,
    flat: &mut Vec<Option<Element<'t>>>,
) {
    for element in elements {
        let shell = dash_deep_shell(&element);
        let strike = inside && is_plain_strike(&element);
        if shell.is_some() || strike {
            let sub = dash_deep_subtree(&element);
            if let Some(sub) = sub
                && (strike || sub.stream.windows(2).any(|pair| pair == ['-', '-']))
            {
                if strike {
                    flat.push(Some(Element::Text(
                        FormatKind::Strike.close_marker().into(),
                    )));
                    let (_, children) = FormatShell::into_parts(element)
                        .expect("strike container has parts");
                    dash_deep_apply(children, true, flat);
                    flat.push(Some(Element::Text(
                        FormatKind::Strike.close_marker().into(),
                    )));
                } else {
                    let shell = shell.expect("shell present");
                    flat.push(Some(dash_flatten_edge(&shell, true)));
                    let (_, children) = FormatShell::into_parts(element)
                        .expect("flatten shell has parts");
                    dash_deep_apply(children, true, flat);
                    flat.push(Some(dash_flatten_edge(&shell, false)));
                }
                continue;
            }
        }
        flat.push(Some(element));
    }
}

fn dash_deep_close(
    stream: &[char],
    content_start: usize,
    seams: &[usize],
    markers: &[usize],
) -> Option<usize> {
    let mut index = content_start + 1;
    while index + 1 < stream.len() {
        if stream[index] == '-'
            && stream[index + 1] == '-'
            && (markers.contains(&index)
                || (!seams.contains(&index) && !stream[index - 1].is_whitespace()))
        {
            return Some(index);
        }
        index += 1;
    }
    None
}

fn dash_deep_opener(stream: &[char], from: usize, seams: &[usize]) -> Option<usize> {
    (from..stream.len().saturating_sub(2)).find(|&pos| {
        !seams.contains(&pos)
            && stream[pos] == '-'
            && stream[pos + 1] == '-'
            && !stream[pos + 2].is_whitespace()
    })
}

fn dash_deep_pairs(
    stream: &[char],
    seams: &[usize],
    markers: &[usize],
) -> Vec<(usize, usize)> {
    let mut first = dash_deep_close(stream, 2, seams, markers);
    if markers.contains(&2) && stream[2] == '-' && stream[3] == '-' {
        first = Some(2);
    }
    let Some(first) = first else {
        return Vec::new();
    };
    let mut pairs = vec![(0, first)];
    let mut cursor = first + 2;
    while cursor + 4 < stream.len() {
        let Some(open) = dash_deep_opener(stream, cursor, seams) else {
            break;
        };
        let Some(close) = dash_deep_close(stream, open + 2, seams, markers) else {
            break;
        };
        pairs.push((open, close));
        cursor = close + 2;
    }
    pairs
}

fn truncate_strike_deep<'t>(
    input: &mut [Option<Element<'t>>],
    strike_index: usize,
) -> Option<(Vec<Element<'t>>, usize)> {
    let element = input[strike_index].as_ref()?;
    let Element::Container(container) = element else {
        return None;
    };
    if container.ctype() != ContainerType::Strikethrough {
        return None;
    }
    let children = container.elements();
    let plain_scan =
        build_dash_scan(&|index| children.get(index), children.len(), 0, true);
    if plain_scan
        .stream
        .first()
        .is_some_and(|ch| ch.is_whitespace())
    {
        return None;
    }
    if dash_stream_close(&plain_scan.stream, 0).is_some() {
        return None;
    }
    let mut deep = DeepStream::default();
    let consumed = dash_deep_seq(children, false, &mut deep)?;
    if consumed == 0 || !deep.expanded {
        return None;
    }
    if deep.stream.first().is_some_and(|ch| ch.is_whitespace()) {
        return None;
    }
    let rest_empty = consumed == children.len();
    let mut pair_stream = vec!['-', '-'];
    pair_stream.extend(deep.stream.iter());
    let mut continuation_end = strike_index + 1;
    if rest_empty {
        pair_stream.extend(['-', '-']);
        let continuation_scan = build_dash_scan(
            &|index| {
                input
                    .get(strike_index + 1 + index)
                    .and_then(|element| element.as_ref())
            },
            input.len() - strike_index - 1,
            0,
            true,
        );
        pair_stream.extend(continuation_scan.stream.iter());
        continuation_end = strike_index + 1 + continuation_scan.end;
    }
    let seams: Vec<usize> = deep.seams.iter().map(|pos| pos + 2).collect();
    let markers: Vec<usize> = deep.markers.iter().map(|pos| pos + 2).collect();
    let pairs = dash_deep_pairs(&pair_stream, &seams, &markers);
    if !matches!(pairs.first(), Some(&(0, _))) {
        return None;
    }
    let mut flat: Vec<Option<Element<'t>>> = Vec::new();
    flat.push(Some(Element::Text(
        FormatKind::Strike.close_marker().into(),
    )));
    let strike = take_element(input, strike_index);
    let (_, strike_children) =
        FormatShell::into_parts(strike).expect("strike container has parts");
    let mut child_iter = strike_children.into_iter();
    let head: Vec<Element<'t>> = child_iter.by_ref().take(consumed).collect();
    let rest_children: Vec<Element<'t>> = child_iter.collect();
    dash_deep_apply(head, false, &mut flat);
    if rest_empty {
        flat.push(Some(Element::Text(
            FormatKind::Strike.close_marker().into(),
        )));
        for element_index in strike_index + 1..continuation_end {
            flat.push(Some(take_element(input, element_index)));
        }
    } else {
        for child in rest_children {
            flat.push(Some(child));
        }
        flat.push(Some(Element::Text(
            FormatKind::Strike.close_marker().into(),
        )));
    }
    let flat_scan = build_dash_scan(
        &|index| flat.get(index).and_then(|element| element.as_ref()),
        flat.len(),
        0,
        true,
    );
    debug_assert_eq!(flat_scan.stream, pair_stream);
    let mut output = Vec::new();
    emit_dash_pairs_flat(&mut flat, &flat_scan, &pairs, &mut output);
    if let Some(close_index) = output.iter().position(|element| {
        matches!(
            element,
            Element::Partial(PartialElement::InlineFormatClose(
                InlineFormatKind::Strikethrough,
            ))
        )
    }) {
        for element in &mut output[..close_index] {
            if is_exact_text(element, "--") {
                *element = Element::Text(Cow::Borrowed("\u{2014}"));
            }
        }
    }
    for element in flat.into_iter().skip(flat_scan.end) {
        output.push(element.expect("unscanned element present"));
    }
    let next = if rest_empty {
        continuation_end
    } else {
        strike_index + 1
    };
    Some((output, next))
}

fn emit_dash_pairs<'t>(
    input: &mut [Option<Element<'t>>],
    scan: &DashScan,
    pairs: &[(usize, usize)],
    output: &mut Vec<Element<'t>>,
) {
    let mut segments: Vec<Vec<Element<'t>>> =
        (0..pairs.len() * 2 + 1).map(|_| Vec::new()).collect();
    let content: Vec<bool> = (0..pairs.len() * 2 + 1)
        .map(|segment| segment % 2 == 1)
        .collect();
    let mut owners: Vec<Option<usize>> = vec![None; scan.stream.len()];
    let mut cursor = 0;
    for (pair, &(open, close)) in pairs.iter().enumerate() {
        for owner in &mut owners[cursor..open] {
            *owner = Some(pair * 2);
        }
        for owner in &mut owners[open + 2..close] {
            *owner = Some(pair * 2 + 1);
        }
        cursor = close + 2;
    }
    for owner in &mut owners[cursor..] {
        *owner = Some(pairs.len() * 2);
    }
    emit_dash_segments(
        input,
        scan,
        &|pos| owners[pos],
        &|pos| {
            for (pair, &(open, close)) in pairs.iter().enumerate() {
                if pos <= open {
                    return pair * 2;
                }
                if pos < close {
                    return pair * 2 + 1;
                }
            }
            pairs.len() * 2
        },
        &content,
        &mut segments,
    );
    for (segment_index, segment) in segments.into_iter().enumerate() {
        if segment_index % 2 == 1 {
            if !segment.is_empty() {
                output.push(strike_shell().build(segment));
            }
        } else {
            output.extend(segment);
        }
    }
}

fn pair_dash_run_stream<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let mut input = take_elements(elements);
    let mut output = Vec::with_capacity(input.len());
    let mut changed = false;
    let mut index = 0;

    while index < input.len() {
        let scan = build_dash_scan(
            &|element_index| {
                input
                    .get(element_index)
                    .and_then(|element| element.as_ref())
            },
            input.len(),
            index,
            false,
        );
        if scan.end < input.len()
            && let Some(next) =
                expand_dash_format_container(&mut input, index, &scan, &mut output)
        {
            changed = true;
            index = next;
            continue;
        }
        if scan.end == index {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        }
        if !scan
            .stream
            .windows(3)
            .any(|window| window == ['-', '-', '-'])
        {
            for element_index in index..scan.end {
                output.push(take_element(&mut input, element_index));
            }
            index = scan.end;
            continue;
        }
        let pairs = dash_stream_pairs(&scan.stream);
        if dash_stream_has_unmatched_opener(&scan.stream, &pairs) {
            if let Some(next) =
                expand_dash_strike_container(&mut input, index, &scan, &mut output)
            {
                changed = true;
                index = next;
                continue;
            }
        }
        if pairs.is_empty() {
            for element_index in index..scan.end {
                output.push(take_element(&mut input, element_index));
            }
            index = scan.end;
            continue;
        }
        changed = true;
        emit_dash_pairs(&mut input, &scan, &pairs, &mut output);
        index = scan.end;
    }

    *elements = output;
    changed
}

fn expand_dash_strike_container<'t>(
    input: &mut Vec<Option<Element<'t>>>,
    start: usize,
    scan: &DashScan,
    output: &mut Vec<Element<'t>>,
) -> Option<usize> {
    let container_index = scan.end;
    let container = input.get(container_index).and_then(Option::as_ref)?;
    let Element::Container(container_ref) = container else {
        return None;
    };
    if container_ref.ctype() != ContainerType::Strikethrough {
        return None;
    }
    let children = container_ref.elements();
    let child_scan =
        build_dash_scan(&|index| children.get(index), children.len(), 0, false);
    if child_scan.end != children.len() {
        return None;
    }
    let continuation = build_dash_scan(
        &|element_index| {
            input
                .get(element_index)
                .and_then(|element| element.as_ref())
        },
        input.len(),
        container_index + 1,
        false,
    );
    let mut stream = scan.stream.clone();
    stream.extend(['-', '-']);
    stream.extend(child_scan.stream.iter());
    stream.extend(['-', '-']);
    stream.extend(continuation.stream.iter());
    let pairs = dash_stream_pairs(&stream);
    let boundary = scan.stream.len();
    if !pairs
        .iter()
        .any(|&(open, close)| open < boundary && close >= boundary)
    {
        return None;
    }
    let mut flat: Vec<Option<Element<'t>>> = Vec::new();
    for element_index in start..scan.end {
        flat.push(Some(take_element(input, element_index)));
    }
    flat.push(Some(Element::Text(
        FormatKind::Strike.close_marker().into(),
    )));
    let container_element = take_element(input, container_index);
    let (_, children) =
        FormatShell::into_parts(container_element).expect("strike container has parts");
    for child in children {
        flat.push(Some(child));
    }
    flat.push(Some(Element::Text(
        FormatKind::Strike.close_marker().into(),
    )));
    for element_index in container_index + 1..continuation.end {
        flat.push(Some(take_element(input, element_index)));
    }
    let flat_scan = build_dash_scan(
        &|element_index| flat.get(element_index).and_then(|element| element.as_ref()),
        flat.len(),
        0,
        false,
    );
    emit_dash_pairs(&mut flat, &flat_scan, &pairs, output);
    Some(continuation.end)
}

fn dash_flatten_shell<'t>(element: &Element<'t>) -> Option<FormatShell<'t>> {
    if let Element::Container(container) = element
        && !container.attributes().get().is_empty()
    {
        return None;
    }
    let shell = FormatShell::from_element(element)?;
    if shell.kind() == FormatKind::Strike {
        return None;
    }
    // Triage2 (live D2, r6-r11): Bold, Italics, and Underline shells behave
    // like Monospace when their body is a sole nested strikethrough.
    // `--a **--b--**` pairs the outer dashes across the shell (the shell
    // then splits, preserving the inner dash as em-dash); `--a __--b--__`
    // extends the outer pair to the underline close (lowering already
    // extends strike-close inside Underline, and splits inside Bold/Italics,
    // per the crossed-underline-close-inside and strike-close-inside-bold
    // pins).
    let has_dash_material = subtree_has_dash_pair(element)
        || (matches!(
            shell.kind(),
            FormatKind::Monospace
                | FormatKind::Bold
                | FormatKind::Italics
                | FormatKind::Underline
        ) && subtree_has_plain_strike(element));
    if !has_dash_material || subtree_has_barrier(element) {
        return None;
    }
    Some(shell)
}

fn dash_flatten_edge<'t>(shell: &FormatShell<'t>, open: bool) -> Element<'t> {
    match shell {
        FormatShell::Container { kind, .. } => {
            let kind = match kind {
                FormatKind::Bold => InlineFormatKind::Bold,
                FormatKind::Italics => InlineFormatKind::Italics,
                FormatKind::Underline => InlineFormatKind::Underline,
                FormatKind::Strike => InlineFormatKind::Strikethrough,
                FormatKind::Subscript => InlineFormatKind::Subscript,
                FormatKind::Superscript => InlineFormatKind::Superscript,
                FormatKind::Monospace => InlineFormatKind::Monospace,
                FormatKind::Color => {
                    unreachable!("container shells never hold the color kind")
                }
            };
            Element::Partial(if open {
                PartialElement::InlineFormatOpen(kind)
            } else {
                PartialElement::InlineFormatClose(kind)
            })
        }
        FormatShell::Color { color, background } => {
            if open {
                Element::Partial(PartialElement::InlineColorOpen(
                    color.clone(),
                    *background,
                ))
            } else {
                Element::Partial(PartialElement::InlineColorClose)
            }
        }
    }
}

fn dash_flatten_scan(children: &[Element<'_>]) -> Option<Vec<char>> {
    let mut stream = Vec::new();
    for child in children {
        if is_plain_strike(child) {
            let inner = formatter_children(child)?;
            stream.extend(['-', '-']);
            stream.extend(dash_flatten_scan(inner)?);
            stream.extend(['-', '-']);
            continue;
        }
        if dash_flatten_shell(child).is_some() {
            let inner = formatter_children(child)?;
            stream.extend(dash_flatten_scan(inner)?);
            continue;
        }
        match dash_unit(child, false) {
            DashUnit::Text => {
                stream.extend(text(child).expect("text unit holds text").chars());
            }
            DashUnit::Atom(ch) => stream.push(ch),
            DashUnit::Partial => {}
            DashUnit::Barrier => return None,
        }
    }
    Some(stream)
}

fn dash_flatten_apply<'t>(
    children: Vec<Element<'t>>,
    flat: &mut Vec<Option<Element<'t>>>,
) {
    for child in children {
        if is_plain_strike(&child) {
            flat.push(Some(Element::Text(
                FormatKind::Strike.close_marker().into(),
            )));
            let (_, grandchildren) =
                FormatShell::into_parts(child).expect("strike container has parts");
            dash_flatten_apply(grandchildren, flat);
            flat.push(Some(Element::Text(
                FormatKind::Strike.close_marker().into(),
            )));
            continue;
        }
        if let Some(shell) = dash_flatten_shell(&child) {
            flat.push(Some(dash_flatten_edge(&shell, true)));
            let (_, grandchildren) =
                FormatShell::into_parts(child).expect("flatten shell has parts");
            dash_flatten_apply(grandchildren, flat);
            flat.push(Some(dash_flatten_edge(&shell, false)));
            continue;
        }
        flat.push(Some(child));
    }
}

fn expand_dash_format_container<'t>(
    input: &mut Vec<Option<Element<'t>>>,
    start: usize,
    scan: &DashScan,
    output: &mut Vec<Element<'t>>,
) -> Option<usize> {
    let container_index = scan.end;
    let container = input.get(container_index).and_then(Option::as_ref)?;
    let shell = dash_flatten_shell(container)?;
    let children: &[Element<'t>] = match container {
        Element::Container(container) => container.elements(),
        Element::Color { elements, .. } => elements,
        _ => return None,
    };
    let child_stream = dash_flatten_scan(children)?;
    let continuation = build_dash_scan(
        &|element_index| {
            input
                .get(element_index)
                .and_then(|element| element.as_ref())
        },
        input.len(),
        container_index + 1,
        false,
    );
    let mut stream = scan.stream.clone();
    stream.extend(child_stream.iter());
    stream.extend(continuation.stream.iter());
    let open_edge = scan.stream.len();
    let close_edge = open_edge + child_stream.len();
    let mut pair_stream = stream.clone();
    if open_edge > 0 && pair_stream[open_edge - 1].is_whitespace() {
        pair_stream[open_edge - 1] = '\u{1}';
    }
    let pairs = dash_stream_pairs(&pair_stream);
    if !pairs.iter().any(|&(open, close)| {
        (open < open_edge && close >= open_edge)
            || (open < close_edge && close >= close_edge)
    }) {
        return None;
    }
    let open_partial = dash_flatten_edge(&shell, true);
    let close_partial = dash_flatten_edge(&shell, false);
    let mut flat: Vec<Option<Element<'t>>> = Vec::new();
    for element_index in start..scan.end {
        flat.push(Some(take_element(input, element_index)));
    }
    flat.push(Some(open_partial));
    let container_element = take_element(input, container_index);
    let (_, children) =
        FormatShell::into_parts(container_element).expect("flatten shell has parts");
    dash_flatten_apply(children, &mut flat);
    flat.push(Some(close_partial));
    for element_index in container_index + 1..continuation.end {
        flat.push(Some(take_element(input, element_index)));
    }
    let flat_scan = build_dash_scan(
        &|element_index| flat.get(element_index).and_then(|element| element.as_ref()),
        flat.len(),
        0,
        false,
    );
    debug_assert_eq!(flat_scan.stream, stream);
    emit_dash_pairs_flat(&mut flat, &flat_scan, &pairs, output);
    Some(continuation.end)
}

fn strike_event<'t>(open: bool) -> Element<'t> {
    Element::Partial(if open {
        PartialElement::InlineFormatOpen(InlineFormatKind::Strikethrough)
    } else {
        PartialElement::InlineFormatClose(InlineFormatKind::Strikethrough)
    })
}

fn push_dash_text_piece<'t>(
    output: &mut Vec<Element<'t>>,
    scan: &DashScan,
    value: &str,
    run_start: usize,
    run_end: usize,
    unit_end: usize,
) {
    let byte_start = scan.char_offsets[run_start];
    let byte_end = if run_end < unit_end {
        scan.char_offsets[run_end]
    } else {
        value.len()
    };
    push_dash_split_text(output, &value[byte_start..byte_end]);
}

fn emit_dash_pairs_flat<'t>(
    input: &mut [Option<Element<'t>>],
    scan: &DashScan,
    pairs: &[(usize, usize)],
    output: &mut Vec<Element<'t>>,
) {
    let mut dropped = vec![false; scan.stream.len()];
    let mut events: Vec<(usize, bool)> = Vec::with_capacity(pairs.len() * 2);
    for &(open, close) in pairs {
        dropped[open] = true;
        dropped[open + 1] = true;
        dropped[close] = true;
        dropped[close + 1] = true;
        events.push((open, true));
        events.push((close, false));
    }
    let mut next_event = 0;
    for unit in &scan.units {
        match *unit {
            DashScanUnit::Text {
                element,
                start,
                end,
            } => {
                while next_event < events.len() && events[next_event].0 <= start {
                    output.push(strike_event(events[next_event].1));
                    next_event += 1;
                }
                if !(start..end).any(|pos| dropped[pos]) {
                    output.push(take_element(input, element));
                    continue;
                }
                let value = text(input[element].as_ref().expect("unit element present"))
                    .expect("text unit holds text")
                    .to_owned();
                let mut run_start: Option<usize> = None;
                for (offset, &is_dropped) in dropped[start..end].iter().enumerate() {
                    let pos = start + offset;
                    if is_dropped {
                        if let Some(run) = run_start.take() {
                            push_dash_text_piece(output, scan, &value, run, pos, end);
                        }
                        if next_event < events.len() && events[next_event].0 == pos {
                            output.push(strike_event(events[next_event].1));
                            next_event += 1;
                        }
                        continue;
                    }
                    if run_start.is_none() {
                        run_start = Some(pos);
                    }
                }
                if let Some(run) = run_start {
                    push_dash_text_piece(output, scan, &value, run, end, end);
                }
            }
            DashScanUnit::Atom { element, .. } => {
                output.push(take_element(input, element));
            }
            DashScanUnit::Partial { element, pos } => {
                output.push(take_element(input, element));
                while next_event < events.len() && events[next_event].0 == pos {
                    output.push(strike_event(events[next_event].1));
                    next_event += 1;
                }
            }
        }
    }
    while next_event < events.len() {
        output.push(strike_event(events[next_event].1));
        next_event += 1;
    }
}

fn outer_candidates<'t>(elements: &[Element<'t>]) -> Vec<Option<OuterCandidate<'t>>> {
    (0..elements.len())
        .map(|index| outer_candidate(elements, index))
        .collect()
}

fn next_candidate_indices(
    candidates: &[Option<OuterCandidate<'_>>],
) -> Vec<Option<usize>> {
    let mut result = vec![None; candidates.len()];
    let mut next = None;
    for index in (0..candidates.len()).rev() {
        result[index] = next;
        if candidates[index].is_some() {
            next = Some(index);
        }
    }
    result
}

fn next_any_formatter_indices(elements: &[Element<'_>]) -> Vec<Option<usize>> {
    let mut result = vec![None; elements.len()];
    let mut next = None;
    for index in (0..elements.len()).rev() {
        result[index] = next;
        if FormatShell::from_element(&elements[index]).is_some() {
            next = Some(index);
        }
    }
    result
}

fn next_formatter_indices(
    elements: &[Element<'_>],
) -> Vec<[Option<usize>; FORMAT_KIND_COUNT]> {
    let mut result = vec![[None; FORMAT_KIND_COUNT]; elements.len()];
    let mut next = [None; FORMAT_KIND_COUNT];
    for index in (0..elements.len()).rev() {
        result[index] = next;
        if let Some(shell) = FormatShell::from_element(&elements[index]) {
            next[shell.kind().index()] = Some(index);
        }
    }
    result
}

fn next_crossed_indices(
    elements: &[Element<'_>],
) -> Vec<[Option<CrossTarget>; FORMAT_KIND_COUNT]> {
    let mut result = vec![[None; FORMAT_KIND_COUNT]; elements.len()];
    let mut next = [None; FORMAT_KIND_COUNT];
    for index in (0..elements.len()).rev() {
        result[index] = next;
        let Some(inner_shell) = FormatShell::from_element(&elements[index]) else {
            continue;
        };
        let Some(children) = formatter_children(&elements[index]) else {
            continue;
        };
        for outer_kind in FormatKind::ALL {
            if outer_kind == inner_shell.kind() {
                continue;
            }
            let close_index = if outer_kind == FormatKind::Strike {
                find_valid_dash_close(children)
            } else {
                find_text_marker(children, outer_kind.close_marker())
            };
            if let Some(marker_index) = close_index {
                next[outer_kind.index()] = Some(CrossTarget {
                    inner_index: index,
                    marker_index,
                });
            }
        }
    }
    result
}

fn next_valid_dash_closes(elements: &[Element<'_>]) -> Vec<Option<usize>> {
    let mut result = vec![None; elements.len()];
    let mut next = None;
    for index in (0..elements.len()).rev() {
        result[index] = next;
        if matches!(elements[index], Element::FootnoteBlock { .. }) {
            next = None;
            continue;
        }
        let valid_close = dash_run_len(&elements[index])
            .is_some_and(|length| length >= 2)
            && !dash_marker_occupies_its_line(elements, index)
            && index
                .checked_sub(1)
                .and_then(|previous| elements.get(previous))
                .is_none_or(|element| !element_ends_with_ascii_space(element));
        if valid_close {
            next = Some(index);
        }
    }
    result
}

fn take_elements<'t>(elements: &mut Vec<Element<'t>>) -> Vec<Option<Element<'t>>> {
    mem::take(elements).into_iter().map(Some).collect()
}

fn take_element<'t>(elements: &mut [Option<Element<'t>>], index: usize) -> Element<'t> {
    elements[index]
        .take()
        .expect("normalization ranges do not overlap")
}

fn take_range<'t>(
    elements: &mut [Option<Element<'t>>],
    range: Range<usize>,
) -> Vec<Element<'t>> {
    range.filter_map(|index| elements[index].take()).collect()
}

fn discard_range(elements: &mut [Option<Element<'_>>], range: Range<usize>) {
    for index in range {
        elements[index].take();
    }
}

pub(crate) fn lower_wikidot_root_dash_runs(elements: &mut Vec<Element<'_>>) {
    if !elements
        .iter()
        .any(|element| dash_run_len(element).is_some())
    {
        return;
    }
    let mut output = Vec::with_capacity(elements.len());
    for element in elements.drain(..) {
        match dash_run_len(&element) {
            Some(run_len) => output.extend(wikidot_dash_run_elements(run_len)),
            None => output.push(element),
        }
    }
    *elements = output;
}

fn lower_dash_runs(elements: &mut Vec<Element<'_>>) {
    let mut output = Vec::with_capacity(elements.len());
    for element in elements.drain(..) {
        if let Some(run_len) = dash_run_len(&element) {
            output.extend(wikidot_dash_run_elements(run_len));
            continue;
        }
        let Element::Text(value) = &element else {
            output.push(element);
            continue;
        };
        if !is_shed_text(value) || !value.contains("--") {
            output.push(element);
            continue;
        }
        for piece in dash_split_pieces(value) {
            if piece.len() >= 2 && piece.bytes().all(|byte| byte == b'-') {
                output.extend(wikidot_dash_run_elements(piece.len()));
            } else {
                output.push(Element::Text(Cow::Owned(piece)));
            }
        }
    }
    *elements = output;
}

pub(crate) fn wikidot_dash_run_elements<'t>(run_len: usize) -> Vec<Element<'t>> {
    debug_assert!(run_len >= 2);
    if run_len == 2 {
        return vec![Element::Text(Cow::Borrowed("\u{2014}"))];
    }

    let mut elements = Vec::with_capacity(run_len / 2);
    for _ in 0..run_len / 5 {
        elements.push(strike_shell().build(vec![Element::Text(Cow::Borrowed("-"))]));
    }
    let remainder = run_len % 5;
    for _ in 0..remainder / 2 {
        elements.push(Element::Text(Cow::Borrowed("\u{2014}")));
    }
    if remainder % 2 == 1 {
        elements.push(Element::Text(Cow::Borrowed("-")));
    }
    elements
}

fn dash_marker_occupies_its_line(elements: &[Element<'_>], index: usize) -> bool {
    elements.get(index).and_then(text) == Some("--")
        && line_edge_beside_marker(elements, index, true)
        && line_edge_beside_marker(elements, index, false)
}

fn dash_marker_occupies_its_line_at(
    elements: &[Option<Element<'_>>],
    index: usize,
) -> bool {
    elements.get(index).and_then(Option::as_ref).and_then(text) == Some("--")
        && option_line_edge(elements, index, true)
        && option_line_edge(elements, index, false)
}

fn option_line_edge(
    elements: &[Option<Element<'_>>],
    index: usize,
    before: bool,
) -> bool {
    let mut cursor = index;
    loop {
        if before {
            if cursor == 0 {
                return true;
            }
            cursor -= 1;
        } else {
            cursor += 1;
            if cursor >= elements.len() {
                return true;
            }
        }
        let Some(element) = elements[cursor].as_ref() else {
            continue;
        };
        match element {
            Element::LineBreak | Element::LineBreaks(_) => return true,
            Element::Text(value) if value.chars().all(char::is_whitespace) => continue,
            _ => return false,
        }
    }
}

fn line_edge_beside_marker(elements: &[Element<'_>], index: usize, before: bool) -> bool {
    let mut cursor = index;
    loop {
        if before {
            if cursor == 0 {
                return true;
            }
            cursor -= 1;
        } else {
            cursor += 1;
            if cursor >= elements.len() {
                return true;
            }
        }
        match &elements[cursor] {
            Element::LineBreak | Element::LineBreaks(_) => return true,
            Element::Text(value) if value.chars().all(char::is_whitespace) => continue,
            _ => return false,
        }
    }
}

fn outer_candidate<'t>(
    elements: &[Element<'t>],
    start: usize,
) -> Option<OuterCandidate<'t>> {
    let marker = text(elements.get(start)?)?;
    if marker == "--" && dash_marker_occupies_its_line(elements, start) {
        return None;
    }
    let simple = match marker {
        "**" => Some((FormatKind::Bold, ContainerType::Bold, "**")),
        "//" => Some((FormatKind::Italics, ContainerType::Italics, "//")),
        "__" => Some((FormatKind::Underline, ContainerType::Underline, "__")),
        "--" => Some((FormatKind::Strike, ContainerType::Strikethrough, "--")),
        ",," => Some((FormatKind::Subscript, ContainerType::Subscript, ",,")),
        "^^" => Some((FormatKind::Superscript, ContainerType::Superscript, "^^")),
        "{{" => Some((FormatKind::Monospace, ContainerType::Monospace, "{{")),
        _ => None,
    };
    if let Some((kind, ctype, literal_marker)) = simple {
        if elements
            .get(start + 1)
            .is_some_and(element_starts_with_ascii_space)
        {
            return None;
        }
        return Some(OuterCandidate {
            shell: FormatShell::Container {
                kind,
                ctype,
                attributes: AttributeMap::new(),
            },
            opener_len: 1,
            literal_marker,
        });
    }

    if marker != "##" {
        return None;
    }
    let mut color = String::new();
    for (offset, element) in elements[start + 1..].iter().enumerate() {
        let value = text(element)?;
        if value == "|" {
            let color = normalize_wikidot_color(&color)?.into_owned().into();
            return Some(OuterCandidate {
                shell: FormatShell::Color {
                    color,
                    background: false,
                },
                opener_len: offset + 2,
                literal_marker: "##",
            });
        }
        color.push_str(value);
    }
    None
}

fn formatter_children<'a, 't>(element: &'a Element<'t>) -> Option<&'a [Element<'t>]> {
    match element {
        Element::Container(container)
            if kind_from_container(container.ctype()).is_some() =>
        {
            Some(container.elements())
        }
        Element::Color { elements, .. } => Some(elements),
        _ => None,
    }
}

fn kind_from_container(ctype: ContainerType) -> Option<FormatKind> {
    match ctype {
        ContainerType::Bold => Some(FormatKind::Bold),
        ContainerType::Italics => Some(FormatKind::Italics),
        ContainerType::Underline => Some(FormatKind::Underline),
        ContainerType::Strikethrough => Some(FormatKind::Strike),
        ContainerType::Subscript => Some(FormatKind::Subscript),
        ContainerType::Superscript => Some(FormatKind::Superscript),
        ContainerType::Monospace => Some(FormatKind::Monospace),
        _ => None,
    }
}

fn monospace_shell<'t>() -> FormatShell<'t> {
    FormatShell::Container {
        kind: FormatKind::Monospace,
        ctype: ContainerType::Monospace,
        attributes: AttributeMap::new(),
    }
}

fn strike_shell<'t>() -> FormatShell<'t> {
    FormatShell::Container {
        kind: FormatKind::Strike,
        ctype: ContainerType::Strikethrough,
        attributes: AttributeMap::new(),
    }
}

fn find_valid_dash_close(children: &[Element<'_>]) -> Option<usize> {
    children.iter().enumerate().find_map(|(index, element)| {
        if !is_exact_text(element, "--") {
            return None;
        }
        let padded = index > 0 && element_ends_with_pad(&children[index - 1]);
        (!padded).then_some(index)
    })
}

fn element_ends_with_pad(element: &Element<'_>) -> bool {
    matches!(element, Element::Text(value) if value.ends_with(' ') || value.ends_with('\t'))
}

fn find_text_marker(elements: &[Element<'_>], marker: &str) -> Option<usize> {
    elements
        .iter()
        .position(|element| is_exact_text(element, marker))
}

fn take_leading_ascii_space(elements: &mut Vec<Element<'_>>) -> bool {
    let Some(Element::Text(value)) = elements.first_mut() else {
        return false;
    };
    let Some(rest) = value.strip_prefix(' ') else {
        return false;
    };
    if rest.is_empty() {
        elements.remove(0);
    } else {
        *value = Cow::Owned(rest.to_owned());
    }
    true
}

fn dash_run_len(element: &Element<'_>) -> Option<usize> {
    let value = text(element)?;
    (value.len() >= 2 && value.bytes().all(|byte| byte == b'-')).then_some(value.len())
}

fn text<'a, 't>(element: &'a Element<'t>) -> Option<&'a str> {
    match element {
        Element::Text(value) => Some(value),
        _ => None,
    }
}

fn is_exact_text(element: &Element<'_>, expected: &str) -> bool {
    text(element) == Some(expected)
}

fn element_starts_with_ascii_space(element: &Element<'_>) -> bool {
    text(element).is_some_and(|value| value.starts_with(' '))
}

fn element_ends_with_ascii_space(element: &Element<'_>) -> bool {
    text(element).is_some_and(|value| value.ends_with(' '))
}

fn visit_children_mut<'t>(element: &mut Element<'t>, visit: fn(&mut Vec<Element<'t>>)) {
    match element {
        Element::Container(container) => visit(container.elements_mut()),
        Element::Table(table) => {
            for row in &mut table.rows {
                for cell in &mut row.cells {
                    visit(&mut cell.elements);
                }
            }
        }
        Element::TabView(tabs) => {
            for item in tabs {
                visit(item.elements_mut());
            }
        }
        Element::Anchor { elements, .. }
        | Element::Collapsible { elements, .. }
        | Element::Color { elements, .. }
        | Element::Include { elements, .. } => visit(elements),
        Element::WikidotBlockFrame { children, .. } => visit(children),
        Element::List { items, .. } => {
            for item in items {
                match item {
                    ListItem::Elements { elements, .. } => visit(elements),
                    ListItem::SubList { element } => visit_children_mut(element, visit),
                }
            }
        }
        Element::DefinitionList(items) => {
            for item in items {
                visit(&mut item.key_elements);
                visit(&mut item.value_elements);
            }
        }
        Element::Partial(partial) => match partial {
            PartialElement::ListItem(ListItem::Elements { elements, .. }) => {
                visit(elements)
            }
            PartialElement::ListItem(ListItem::SubList { element }) => {
                visit_children_mut(element, visit)
            }
            PartialElement::TableRow(row) => {
                for cell in &mut row.cells {
                    visit(&mut cell.elements);
                }
            }
            PartialElement::TableCell(cell) => visit(&mut cell.elements),
            PartialElement::WikidotFoster(elements) => visit(elements),
            PartialElement::WikidotCollapsibleTail(elements) => visit(elements),
            PartialElement::WikidotCollapsibleHeadingEnd => {}
            PartialElement::Tab(tab) => visit(&mut tab.elements),
            PartialElement::RubyText(ruby_text) => visit(&mut ruby_text.elements),
            PartialElement::WikidotEmptyInlineOwner
            | PartialElement::WikidotBlockOpen(_)
            | PartialElement::WikidotBlockClose(_)
            | PartialElement::InlineSizeOpen(..)
            | PartialElement::InlineSizeClose(_)
            | PartialElement::InlineSpanOpen(..)
            | PartialElement::InlineSpanClose(_)
            | PartialElement::InlineFormatOpen(_)
            | PartialElement::InlineFormatClose(_)
            | PartialElement::InlineColorOpen(..)
            | PartialElement::InlineColorClose
            | PartialElement::InlineAnchorOpen(..)
            | PartialElement::InlineAnchorClose(_) => {}
        },
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{Element, InlineFormatKind, PartialElement};
    use std::borrow::Cow;

    fn text(value: &'static str) -> Element<'static> {
        Element::Text(Cow::Borrowed(value))
    }

    #[test]
    fn inline_repair_screen_flags_each_pass_material() {
        for marker in ["**", "//", "__", "{{", "}}", "##", "--", "^^", ",,"] {
            assert!(
                sequence_has_inline_repair_material(&[text(marker)]),
                "unpadded {marker:?} opens a repair pass",
            );
        }
        assert!(sequence_has_inline_repair_material(&[
            text("plain"),
            text("}}"),
            text("tail"),
        ]));
        assert!(sequence_has_inline_repair_material(&[text("a--b")]));
        assert!(sequence_has_inline_repair_material(&[
            text("a-"),
            text("-b")
        ]));
        assert!(sequence_has_inline_repair_material(&[text("---")]));
        assert!(sequence_has_inline_repair_material(&[Element::LineBreak]));
        assert!(sequence_has_inline_repair_material(&[Element::Partial(
            PartialElement::InlineFormatOpen(InlineFormatKind::Bold),
        )]));
    }

    #[test]
    fn inline_repair_screen_ignores_padded_markers() {
        for marker in ["--", "^^", ",,"] {
            assert!(
                !sequence_has_inline_repair_material(&[
                    text(marker),
                    text(" "),
                    text("x")
                ]),
                "padded {marker:?} is not repair material",
            );
        }
    }

    #[test]
    fn flat_comment_probe_text_skips_repair_passes_without_changes() {
        let mut elements: Vec<Element<'_>> = ["[", "!", "--", " ", "a", " "]
            .into_iter()
            .cycle()
            .take(60)
            .map(text)
            .collect();
        assert!(!sequence_has_inline_repair_material(&elements));
        let original = elements.clone();
        assert!(!normalize_sequence_passes(&mut elements, false));
        assert_eq!(elements, original);
    }
}
