use crate::parsing::rule::impls::normalize_wikidot_color;
use crate::tree::{
    AttributeMap, Container, ContainerType, Element, ListItem, PartialElement,
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
    for _ in 0..MAX_NORMALIZATION_PASSES {
        let changed = repair_fifo_triples_pass(elements)
            | repair_same_family_pass(elements)
            | repair_crossed_pass(elements)
            | repair_raw_strike_pairs_pass(elements)
            | repair_span_crossed_bold_pairs_pass(elements)
            | split_strike_runs_pass(elements);
        if !changed {
            break;
        }
    }

    for element in elements.iter_mut() {
        visit_children_mut(element, normalize_sequence);
    }

    lower_dash_runs(elements);
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

fn repair_span_crossed_bold_pairs_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let closes: Vec<Option<(usize, Vec<usize>)>> = (0..elements.len())
        .map(|index| span_crossed_bold_close(elements, index))
        .collect();
    let mut input = take_elements(elements);
    let mut output = Vec::with_capacity(input.len());
    let mut changed = false;
    let mut index = 0;

    while index < input.len() {
        let Some((close, splits)) = &closes[index] else {
            output.push(take_element(&mut input, index));
            index += 1;
            continue;
        };
        discard_range(&mut input, index..index + 1);
        let mut segment_start = index + 1;
        for split in splits {
            let segment = take_range(&mut input, segment_start..*split);
            push_bold_segment(&mut output, segment);
            output.push(take_element(&mut input, *split));
            segment_start = split + 1;
        }
        let segment = take_range(&mut input, segment_start..*close);
        push_bold_segment(&mut output, segment);
        discard_range(&mut input, *close..*close + 1);
        index = close + 1;
        changed = true;
    }

    *elements = output;
    changed
}

fn push_bold_segment<'t>(output: &mut Vec<Element<'t>>, segment: Vec<Element<'t>>) {
    let has_content = segment.iter().any(|element| {
        !matches!(element, Element::Partial(_))
            && text(element).is_none_or(|value| !value.trim().is_empty())
    });
    if has_content {
        output.push(bold_shell().build(segment));
    } else {
        output.extend(segment);
    }
}

fn span_crossed_bold_close<'t>(
    elements: &[Element<'t>],
    open: usize,
) -> Option<(usize, Vec<usize>)> {
    if !is_exact_text(&elements[open], "**") {
        return None;
    }
    if elements
        .get(open + 1)
        .is_none_or(|element| element_starts_with_ascii_space(element))
    {
        return None;
    }

    let mut depth = 0usize;
    for element in &elements[..open] {
        match element {
            Element::Partial(PartialElement::InlineSpanOpen(_)) => depth += 1,
            Element::Partial(PartialElement::InlineSpanClose(_)) => {
                depth = depth.checked_sub(1)?;
            }
            _ => {}
        }
    }

    let mut floor = depth;
    let mut crossed = false;
    let mut has_content = false;
    let mut splits = Vec::new();
    let mut index = open + 1;
    while index < elements.len() {
        let element = &elements[index];
        match element {
            Element::Partial(PartialElement::InlineSpanOpen(_)) => {
                depth += 1;
                crossed = true;
            }
            Element::Partial(PartialElement::InlineSpanClose(_)) => {
                if depth == floor {
                    splits.push(index);
                    floor = floor.checked_sub(1)?;
                }
                depth = depth.checked_sub(1)?;
                crossed = true;
            }
            Element::Partial(_) => return None,
            _ if is_exact_text(element, "**") => {
                if element_ends_with_ascii_space(&elements[index - 1]) {
                    return None;
                }
                return (crossed && has_content).then_some((index, splits));
            }
            _ => {
                has_content |= text(element).is_none_or(|value| !value.trim().is_empty());
            }
        }
        index += 1;
    }
    None
}

fn split_strike_runs_pass<'t>(elements: &mut Vec<Element<'t>>) -> bool {
    let mut output = Vec::with_capacity(elements.len());
    let mut changed = false;

    for element in mem::take(elements) {
        let Element::Container(container) = element else {
            output.push(element);
            continue;
        };
        if container.ctype() != ContainerType::Strikethrough
            || !container
                .elements()
                .iter()
                .any(|element| dash_run_len(element) == Some(4))
        {
            output.push(Element::Container(container));
            continue;
        }

        let mut segment = Vec::new();
        for child in Vec::<Element>::from(container) {
            if dash_run_len(&child) == Some(4) {
                if !segment.is_empty() {
                    output.push(strike_shell().build(mem::take(&mut segment)));
                }
            } else {
                segment.push(child);
            }
        }
        if !segment.is_empty() {
            output.push(strike_shell().build(segment));
        }
        changed = true;
    }

    *elements = output;
    changed
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
        let Some(run_len) = dash_run_len(&element) else {
            output.push(element);
            continue;
        };
        output.extend(wikidot_dash_run_elements(run_len));
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

fn bold_shell<'t>() -> FormatShell<'t> {
    FormatShell::Container {
        kind: FormatKind::Bold,
        ctype: ContainerType::Bold,
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
            | PartialElement::InlineSpanClose(_) => {}
        },
        _ => {}
    }
}
