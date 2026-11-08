use super::block::blocks::{
    BLOCK_ALIGN_CENTER, BLOCK_ALIGN_JUSTIFY, BLOCK_ALIGN_LEFT, BLOCK_ALIGN_RIGHT,
    BLOCK_COLLAPSIBLE, BLOCK_DIV, BLOCK_NOTE, CollapsibleHead, parse_collapsible_head,
    wikidot_div_head_attributes,
};
use super::block::{BlockBodyStart, BlockRule};
use super::prelude::*;
use super::{RULE_LIST, RULE_PAGE};
use crate::parsing::collect::{collect_consume_keep_split, consume_valid_comment};
use crate::parsing::paragraph::gather_paragraphs;
use crate::tree::{
    Alignment, AttributeMap, Container, ContainerType, ListItem, ListType, PartialElement,
};
use std::borrow::Cow;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum LineOwner {
    Quote { depth: usize },
    List { depth: usize, ltype: ListType },
    DefinitionList,
}

#[derive(Debug)]
pub(crate) struct LostOwnerBlock<'t> {
    pub control: Option<Element<'t>>,
    pub body: Vec<Element<'t>>,
    pub errors: Vec<ParseError>,
    pub append_unquoted_close_break: bool,
    pub hoisted_list: bool,
    pub sibling_hoisted: bool,
}

enum BlockHead<'t> {
    Div(AttributeMap<'t>),
    Align(Alignment),
    Collapsible(CollapsibleHead<'t>),
    Note(AttributeMap<'t>),
}

pub(crate) fn try_consume_lost_owner_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
    owner: LineOwner,
) -> Result<Option<LostOwnerBlock<'t>>, ParseError>
where
    'r: 't,
{
    if parser.current().token != Token::LeftBlock {
        return Ok(None);
    }

    let state = parser.get_mutable_state();
    let mut candidate = parser.clone();
    let result = parse_lost_owner_block(&mut candidate, owner);
    match result {
        Ok(Some(block)) => {
            parser.update(&candidate);
            Ok(Some(block))
        }
        Ok(None) | Err(_) => {
            parser.reset_mutable_state(state);
            Ok(None)
        }
    }
}

pub(crate) fn try_consume_definition_list_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Option<Vec<Element<'t>>>, ParseError>
where
    'r: 't,
{
    if !parser.settings().layout.legacy()
        || !parser.start_of_line()
        || parser.current().token != Token::Colon
    {
        return Ok(None);
    }

    let state = parser.get_mutable_state();
    let mut candidate = parser.clone();
    let parsed: Result<Option<Vec<Element<'t>>>, ParseError> = (|| {
        candidate.step()?;
        if candidate.current().token != Token::Whitespace {
            return Ok(None);
        }
        candidate.step()?;
        let Some(lost) =
            parse_lost_owner_block(&mut candidate, LineOwner::DefinitionList)?
        else {
            return Ok(None);
        };
        Ok(Some(lost.body))
    })();

    match parsed {
        Ok(Some(body)) => {
            parser.update(&candidate);
            Ok(Some(body))
        }
        Ok(None) | Err(_) => {
            parser.reset_mutable_state(state);
            Ok(None)
        }
    }
}

pub(crate) fn try_consume_literal_list_alignment<'r, 't>(
    parser: &mut Parser<'r, 't>,
    depth: usize,
    ltype: ListType,
) -> Result<Option<Element<'t>>, ParseError>
where
    'r: 't,
{
    if parser.current().token != Token::LeftBlock {
        return Ok(None);
    }

    let state = parser.get_mutable_state();
    let mut candidate = parser.clone();
    let opener_start = candidate.current().span.start;
    let parsed: Result<Option<Element<'t>>, ParseError> = (|| {
        let (name, in_head, segment) = candidate.get_block_name(false)?;
        let Some((block_rule, _, close_name)) = alignment_rule(&name) else {
            return Ok(None);
        };
        if !candidate.head_word_is_actionable(
            &name,
            matches!(name, Cow::Borrowed(_)),
            segment,
        ) {
            return Ok(None);
        }
        candidate.set_block(block_rule);
        let body_start = candidate.get_head_none_with_body_start(block_rule, in_head)?;
        if body_start != BlockBodyStart::NextPhysicalLine {
            return Ok(None);
        }
        if !first_matching_close_is_unprefixed(&candidate, close_name, depth, ltype) {
            return Ok(None);
        }

        let source = candidate.full_text().inner();
        let line_end = source[opener_start..]
            .find('\n')
            .map_or(source.len(), |offset| opener_start + offset);
        Ok(Some(text!(&source[opener_start..line_end])))
    })();

    match parsed {
        Ok(Some(literal)) => {
            parser.update(&candidate);
            Ok(Some(literal))
        }
        Ok(None) | Err(_) => {
            parser.reset_mutable_state(state);
            Ok(None)
        }
    }
}

fn first_matching_close_is_unprefixed(
    parser: &Parser<'_, '_>,
    close_name: &str,
    depth: usize,
    ltype: ListType,
) -> bool {
    let marker = match ltype {
        ListType::Bullet => '*',
        ListType::Numbered => '#',
        ListType::Generic => return false,
    };
    let source = &parser.full_text().inner()[parser.current().span.start..];
    for line in source.lines() {
        let line = line.trim_end_matches([' ', '\t', '\r']);
        if line_matches_close(line, close_name) {
            return true;
        }
        let Some(body) = line.get(depth..).filter(|_| {
            line.as_bytes()[..depth]
                .iter()
                .all(|byte| matches!(byte, b' ' | b'\t'))
        }) else {
            continue;
        };
        let Some(body) = body.strip_prefix(marker) else {
            continue;
        };
        let body = body.trim_start_matches([' ', '\t']);
        if line_matches_close(body, close_name) {
            return false;
        }
    }
    false
}

fn line_matches_close(line: &str, close_name: &str) -> bool {
    line.strip_prefix("[[/")
        .and_then(|inner| inner.strip_suffix("]]"))
        .is_some_and(|inner| inner.eq_ignore_ascii_case(close_name))
}

fn parse_lost_owner_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
    owner: LineOwner,
) -> Result<Option<LostOwnerBlock<'t>>, ParseError>
where
    'r: 't,
{
    let entry_parser = parser.clone();
    let head_byte = entry_parser.current().span.start;
    let (name, in_head, segment) = parser.get_block_name(false)?;
    if !parser.head_word_is_actionable(&name, matches!(name, Cow::Borrowed(_)), segment) {
        return Ok(None);
    }

    let view = name.strip_suffix('_').unwrap_or(&name);
    // lihoist p6: inside an explicit `[[li]]` body, an explicit-list head
    // with no closer inside that body (its pair closer is allocated to the
    // outer pair) stays a literal run item in live instead of hoisting a
    // formation. Bail out of the lost-owner path so the run parser keeps
    // the head as text.
    if matches!(owner, LineOwner::List { .. })
        && (view.eq_ignore_ascii_case("ul") || view.eq_ignore_ascii_case("ol"))
        && let Some(li_head) = entry_parser.wikidot_li_collect_head()
        && let Some(body_end) = entry_parser.wikidot_li_body_end_by_head(li_head)
        && !closer_for_block_in_range(&entry_parser, head_byte, body_end, view)
    {
        return Ok(None);
    }
    if matches!(owner, LineOwner::Quote { .. } | LineOwner::List { .. })
        && (view.eq_ignore_ascii_case("ul") || view.eq_ignore_ascii_case("ol"))
        && lost_owner_list_body_is_unowned(&entry_parser)
    {
        return parse_lost_owner_list_block(entry_parser, parser, None);
    }
    if let LineOwner::List { ltype, .. } = owner
        && (view.eq_ignore_ascii_case("ul") || view.eq_ignore_ascii_case("ol"))
        && lost_owner_list_body_is_sibling_items(&entry_parser, ltype)
    {
        return parse_lost_owner_list_block(entry_parser, parser, Some(ltype));
    }

    let (head, close_name, body_start) = if name.eq_ignore_ascii_case("div") {
        parser.set_block(&BLOCK_DIV);
        let (mut arguments, body_start) =
            parser.get_head_map_with_body_start_wikidot(&BLOCK_DIV, in_head)?;
        let attributes = wikidot_div_head_attributes(parser, &mut arguments);
        (BlockHead::Div(attributes), "div", body_start)
    } else if name.eq_ignore_ascii_case("collapsible") {
        parser.set_block(&BLOCK_COLLAPSIBLE);
        let (head, body_start) = parse_collapsible_head(parser, &name, in_head)?;
        (BlockHead::Collapsible(head), "collapsible", body_start)
    } else if name.eq_ignore_ascii_case("note") {
        if !matches!(owner, LineOwner::DefinitionList) || parser.in_note() {
            return Ok(None);
        }
        parser.set_block(&BLOCK_NOTE);
        let (arguments, body_start) =
            parser.get_head_map_with_body_start_wikidot(&BLOCK_NOTE, in_head)?;
        if !arguments.is_empty() {
            return Ok(None);
        }
        let attributes = arguments.to_attribute_map(parser.settings());
        (BlockHead::Note(attributes), "note", body_start)
    } else if let Some((block_rule, alignment, close_name)) = alignment_rule(&name) {
        parser.set_block(block_rule);
        let body_start = parser.get_head_none_with_body_start(block_rule, in_head)?;
        (BlockHead::Align(alignment), close_name, body_start)
    } else {
        return Ok(None);
    };

    if body_start != BlockBodyStart::NextPhysicalLine {
        return Ok(None);
    }
    if matches!(owner, LineOwner::List { .. }) && matches!(head, BlockHead::Align(_)) {
        return Ok(None);
    }
    if matches!(owner, LineOwner::DefinitionList) && matches!(head, BlockHead::Align(_)) {
        return Ok(None);
    }
    if matches!(head, BlockHead::Align(_)) && parser.wikidot_in_scored_div_direct_body() {
        return Ok(None);
    }
    if let LineOwner::Quote { depth } = owner
        && current_line_has_quote_owner(parser, depth)
    {
        return Ok(None);
    }
    if matches!(head, BlockHead::Collapsible(_))
        && parser.settings().layout.legacy()
        && parser
            .wikidot_block_pair_at_head_byte(head_byte)
            .is_none()
        && (parser.discarding_hidden_body()
            || parser.wikidot_scan_shows_stolen_head(
                crate::parsing::parser::block_pairs::WikidotBlockKind::Collapsible,
                head_byte,
            ))
    {
        return Ok(None);
    }
    if !has_matching_close(parser, close_name, owner)? {
        return Ok(None);
    }

    let previous_depth = parser.native_blockquote_depth();
    let previous_cursor = parser.quote_body_cursor();
    let previous_quote_boundary = parser.quote_boundary_closes_body();
    let previous_collapsible = parser.in_wikidot_collapsible();
    let lost_div = matches!(head, BlockHead::Div(_));
    let lost_collapsible = matches!(head, BlockHead::Collapsible(_));
    parser.set_native_blockquote_depth(None);
    parser.set_quote_body_cursor(None);
    if matches!(owner, LineOwner::List { .. }) {
        parser.set_quote_boundary_closes_body(true);
        if lost_div {
            parser.enter_wikidot_div_body();
        }
        if lost_collapsible {
            parser.set_in_wikidot_collapsible(true);
        }
    }
    let mut append_unquoted_close_break = false;
    let mut residual_list_close = None;
    let mut close_tail = None;
    let body = gather_paragraphs(
        parser,
        RULE_PAGE,
        Some(|parser: &mut Parser<'r, 't>| {
            if parser.settings().layout.legacy()
                && !parser.discarding_hidden_body()
                && parser.wikidot_stack_accepts_table_cell_end()
                && parser.wikidot_flat_pair_open_at(parser.current().span.start)
            {
                return Ok(true);
            }
            consume_matching_close(
                parser,
                close_name,
                owner,
                &mut append_unquoted_close_break,
                &mut residual_list_close,
                &mut close_tail,
            )
        }),
    );
    if matches!(owner, LineOwner::List { .. }) {
        if lost_div {
            parser.leave_wikidot_div_body();
        }
        parser.set_in_wikidot_collapsible(previous_collapsible);
        parser.set_quote_boundary_closes_body(previous_quote_boundary);
    }
    parser.set_native_blockquote_depth(previous_depth);
    parser.set_quote_body_cursor(previous_cursor);
    let (mut body, mut errors, _) = body?.into();
    if matches!(owner, LineOwner::List { .. })
        && matches!(head, BlockHead::Collapsible(_))
    {
        absorb_post_close_lazy_lines(parser, &mut body, &mut errors);
    }
    trim_trailing_physical_break(&mut body);
    if let Some(tail) = close_tail {
        body.push(Element::Text(tail));
    }
    if let Some(ltype) = residual_list_close {
        append_residual_list_close(&mut body, ltype);
    }

    let control = match head {
        _ if matches!(owner, LineOwner::DefinitionList) => None,
        BlockHead::Div(attributes) => {
            // Live keeps an empty shell only when the authored head carries a
            // non-empty attribute; a bare authored div head loses the line.
            attributes
                .get()
                .values()
                .any(|value| !value.is_empty())
                .then(|| {
                    Element::Container(Container::new(
                        ContainerType::Div,
                        Vec::new(),
                        attributes,
                    ))
                })
        }
        BlockHead::Align(alignment) => Some(Element::Container(Container::new(
            ContainerType::Align(alignment),
            Vec::new(),
            AttributeMap::new(),
        ))),
        BlockHead::Collapsible(head) => Some(head.into_element(Vec::new())),
        BlockHead::Note(attributes) => Some(Element::Container(Container::new(
            ContainerType::Note,
            Vec::new(),
            attributes,
        ))),
    };
    Ok(Some(LostOwnerBlock {
        control,
        body,
        errors,
        append_unquoted_close_break,
        hoisted_list: false,
        sibling_hoisted: false,
    }))
}

/// Live keeps the lines after a list-line collapsible close in the item's
/// own run: they render as bare continuations (joined by breaks), never as
/// paragraphs, until a freshening block or a blank line ends the run. This
/// absorbs each following lazy line's inline elements into the body; the
/// caller gathers them with the saved close, so the run stays unwrapped.
fn absorb_post_close_lazy_lines<'r, 't>(
    parser: &mut Parser<'r, 't>,
    body: &mut Vec<Element<'t>>,
    errors: &mut Vec<ParseError>,
) where
    'r: 't,
{
    let lazy_line_closes = [
        ParseCondition::current(Token::LineBreak),
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::current(Token::InputEnd),
    ];
    loop {
        if !parser.start_of_line()
            || matches!(
                parser.current().token,
                Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
            )
        {
            return;
        }
        let mut candidate = parser.clone();
        let success = match collect_consume_keep_split(
            &mut candidate,
            RULE_LIST,
            &lazy_line_closes,
            &[],
            None,
        ) {
            Ok(success) => success,
            Err(_) => return,
        };
        let ((elements, _, _), line_errors, _) = success.into();
        // A freshening block, a blank line, or whitespace-only content
        // ends the lazy run; leave it for the later gather.
        if elements.is_empty()
            || elements.iter().all(Element::is_whitespace)
            || !elements.iter().all(Element::paragraph_safe)
        {
            return;
        }
        parser.update(&candidate);
        errors.extend(line_errors);
        body.push(Element::LineBreak);
        body.extend(elements);
        if parser.current().token == Token::LineBreak && parser.step().is_err() {
            return;
        }
    }
}

/// Whether the explicit list head on a line-owner line has an unowned body.
/// Live hoists the list out of the line container only when the body lines are
/// not themselves owned by it; a fully owned body (every line carries the same
/// quote or list prefix) keeps the list inside the container.
/// Whether the `[[...]]` head at `head_byte` begins a list-item line: the
/// line it is on carries only quote markers, list markers, and whitespace
/// before it.
pub(crate) fn head_on_list_item_line(source: &str, head_byte: usize) -> bool {
    list_item_line_marker(source, head_byte).is_some()
}

/// The list marker (`*` or `#`) when the `[[...]]` head at `head_byte`
/// begins a list-item line: the line it is on carries only quote markers,
/// one list marker, and whitespace before it.
pub(crate) fn list_item_line_marker(source: &str, head_byte: usize) -> Option<char> {
    let from = head_byte.min(source.len());
    let line_start = source[..from].rfind('\n').map_or(0, |index| index + 1);
    let prefix = &source[line_start..from];
    let trimmed = prefix.trim_start_matches([' ', '\t', '>', '\r']);
    let (marker, rest) = if let Some(rest) = trimmed.strip_prefix('*') {
        ('*', rest)
    } else if let Some(rest) = trimmed.strip_prefix('#') {
        ('#', rest)
    } else {
        return None;
    };
    rest.chars().all(char::is_whitespace).then_some(marker)
}

/// Whether the body source contains a starred `[[ul]]`/`[[ol]]` closer
/// line (a starred list pair formation closing inside the body).
pub(crate) fn starred_list_closer_in_source(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim_start_matches([' ', '\t', '>']);
        let Some(rest) = trimmed
            .strip_prefix('*')
            .or_else(|| trimmed.strip_prefix('#'))
        else {
            return false;
        };
        let lower = rest.trim_start_matches([' ', '\t']).to_ascii_lowercase();
        ["[[/ul]]", "[[/ul_]]", "[[/ol]]", "[[/ol_]]"]
            .iter()
            .any(|needle| lower.starts_with(needle))
    })
}

/// Whether a `[[name]]` closer line (starred or unstarred) exists strictly
/// between `from` and the `[[li]]` body end byte.
pub(crate) fn closer_for_block_in_range(
    parser: &Parser<'_, '_>,
    from: usize,
    to: usize,
    name: &str,
) -> bool {
    let source = parser.full_text().inner();
    let from = from.min(source.len());
    let to = to.min(source.len());
    if to <= from {
        return false;
    }
    let range = &source[from..to];
    let lowered = range.to_ascii_lowercase();
    let needle = format!("[[/{name}");
    let name = name.to_ascii_lowercase();
    let _ = name;
    let mut pos = 0;
    while let Some(rel) = lowered[pos..].find(&needle) {
        let at = pos + rel;
        let after = range.get(at + needle.len()..);
        if after.is_some_and(|rest| {
            rest.starts_with("]]")
                || rest.starts_with(' ')
                || rest.starts_with('\t')
                || rest.starts_with('_')
                || rest.starts_with('\n')
        }) {
            return true;
        }
        pos = at + needle.len();
    }
    false
}

/// The list marker (`*` or `#`) when the `[[name]]` closer consumed ending
/// at `to` sits on a list-item line (LT46: a closer on a list-item line
/// changes the post-list break: bullet drops it, numbered anchors).
pub(crate) fn consumed_closer_line_marker(
    parser: &Parser<'_, '_>,
    to: usize,
    name: &str,
) -> Option<char> {
    let source = parser.full_text().inner();
    let to = to.min(source.len());
    let range = &source[..to];
    let lowered = range.to_ascii_lowercase();
    let needle = format!("[[/{}", name.to_ascii_lowercase());
    let mut pos = 0;
    let mut closer_head = None;
    while let Some(rel) = lowered[pos..].find(&needle) {
        let at = pos + rel;
        let after = range.get(at + needle.len()..);
        if after.is_some_and(|rest| {
            rest.starts_with("]]")
                || rest.starts_with(' ')
                || rest.starts_with('\t')
                || rest.starts_with('_')
                || rest.starts_with('\n')
        }) {
            closer_head = Some(at);
        }
        pos = at + needle.len();
    }
    closer_head.and_then(|head_byte| list_item_line_marker(source, head_byte))
}

fn lost_owner_list_body_is_unowned(parser: &Parser<'_, '_>) -> bool {
    let source = parser.full_text().inner();
    let start = parser.current().span.start.min(source.len());
    let Some(offset) = source[start..].find('\n') else {
        return false;
    };
    let body_start = start + offset + 1;
    source[body_start..]
        .lines()
        .next()
        .is_some_and(|line| line.trim_start().starts_with("[["))
}

fn lost_owner_list_body_is_sibling_items(parser: &Parser<'_, '_>, ltype: ListType) -> bool {
    let marker = match ltype {
        ListType::Bullet => "*",
        ListType::Numbered => "#",
        ListType::Generic => return false,
    };
    let source = parser.full_text().inner();
    let start = parser.current().span.start.min(source.len());
    let Some(offset) = source[start..].find('\n') else {
        return false;
    };
    let body_start = start + offset + 1;
    source[body_start..].lines().next().is_some_and(|line| {
        let line = line.trim_start();
        line.strip_prefix(marker)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    })
}

/// One part of a decomposed hoist: either a run of list items (rendered
/// as a native list) or bare elements (un-item'd lines, which live keeps
/// between the split lists instead of gluing into an item).
enum HoistPart<'t> {
    Items(Vec<ListItem<'t>>),
    Bare(Vec<Element<'t>>),
}

/// A hoisted sibling explicit list decomposes into one native list per
/// run that the explicit list packed: a sibling run that holds a
/// collapsible (or follows one) is its own list, so the followers of an
/// item collapsible form a separate list instead of continuing the item
/// that holds the collapsible.
fn hoisted_split_lists<'t>(element: Element<'t>, ltype: ListType) -> Vec<Element<'t>> {
    let Element::List {
        attributes, items, ..
    } = element
    else {
        return Vec::new();
    };
    let mut parts: Vec<HoistPart<'t>> = Vec::new();
    let mut current: Vec<ListItem<'t>> = Vec::new();
    let mut armed = false;
    for item in items {
        collect_hoisted_chunk_item(item, &mut parts, &mut current, &mut armed);
    }
    if !current.is_empty() {
        parts.push(HoistPart::Items(current));
    }
    for part in &mut parts {
        if let HoistPart::Items(items) = part {
            items.retain(|item| !hoisted_item_is_empty(item));
        }
    }
    parts
        .into_iter()
        .filter_map(|part| match part {
            HoistPart::Items(items) if !items.is_empty() => Some(vec![Element::List {
                ltype,
                attributes: attributes.clone(),
                items,
                wikidot_explicit_block: false,
                wikidot_unclosed: false,
            }]),
            HoistPart::Bare(elements) => Some(elements),
            HoistPart::Items(_) => None,
        })
        .flatten()
        .collect()
}

/// Seals the current chunk so the following sibling run starts a new list.
fn start_new_hoisted_chunk<'t>(
    parts: &mut Vec<HoistPart<'t>>,
    current: &mut Vec<ListItem<'t>>,
) {
    if !current.is_empty() {
        parts.push(HoistPart::Items(std::mem::take(current)));
    }
}

/// Seals a pending run of non-list elements as bare elements between the
/// split lists (a run of only line breaks carries no content).
fn seal_hoisted_bare<'t>(
    parts: &mut Vec<HoistPart<'t>>,
    current: &mut Vec<ListItem<'t>>,
    rest_run: &mut Vec<Element<'t>>,
) {
    if !rest_run.is_empty()
        && !rest_run
            .iter()
            .all(|element| matches!(element, Element::LineBreak))
    {
        start_new_hoisted_chunk(parts, current);
        parts.push(HoistPart::Bare(std::mem::take(rest_run)));
    }
}

/// One positioned piece of a hoisted chunk item's content: a native list
/// or a run of non-list elements, in source order.
enum HoistedSeq<'t> {
    List(Element<'t>),
    Rest(Vec<Element<'t>>),
}

fn push_hoisted_seq<'t>(out: &mut Vec<HoistedSeq<'t>>, element: Element<'t>) {
    match element {
        Element::List { .. } => out.push(HoistedSeq::List(element)),
        Element::Partial(PartialElement::ListItem(ListItem::Elements {
            elements,
            ..
        })) => {
            for element in elements {
                push_hoisted_seq(out, element);
            }
        }
        Element::Partial(PartialElement::ListItem(ListItem::SubList { element })) => {
            if matches!(*element, Element::List { .. }) {
                out.push(HoistedSeq::List(*element));
            }
        }
        other => match out.last_mut() {
            Some(HoistedSeq::Rest(run)) => run.push(other),
            _ => out.push(HoistedSeq::Rest(vec![other])),
        },
    }
}

/// Collects one item of a decomposing hoist into the current chunk,
/// cutting a new chunk at each sibling run past a collapsible.
fn collect_hoisted_chunk_item<'t>(
    item: ListItem<'t>,
    parts: &mut Vec<HoistPart<'t>>,
    current: &mut Vec<ListItem<'t>>,
    armed: &mut bool,
) {
    match item {
        ListItem::SubList { element } => match *element {
            Element::List {
                items,
                wikidot_explicit_block: false,
                ..
            } => {
                if *armed || items.iter().any(hoisted_item_contains_collapsible) {
                    start_new_hoisted_chunk(parts, current);
                    *armed = true;
                }
                for sub in items {
                    let frame = hoisted_item_contains_frame(&sub);
                    collect_hoisted_chunk_item(sub, parts, current, armed);
                    if frame {
                        // A frame opener in a list item ends the live run:
                        // its following sibling items start a new list.
                        start_new_hoisted_chunk(parts, current);
                    }
                }
            }
            // An explicit nested list (or any non-list content) keeps
            // the existing flatten, inside the current chunk.
            other => {
                collect_hoisted_leaf_items(
                    ListItem::SubList {
                        element: Box::new(other),
                    },
                    current,
                );
            }
        },
        ListItem::Elements {
            attributes,
            elements,
        } => {
            let mut sequence = Vec::new();
            for element in elements {
                push_hoisted_seq(&mut sequence, element);
            }
            if !sequence
                .iter()
                .any(|part| matches!(part, HoistedSeq::List(_)))
            {
                let elements: Vec<Element<'t>> = sequence
                    .into_iter()
                    .flat_map(|part| match part {
                        HoistedSeq::Rest(run) => run,
                        HoistedSeq::List(_) => unreachable!("checked lists above"),
                    })
                    .collect();
                current.push(ListItem::Elements {
                    attributes,
                    elements,
                });
                return;
            }
            let mut rest_run: Vec<Element<'t>> = Vec::new();
            for part in sequence {
                match part {
                    HoistedSeq::List(list) => {
                        seal_hoisted_bare(parts, current, &mut rest_run);
                        match list {
                            Element::List {
                                items,
                                wikidot_explicit_block: false,
                                ..
                            } => {
                                if *armed
                                    || items.iter().any(hoisted_item_contains_collapsible)
                                {
                                    start_new_hoisted_chunk(parts, current);
                                    *armed = true;
                                }
                                for sub in items {
                                    let frame = hoisted_item_contains_frame(&sub);
                                    collect_hoisted_chunk_item(
                                        sub, parts, current, armed,
                                    );
                                    if frame {
                                        start_new_hoisted_chunk(parts, current);
                                    }
                                }
                            }
                            Element::List { items, .. } => {
                                for sub in items {
                                    collect_hoisted_leaf_items(sub, current);
                                }
                            }
                            other => {
                                collect_hoisted_leaf_items(
                                    ListItem::SubList {
                                        element: Box::new(other),
                                    },
                                    current,
                                );
                            }
                        }
                    }
                    HoistedSeq::Rest(run) => rest_run.extend(run),
                }
            }
            seal_hoisted_bare(parts, current, &mut rest_run);
        }
    }
}

fn hoisted_item_contains_frame(item: &ListItem<'_>) -> bool {
    match item {
        ListItem::Elements { elements, .. } => {
            elements.iter().any(hoisted_element_contains_frame)
        }
        ListItem::SubList { element } => match &**element {
            Element::List { items, .. } => items.iter().any(hoisted_item_contains_frame),
            other => hoisted_element_contains_frame(other),
        },
    }
}

fn hoisted_element_contains_frame(element: &Element<'_>) -> bool {
    match element {
        Element::Partial(PartialElement::WikidotBlockOpen(_)) => true,
        Element::Partial(PartialElement::ListItem(item)) => {
            hoisted_item_contains_frame(item)
        }
        Element::List { items, .. } => items.iter().any(hoisted_item_contains_frame),
        Element::Container(container) => container
            .elements()
            .iter()
            .any(hoisted_element_contains_frame),
        _ => false,
    }
}

fn hoisted_item_contains_collapsible(item: &ListItem<'_>) -> bool {
    match item {
        ListItem::Elements { elements, .. } => {
            elements.iter().any(hoisted_element_contains_collapsible)
        }
        ListItem::SubList { element } => match &**element {
            Element::List { items, .. } => items.iter().any(hoisted_item_contains_collapsible),
            other => hoisted_element_contains_collapsible(other),
        },
    }
}

fn hoisted_element_contains_collapsible(element: &Element<'_>) -> bool {
    match element {
        Element::Collapsible { .. } => true,
        Element::Partial(PartialElement::ListItem(item)) => hoisted_item_contains_collapsible(item),
        Element::List { items, .. } => items.iter().any(hoisted_item_contains_collapsible),
        Element::Container(container) => container
            .elements()
            .iter()
            .any(hoisted_element_contains_collapsible),
        _ => false,
    }
}

fn collect_hoisted_leaf_items<'t>(item: ListItem<'t>, leaves: &mut Vec<ListItem<'t>>) {
    match item {
        ListItem::SubList { element } => {
            if let Element::List { items, .. } = *element {
                for sub in items {
                    collect_hoisted_leaf_items(sub, leaves);
                }
            }
        }
        ListItem::Elements {
            attributes,
            elements,
        } => {
            let mut lists = Vec::new();
            let mut rest = Vec::new();
            partition_hoisted_elements(elements, &mut lists, &mut rest);
            if lists.is_empty() {
                leaves.push(ListItem::Elements {
                    attributes,
                    elements: rest,
                });
                return;
            }
            if !rest
                .iter()
                .all(|element| matches!(element, Element::LineBreak))
            {
                leaves.push(ListItem::Elements {
                    attributes,
                    elements: rest,
                });
            }
            for list in lists {
                if let Element::List { items, .. } = list {
                    for sub in items {
                        collect_hoisted_leaf_items(sub, leaves);
                    }
                }
            }
        }
    }
}

fn partition_hoisted_elements<'t>(
    elements: Vec<Element<'t>>,
    lists: &mut Vec<Element<'t>>,
    rest: &mut Vec<Element<'t>>,
) {
    for element in elements {
        match element {
            Element::List { .. } => lists.push(element),
            Element::Partial(PartialElement::ListItem(ListItem::Elements {
                elements,
                ..
            })) => partition_hoisted_elements(elements, lists, rest),
            Element::Partial(PartialElement::ListItem(ListItem::SubList {
                element,
            })) => {
                if let Element::List { .. } = *element {
                    lists.push(*element);
                }
            }
            _ => rest.push(element),
        }
    }
}

fn hoisted_item_is_empty(item: &ListItem<'_>) -> bool {
    match item {
        ListItem::Elements { elements, .. } => {
            elements.iter().all(hoisted_element_is_empty)
        }
        ListItem::SubList { element } => match &**element {
            Element::List { items, .. } => items.iter().all(hoisted_item_is_empty),
            _ => false,
        },
    }
}

fn hoisted_element_is_empty(element: &Element<'_>) -> bool {
    match element {
        Element::LineBreak => true,
        Element::Partial(PartialElement::ListItem(item)) => hoisted_item_is_empty(item),
        Element::List { items, .. } => items.iter().all(hoisted_item_is_empty),
        _ => false,
    }
}

fn parse_lost_owner_list_block<'r, 't>(
    entry: Parser<'r, 't>,
    parser: &mut Parser<'r, 't>,
    decompose_siblings: Option<ListType>,
) -> Result<Option<LostOwnerBlock<'t>>, ParseError>
where
    'r: 't,
{
    let mut candidate = entry.clone_with_rule(super::RULE_BLOCK);
    let previous_depth = candidate.native_blockquote_depth();
    let previous_cursor = candidate.quote_body_cursor();
    candidate.set_native_blockquote_depth(None);
    candidate.set_quote_body_cursor(None);
    let parsed = super::RULE_BLOCK.try_consume(&mut candidate);
    candidate.set_native_blockquote_depth(previous_depth);
    candidate.set_quote_body_cursor(previous_cursor);
    let success = match parsed {
        Ok(success) => success,
        Err(_) => return Ok(None),
    };
    let (elements, errors, _) = success.into();
    let elements: Vec<Element<'t>> = elements.into_iter().collect();
    let explicit_list_at_start = matches!(
        elements.first(),
        Some(Element::List {
            wikidot_explicit_block: true,
            ..
        })
    ) && elements[1..]
        .iter()
        .all(|element| matches!(element, Element::LineBreak));
    if !explicit_list_at_start {
        return Ok(None);
    }
    let mut elements = elements;
    if matches!(
        elements.first(),
        Some(Element::List {
            ltype: ListType::Numbered,
            ..
        })
    ) {
        while matches!(elements.last(), Some(Element::LineBreak)) {
            elements.pop();
        }
        if let Some(Element::List {
            ltype,
            items,
            wikidot_unclosed,
            ..
        }) = elements.first_mut()
        {
            *ltype = ListType::Bullet;
            *wikidot_unclosed = true;
            let mut item_attributes = AttributeMap::new();
            item_attributes.insert("style", cow!("list-style: none"));
            items.push(ListItem::Elements {
                elements: vec![Element::LineBreak],
                attributes: item_attributes,
            });
        }
    }
    parser.update(&candidate);
    let body = if let Some(sibling_ltype) = decompose_siblings {
        let mut elements = elements.into_iter();
        match elements.next() {
            Some(list) => hoisted_split_lists(list, sibling_ltype),
            None => Vec::new(),
        }
    } else {
        elements
    };
    Ok(Some(LostOwnerBlock {
        control: None,
        body,
        errors,
        append_unquoted_close_break: false,
        hoisted_list: true,
        sibling_hoisted: decompose_siblings.is_some(),
    }))
}

fn has_matching_close<'r, 't>(
    parser: &Parser<'r, 't>,
    close_name: &'static str,
    owner: LineOwner,
) -> Result<bool, ParseError>
where
    'r: 't,
{
    let (owner_kind, owner_depth) = match owner {
        LineOwner::Quote { depth } => (0, depth),
        LineOwner::List {
            depth,
            ltype: ListType::Bullet,
        } => (1, depth),
        LineOwner::List {
            depth,
            ltype: ListType::Numbered,
        } => (2, depth),
        LineOwner::List {
            depth,
            ltype: ListType::Generic,
        } => (3, depth),
        LineOwner::DefinitionList => (4, 0),
    };
    let mut scan = parser.clone();
    let mut visited = Vec::new();
    loop {
        let token_start = scan.current().span.start;
        let start_of_line = scan.start_of_line();
        if let Some(outcome) = scan.lost_owner_scan_outcome((
            close_name,
            owner_kind,
            owner_depth,
            token_start,
            start_of_line,
        )) {
            parser.cache_lost_owner_scan_outcomes(
                close_name,
                owner_kind,
                owner_depth,
                &visited,
                outcome,
            );
            return Ok(outcome);
        }
        visited.push((token_start, start_of_line));
        #[cfg(test)]
        parser.increment_lost_owner_scan_token_visits();

        if let Some(literal_end) = scan.lost_owner_literal_range_end(token_start) {
            while scan.current().token != Token::InputEnd
                && scan.current().span.start < literal_end
            {
                scan.step()?;
            }
            continue;
        }
        if scan.current().token == Token::LeftComment {
            let mut comment = scan.clone();
            if consume_valid_comment(&mut comment).is_ok() {
                scan.update(&comment);
                continue;
            }
        }

        let mut ignored_break = false;
        let mut ignored_residual_close = None;
        let mut ignored_tail = None;
        if consume_matching_close(
            &mut scan,
            close_name,
            owner,
            &mut ignored_break,
            &mut ignored_residual_close,
            &mut ignored_tail,
        )? {
            parser.cache_lost_owner_scan_outcomes(
                close_name,
                owner_kind,
                owner_depth,
                &visited,
                true,
            );
            return Ok(true);
        }
        if scan.current().token == Token::InputEnd {
            parser.cache_lost_owner_scan_outcomes(
                close_name,
                owner_kind,
                owner_depth,
                &visited,
                false,
            );
            return Ok(false);
        }
        scan.step()?;
    }
}

fn alignment_rule(name: &str) -> Option<(&'static BlockRule, Alignment, &'static str)> {
    match name {
        "<" => Some((&BLOCK_ALIGN_LEFT, Alignment::Left, "<")),
        ">" => Some((&BLOCK_ALIGN_RIGHT, Alignment::Right, ">")),
        "=" => Some((&BLOCK_ALIGN_CENTER, Alignment::Center, "=")),
        "==" => Some((&BLOCK_ALIGN_JUSTIFY, Alignment::Justify, "==")),
        _ => None,
    }
}

fn trim_trailing_physical_break(elements: &mut Vec<Element<'_>>) {
    if matches!(elements.last(), Some(Element::LineBreak)) {
        elements.pop();
        return;
    }
    let Some(Element::Container(container)) = elements.last_mut() else {
        return;
    };
    if container.ctype() == ContainerType::Paragraph
        && matches!(container.elements().last(), Some(Element::LineBreak))
    {
        container.elements_mut().pop();
    }
}

fn append_residual_list_close<'t>(elements: &mut Vec<Element<'t>>, ltype: ListType) {
    let marker = match ltype {
        ListType::Bullet => "*",
        ListType::Numbered => "#",
        ListType::Generic => return,
    };
    if let Some(Element::Container(container)) = elements.last_mut()
        && container.ctype() == ContainerType::Paragraph
    {
        container.elements_mut().push(Element::LineBreak);
        container.elements_mut().push(text!(marker));
        return;
    }
    elements.push(Element::Container(Container::new(
        ContainerType::Paragraph,
        vec![text!(marker)],
        AttributeMap::new(),
    )));
}

fn current_line_has_quote_owner(parser: &Parser<'_, '_>, required_depth: usize) -> bool {
    parser.start_of_line()
        && parser.current().token == Token::Quote
        && parser.current().slice.len() >= required_depth
}

fn consume_matching_close<'r, 't>(
    parser: &mut Parser<'r, 't>,
    close_name: &str,
    owner: LineOwner,
    append_unquoted_close_break: &mut bool,
    residual_list_close: &mut Option<ListType>,
    close_tail: &mut Option<Cow<'t, str>>,
) -> Result<bool, ParseError>
where
    'r: 't,
{
    if !parser.start_of_line() {
        return Ok(false);
    }

    for prefixed in [false, true] {
        let mut close = parser.clone();
        if prefixed && !consume_owner_prefix(&mut close, owner)? {
            continue;
        }
        if !prefixed && close.current().token != Token::LeftBlockEnd {
            continue;
        }

        let Ok(name) = close.get_end_block() else {
            continue;
        };
        if !name.eq_ignore_ascii_case(close_name) {
            continue;
        }
        close.get_optional_space()?;
        if !matches!(
            close.current().token,
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
        ) {
            if !matches!(owner, LineOwner::DefinitionList) {
                continue;
            }
            let source = close.full_text().inner();
            let start = close.current().span.start;
            let end = source[start..]
                .find('\n')
                .map_or(source.len(), |offset| start + offset);
            *close_tail = Some(Cow::Borrowed(source[start..end].trim_end_matches('\r')));
            while close.current().span.start < end
                && close.current().token != Token::InputEnd
            {
                close.step()?;
            }
            parser.update(&close);
            return Ok(true);
        }
        let trailing_line_break = close.current().token == Token::LineBreak;
        if trailing_line_break {
            close.step()?;
        }
        if matches!(owner, LineOwner::Quote { .. }) && !prefixed && trailing_line_break {
            *append_unquoted_close_break = true;
        }
        if let LineOwner::List { depth, ltype } = owner
            && depth > 0
            && prefixed
        {
            *residual_list_close = Some(ltype);
        }
        parser.update(&close);
        return Ok(true);
    }

    Ok(false)
}

fn consume_owner_prefix(
    parser: &mut Parser<'_, '_>,
    owner: LineOwner,
) -> Result<bool, ParseError> {
    match owner {
        LineOwner::Quote { depth } => {
            if parser.current().token != Token::Quote
                || parser.current().slice.len() != depth
            {
                return Ok(false);
            }
            parser.step()?;
            parser.get_optional_space()?;
            Ok(true)
        }
        LineOwner::DefinitionList => {
            if parser.current().token != Token::Colon {
                return Ok(false);
            }
            parser.step()?;
            parser.get_optional_space()?;
            Ok(true)
        }
        LineOwner::List { depth, ltype } => {
            if depth > 0 {
                if parser.current().token != Token::Whitespace
                    || parser.current().slice.len() != depth
                {
                    return Ok(false);
                }
                parser.step()?;
            }
            let expected = match ltype {
                ListType::Bullet => Token::BulletItem,
                ListType::Numbered => Token::NumberedItem,
                ListType::Generic => return Ok(false),
            };
            if parser.current().token != expected {
                return Ok(false);
            }
            parser.step()?;
            if parser.current().token != Token::Whitespace {
                return Ok(false);
            }
            parser.step()?;
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    fn with_lost(
        source: &str,
        owner: LineOwner,
        test: impl FnOnce(Option<LostOwnerBlock<'_>>),
    ) {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().unwrap();
        test(try_consume_lost_owner_block(&mut parser, owner).unwrap());
    }

    #[test]
    fn lost_quote_owner_parses_unquoted_body_and_consumes_marked_close() {
        with_lost(
            "[[div]]\nbody\n> [[/div]]",
            LineOwner::Quote { depth: 1 },
            |lost| {
                let lost = lost.unwrap();
                assert!(lost.control.is_none());
                assert!(!lost.body.is_empty());
            },
        );
    }

    #[test]
    fn fully_quoted_body_keeps_normal_block_authority() {
        with_lost(
            "[[div]]\n> body\n> [[/div]]",
            LineOwner::Quote { depth: 1 },
            |lost| assert!(lost.is_none()),
        );
    }

    #[test]
    fn missing_lost_owner_closes_scan_each_token_once() {
        let source = "[[div]]\nbody\n".repeat(256);
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().unwrap();

        let mut openers = 0;
        while parser.current().token != Token::InputEnd {
            if parser.current().token == Token::LeftBlock {
                assert!(
                    try_consume_lost_owner_block(
                        &mut parser,
                        LineOwner::Quote { depth: 1 },
                    )
                    .unwrap()
                    .is_none()
                );
                openers += 1;
            }
            parser.step().unwrap();
        }

        assert_eq!(openers, 256);
        assert!(
            parser.lost_owner_scan_token_visits() <= tokenization.tokens().len(),
            "visited {} tokens for {} input tokens",
            parser.lost_owner_scan_token_visits(),
            tokenization.tokens().len(),
        );
    }

    #[test]
    fn lost_owner_operation_count_scales_with_input_tokens() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for repetitions in [32, 64, 128, 256, 512] {
            let source = "[[div]]\nbody\n".repeat(repetitions);
            let tokenization = crate::tokenize(&source);
            let mut parser = Parser::new(&tokenization, &page_info, &settings);
            parser.step().unwrap();

            while parser.current().token != Token::InputEnd {
                if parser.current().token == Token::LeftBlock {
                    assert!(
                        try_consume_lost_owner_block(
                            &mut parser,
                            LineOwner::Quote { depth: 1 },
                        )
                        .unwrap()
                        .is_none()
                    );
                }
                parser.step().unwrap();
            }

            assert!(
                parser.lost_owner_scan_token_visits() <= tokenization.tokens().len(),
                "{repetitions} repetitions visited {} tokens for {} input tokens",
                parser.lost_owner_scan_token_visits(),
                tokenization.tokens().len(),
            );
        }
    }

    #[test]
    fn literal_owner_closes_do_not_satisfy_lost_owner_lookahead() {
        let unit = concat!(
            "[[div]]\n",
            "[[code]]\n[[/div]]\n[[/code]]\n",
            "[[raw]]\n[[/div]]\n[[/raw]]\n",
            "[[html]]\n[[/div]]\n[[/html]]\n",
            "@@[[/div]]@@\n",
            "[!-- [[/div]] --]\n",
        );
        let source = unit.repeat(128);
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().unwrap();

        let mut openers = 0;
        while parser.current().token != Token::InputEnd {
            let start = parser.current().span.start;
            if parser.current().token == Token::LeftBlock
                && source[start..].starts_with("[[div]]")
            {
                assert!(
                    try_consume_lost_owner_block(
                        &mut parser,
                        LineOwner::Quote { depth: 1 },
                    )
                    .unwrap()
                    .is_none()
                );
                openers += 1;
            }
            parser.step().unwrap();
        }

        assert_eq!(openers, 128);
        assert!(
            parser.lost_owner_scan_token_visits() <= tokenization.tokens().len(),
            "visited {} tokens for {} input tokens",
            parser.lost_owner_scan_token_visits(),
            tokenization.tokens().len(),
        );
    }

    #[test]
    fn lost_owner_close_cache_keeps_list_depths_separate() {
        let source = "[[div]]\n * [[/div]]";
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().unwrap();
        let (name, in_head, _) = parser.get_block_name(false).unwrap();
        assert!(name.eq_ignore_ascii_case("div"));
        parser.set_block(&BLOCK_DIV);
        let (_, body_start) = parser
            .get_head_map_with_body_start_wikidot(&BLOCK_DIV, in_head)
            .unwrap();
        assert_eq!(body_start, BlockBodyStart::NextPhysicalLine);

        assert!(
            has_matching_close(
                &parser,
                "div",
                LineOwner::List {
                    depth: 1,
                    ltype: ListType::Bullet,
                },
            )
            .unwrap()
        );
        assert!(
            !has_matching_close(
                &parser,
                "div",
                LineOwner::List {
                    depth: 2,
                    ltype: ListType::Bullet,
                },
            )
            .unwrap()
        );
    }
}
