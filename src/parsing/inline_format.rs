use crate::parsing::rule::impls::{
    is_wikidot_only_autolink_url, normalize_wikidot_color,
};
use crate::tree::{
    AnchorTarget, AttributeMap, Container, ContainerType, Element, InlineFormatKind,
    LinkLabel, LinkLocation, ListItem, PartialElement,
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
            changed |= repair_monospace_split_close_pass(elements);
            changed |= repair_monospace_link_close_pass(elements);
        }
        changed |=
            repair_wikidot_scheme_star_pass(elements) | repair_dash_runs_pass(elements);
        if !changed {
            break;
        }
    }

    for element in elements.iter_mut() {
        if matches!(element, Element::Container(container) if container.ctype() == ContainerType::Monospace)
        {
            visit_children_mut(element, normalize_sequence_inside_monospace);
        } else {
            visit_children_mut(element, normalize_sequence);
        }
    }

    lower_dash_runs(elements);
    substitute_number_unit_spaces(elements);
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
        let valid_open = input[index]
            .as_ref()
            .is_some_and(|element| is_exact_text(element, "--"))
            && input
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
    (1..children.len().saturating_sub(1)).find_map(|child_index| {
        let Element::Text(value) = &children[child_index] else {
            return None;
        };
        let (prefix, suffix) = strike_close_split(value.as_ref())?;
        if prefix.is_empty() && element_ends_with_ascii_space(&children[child_index - 1])
        {
            return None;
        }
        if !children[..child_index].iter().any(element_contains_link) {
            return None;
        }
        Some((child_index, prefix.to_string(), suffix.to_string()))
    })
}

fn repair_strike_shed_pairs_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let mut input = take_elements(elements);
    let mut output = Vec::with_capacity(input.len());
    let mut changed = false;
    let mut index = 0;

    while index < input.len() {
        let Some((opener_len, excess)) = strike_shed_opener(&input, index) else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };
        let Some((close, prefix, suffix)) = strike_shed_close(&input, index, opener_len)
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
        if next.is_some_and(element_starts_with_ascii_space) {
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

fn strike_shed_close(
    input: &[Option<Element<'_>>],
    opener: usize,
    opener_len: usize,
) -> Option<(usize, String, String)> {
    let first = opener + opener_len;
    let (close, prefix, suffix) = (first..input.len()).find_map(|close_index| {
        let element = input[close_index].as_ref()?;
        let Element::Text(value) = element else {
            return None;
        };
        let (prefix, suffix) = strike_close_split(value.as_ref())?;
        if prefix.is_empty()
            && input[close_index - 1]
                .as_ref()
                .is_some_and(|previous| element_ends_with_ascii_space(previous))
        {
            return None;
        }
        if close_index == first && prefix.is_empty() {
            return None;
        }
        Some((close_index, prefix.to_string(), suffix.to_string()))
    })?;
    if !input[first..close]
        .iter()
        .filter_map(|element| element.as_ref())
        .any(element_contains_link)
    {
        return None;
    }
    Some((close, prefix, suffix))
}

pub(crate) fn element_contains_link(element: &Element<'_>) -> bool {
    matches!(element, Element::Link { .. })
        || formatter_children(element)
            .is_some_and(|children| children.iter().any(element_contains_link))
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
    (1..children.len()).find_map(|child_index| {
        let Element::Text(value) = &children[child_index] else {
            return None;
        };
        let (prefix, suffix) = shed_text_pair_split(value.as_ref(), marker)?;
        if prefix.is_empty() && element_ends_with_ascii_space(&children[child_index - 1])
        {
            return None;
        }
        if !children[..child_index].iter().any(element_contains_link) {
            return None;
        }
        Some((child_index, prefix.to_string(), suffix.to_string()))
    })
}

fn repair_script_pairs_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let mut input = take_elements(elements);
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
        let Some((close, prefix, suffix)) = script_pair_close(&input, index, kind) else {
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

fn script_pair_close(
    input: &[Option<Element<'_>>],
    index: usize,
    kind: FormatKind,
) -> Option<(usize, String, String)> {
    let next = input.get(index + 1)?.as_ref()?;
    if element_starts_with_ascii_space(next) {
        return None;
    }
    let marker = kind.close_marker();
    (index + 1..input.len()).find_map(|close_index| {
        let element = input[close_index].as_ref()?;
        let Element::Text(value) = element else {
            return None;
        };
        let (prefix, suffix) = shed_text_pair_split(value.as_ref(), marker)?;
        if prefix.is_empty()
            && input[close_index - 1]
                .as_ref()
                .is_some_and(|previous| element_ends_with_ascii_space(previous))
        {
            return None;
        }
        if close_index == index + 1 && prefix.is_empty() {
            return None;
        }
        Some((close_index, prefix.to_string(), suffix.to_string()))
    })
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
    (1..children.len().saturating_sub(1)).find_map(|child_index| {
        let Element::Text(value) = &children[child_index] else {
            return None;
        };
        let (prefix, suffix) = monospace_close_split(value.as_ref())?;
        if prefix.is_empty() && element_ends_with_ascii_space(&children[child_index - 1])
        {
            return None;
        }
        if !children[..child_index].iter().any(element_contains_link) {
            return None;
        }
        Some((child_index, prefix.to_string(), suffix.to_string()))
    })
}

fn repair_monospace_link_close_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let mut input = take_elements(elements);
    let mut output = Vec::with_capacity(input.len());
    let mut changed = false;
    let mut index = 0;

    while index < input.len() {
        let is_opener = input[index]
            .as_ref()
            .is_some_and(|element| is_exact_text(element, "{{"));
        let close = is_opener
            .then(|| monospace_link_close(&input, index))
            .flatten()
            .filter(|(close, _, _)| {
                // This repair exists to close a monospace span that the
                // base parser left open over a URL (issue #833). Brace
                // text around anything else is already owned content.
                input[index + 1..*close]
                    .iter()
                    .filter_map(|element| element.as_ref())
                    .any(element_contains_link)
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
) -> Option<(usize, String, String)> {
    ((opener + 1)..input.len()).find_map(|index| {
        let element = input[index].as_ref()?;
        let (prefix, suffix) = monospace_close_split(text(element)?)?;
        Some((index, prefix.to_string(), suffix.to_string()))
    })
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
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProjItem<'a> {
    Text(&'a str),
    PartialLike,
    Content,
    Barrier,
    EdgeOpen(InlineFormatKind),
    EdgeClose(InlineFormatKind),
    LinkContent,
    FormatEventOpen(InlineFormatKind),
    FormatEventClose(InlineFormatKind),
    ColorEventOpen,
    ColorEventClose,
}

#[derive(Clone, Copy)]
enum Expansion {
    Format(InlineFormatKind),
    Color,
}

const LITERAL_MARKERS: [InlineFormatKind; 3] = [
    InlineFormatKind::Bold,
    InlineFormatKind::Italics,
    InlineFormatKind::Underline,
];

const MARKER_TEXTS: [&str; 6] = ["**", "//", "__", "{{", "}}", "##"];
const MONO_EXPANSION_MARKER_TEXTS: [&str; 4] = ["**", "//", "__", "##"];

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
    project_marker_sequence(&original, &mut projection);
    let mut roles: Vec<Option<PairRole<'t>>> = Vec::new();
    roles.resize_with(projection.len(), || None);
    let mut found = false;
    for kind in LITERAL_MARKERS {
        found |= pair_symmetric_markers(&projection, &mut roles, kind);
    }
    if !inside_monospace {
        found |= pair_monospace_markers(&projection, &mut roles);
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
    projection: &mut Vec<ProjItem<'a>>,
) {
    for element in elements {
        if let Some(expansion) = expansion_of(element) {
            let children = formatter_children(element)
                .expect("expansion requires formatter children");
            match expansion {
                Expansion::Format(kind) if LITERAL_MARKERS.contains(&kind) => {
                    projection.push(ProjItem::EdgeOpen(kind));
                    project_marker_sequence(children, projection);
                    projection.push(ProjItem::EdgeClose(kind));
                }
                _ => {
                    projection.push(ProjItem::PartialLike);
                    project_marker_sequence(children, projection);
                    projection.push(ProjItem::PartialLike);
                }
            }
        } else if pairing_barrier(element) {
            projection.push(ProjItem::Barrier);
        } else {
            match element {
                Element::Text(value) => projection.push(ProjItem::Text(value)),
                Element::Partial(partial) => projection.push(match partial {
                    PartialElement::InlineFormatOpen(kind) => {
                        ProjItem::FormatEventOpen(*kind)
                    }
                    PartialElement::InlineFormatClose(kind) => {
                        ProjItem::FormatEventClose(*kind)
                    }
                    PartialElement::InlineColorOpen(..) => ProjItem::ColorEventOpen,
                    PartialElement::InlineColorClose => ProjItem::ColorEventClose,
                    _ => ProjItem::PartialLike,
                }),
                _ if element_contains_link(element) => {
                    projection.push(ProjItem::LinkContent)
                }
                _ => projection.push(ProjItem::Content),
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
            let subtree_changed = roles[open_index..=close_index]
                .iter()
                .any(Option::is_some);
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
                output.push(Element::Partial(PartialElement::InlineFormatClose(
                    *kind,
                )));
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
                        output.push(Element::Partial(
                            PartialElement::InlineColorOpen(
                                color.clone(),
                                *background,
                            ),
                        ));
                    } else {
                        output.push(Element::Partial(
                            PartialElement::InlineColorClose,
                        ));
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
                FormatKind::Italics => {
                    Some(Expansion::Format(InlineFormatKind::Italics))
                }
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
                FormatKind::Strike | FormatKind::Color => None,
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
    formatter_children(element).is_some_and(|children| {
        children.iter().any(subtree_has_barrier)
    })
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
        | Element::TableOfContents { .. } => true,
        Element::Partial(partial) => matches!(
            partial,
            PartialElement::ListItem(_)
                | PartialElement::TableRow(_)
                | PartialElement::TableCell(_)
                | PartialElement::Tab(_)
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
        ProjItem::Content | ProjItem::LinkContent => true,
        ProjItem::PartialLike
        | ProjItem::Barrier
        | ProjItem::EdgeOpen(_)
        | ProjItem::EdgeClose(_)
        | ProjItem::FormatEventOpen(_)
        | ProjItem::FormatEventClose(_)
        | ProjItem::ColorEventOpen
        | ProjItem::ColorEventClose => false,
    })
}

fn pair_symmetric_markers<'t>(
    projection: &[ProjItem<'_>],
    roles: &mut [Option<PairRole<'t>>],
    kind: InlineFormatKind,
) -> bool {
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
        if index == projection.len() || matches!(projection[index], ProjItem::Barrier)
        {
            if segment_has_literal {
                edge_active[segment_start..index].fill(true);
            }
            segment_start = index + 1;
            segment_has_literal = false;
        } else if matches!(projection[index], ProjItem::Text(value) if value == marker)
        {
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
                if !proj_ends_with_pad(&projection[index - 1])
                    && projection_has_content(&projection[open_index + 1..index])
                {
                    if siblings[open_index] != Some(index)
                        && roles[open_index].is_none()
                        && roles[index].is_none()
                    {
                        roles[open_index] = Some(PairRole::FormatOpen(kind));
                        roles[index] = Some(PairRole::FormatClose(kind));
                        for endpoint in [open_index, index] {
                            if let Some(sibling) = siblings[endpoint]
                                && roles[sibling].is_none()
                            {
                                roles[sibling] =
                                    Some(PairRole::LiteralMarker(kind));
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
                                roles[swept] =
                                    Some(PairRole::LiteralMarker(kind));
                                if let Some(sibling) = siblings[swept]
                                    && roles[sibling].is_none()
                                {
                                    roles[sibling] =
                                        Some(PairRole::LiteralMarker(kind));
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
                        found = true;
                    }
                    open = None;
                }
            }
        }
    }
    found
}

fn pair_color_markers<'t>(
    projection: &[ProjItem<'_>],
    roles: &mut [Option<PairRole<'t>>],
) -> bool {
    let mut found = false;
    let mut pending: Option<(usize, usize, Cow<'t, str>, bool)> = None;
    let mut depth = 0usize;
    for index in 0..projection.len() {
        if matches!(projection[index], ProjItem::Barrier) {
            pending = None;
            continue;
        }
        match projection[index] {
            ProjItem::ColorEventOpen => {
                depth += 1;
                continue;
            }
            ProjItem::ColorEventClose => {
                depth = depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }
        if pending.is_none() {
            if depth == 0
                && let Some((color, background, len)) =
                    color_head_at(projection, index)
            {
                pending = Some((index, index + len, color, background));
            }
            continue;
        }
        let head = pending.as_ref().unwrap().0;
        let content_start = pending.as_ref().unwrap().1;
        if depth == 0
            && index >= content_start
            && matches!(projection[index], ProjItem::Text(value) if value == "##")
            && projection_has_content(&projection[content_start..index])
        {
            let (_, _, color, background) = pending.take().unwrap();
            if roles[head].is_none() && roles[index].is_none() {
                roles[head] = Some(PairRole::ColorOpen(color, background));
                for slot in &mut roles[head + 1..content_start] {
                    *slot = Some(PairRole::Drop);
                }
                roles[index] = Some(PairRole::ColorClose);
                found = true;
            }
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
    let mut background = false;
    for (offset, item) in projection[start + 1..].iter().enumerate() {
        let ProjItem::Text(value) = item else {
            return None;
        };
        if *value == "|" {
            if color.trim().is_empty() && !background {
                background = true;
                color.clear();
                continue;
            }
            let color = normalize_wikidot_color(&color)?.into_owned();
            return Some((Cow::Owned(color), background, offset + 2));
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
            | PartialElement::InlineSizeOpen(_)
            | PartialElement::InlineSizeClose(_)
            | PartialElement::InlineSpanOpen(_)
            | PartialElement::InlineSpanClose(_)
            | PartialElement::InlineFormatOpen(_)
            | PartialElement::InlineFormatClose(_)
            | PartialElement::InlineColorOpen(..)
            | PartialElement::InlineColorClose => DashUnit::Partial,
            _ => DashUnit::Barrier,
        },
        Element::Link { .. } if admit_link => DashUnit::Atom(DASH_ATOM_CHAR),
        _ => {
            let Some(shell) = FormatShell::from_element(element) else {
                return DashUnit::Barrier;
            };
            if shell.kind() == FormatKind::Strike || subtree_has_dash_pair(element) {
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
        Element::Color { elements, .. } => {
            elements.iter().any(subtree_has_dash_pair)
        }
        Element::Link { label, .. } => match label {
            LinkLabel::Text(value) | LinkLabel::Slug(value) => value.contains("--"),
            _ => false,
        },
        _ => false,
    }
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
        stream[pos] == '-'
            && stream[pos + 1] == '-'
            && !stream[pos + 2].is_whitespace()
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
    let from = pairs
        .last()
        .map(|&(_, close)| close + 2)
        .unwrap_or(0);
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
                let value = text(
                    input[element]
                        .as_ref()
                        .expect("unit element present"),
                )
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
        let (shell, children) =
            FormatShell::into_parts(element).expect("strike container has a shell");
        let mut input: Vec<Option<Element<'t>>> =
            children.into_iter().map(Some).collect();
        let scan = build_dash_scan(
            &|index| input.get(index).and_then(|element| element.as_ref()),
            input.len(),
            0,
            true,
        );
        let close = if scan
            .stream
            .first()
            .is_some_and(|ch| !ch.is_whitespace())
        {
            dash_stream_close(&scan.stream, 0)
        } else {
            None
        };
        let Some(close) = close else {
            output.push(
                shell.build(
                    input
                        .into_iter()
                        .map(|element| element.expect("child present"))
                        .collect(),
                ),
            );
            continue;
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
        output.push(shell.build(content));
        output.append(&mut tail);
        output.push(Element::Text(FormatKind::Strike.close_marker().into()));
        changed = true;
    }

    *elements = output;
    changed
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
            if let Some(next) = expand_dash_strike_container(
                &mut input,
                index,
                &scan,
                &mut output,
            ) {
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
    let child_scan = build_dash_scan(
        &|index| children.get(index),
        children.len(),
        0,
        false,
    );
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
    flat.push(Some(Element::Text(FormatKind::Strike.close_marker().into())));
    let container_element = take_element(input, container_index);
    let (_, children) =
        FormatShell::into_parts(container_element).expect("strike container has parts");
    for child in children {
        flat.push(Some(child));
    }
    flat.push(Some(Element::Text(FormatKind::Strike.close_marker().into())));
    for element_index in container_index + 1..continuation.end {
        flat.push(Some(take_element(input, element_index)));
    }
    let flat_scan = build_dash_scan(
        &|element_index| {
            flat.get(element_index)
                .and_then(|element| element.as_ref())
        },
        flat.len(),
        0,
        false,
    );
    emit_dash_pairs(&mut flat, &flat_scan, &pairs, output);
    Some(continuation.end)
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
            if let Some(marker_index) =
                find_text_marker(children, outer_kind.close_marker())
            {
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
        let valid_close = dash_run_len(&elements[index])
            .is_some_and(|length| length >= 2)
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

fn outer_candidate<'t>(
    elements: &[Element<'t>],
    start: usize,
) -> Option<OuterCandidate<'t>> {
    let marker = text(elements.get(start)?)?;
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
    let mut background = false;
    for (offset, element) in elements[start + 1..].iter().enumerate() {
        let value = text(element)?;
        if value == "|" {
            if color.trim().is_empty() && !background {
                background = true;
                color.clear();
                continue;
            }
            let color = normalize_wikidot_color(&color)?.into_owned().into();
            return Some(OuterCandidate {
                shell: FormatShell::Color { color, background },
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
            for tab in tabs {
                visit(&mut tab.elements);
            }
        }
        Element::Anchor { elements, .. }
        | Element::Collapsible { elements, .. }
        | Element::Color { elements, .. }
        | Element::Include { elements, .. } => visit(elements),
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
            PartialElement::Tab(tab) => visit(&mut tab.elements),
            PartialElement::RubyText(ruby_text) => visit(&mut ruby_text.elements),
            PartialElement::WikidotEmptyInlineOwner
            | PartialElement::InlineSizeOpen(_)
            | PartialElement::InlineSizeClose(_)
            | PartialElement::InlineSpanOpen(_)
            | PartialElement::InlineSpanClose(_)
            | PartialElement::InlineFormatOpen(_)
            | PartialElement::InlineFormatClose(_)
            | PartialElement::InlineColorOpen(..)
            | PartialElement::InlineColorClose => {}
        },
        _ => {}
    }
}
