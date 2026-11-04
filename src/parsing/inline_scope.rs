use crate::tree::{
    AttributeMap, Container, ContainerType, Element, ListItem, PartialElement,
};
use std::collections::BTreeSet;
use std::mem;

const MAX_ACTIVE_INLINE_SCOPES: usize = 101;
pub(super) const WIKIDOT_SCORE_SPAN_UNWRAPPED_ATTRIBUTE: &str =
    "data-ftml-score-span-own-line";

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    Size,
    Span,
}

struct PendingScope {
    ordinal: usize,
    accepted: bool,
}

struct ActiveScope<'t> {
    ctype: ContainerType,
    attributes: AttributeMap<'t>,
    scored: bool,
    has_content: bool,
    previous_same_kind: Option<usize>,
    previous_active: Option<usize>,
    next_active: Option<usize>,
}

#[derive(Default)]
struct ActiveScopes<'t> {
    scopes: Vec<ActiveScope<'t>>,
    top_size: Option<usize>,
    top_span: Option<usize>,
    active_tail: Option<usize>,
}

struct ActiveScopeRevIter<'a, 't> {
    scopes: &'a [ActiveScope<'t>],
    next: Option<usize>,
}

impl<'a, 't> Iterator for ActiveScopeRevIter<'a, 't> {
    type Item = &'a ActiveScope<'t>;

    fn next(&mut self) -> Option<Self::Item> {
        let position = self.next?;
        let scope = &self.scopes[position];
        self.next = scope.previous_active;
        Some(scope)
    }
}

impl<'t> ActiveScopes<'t> {
    fn push(
        &mut self,
        kind: ScopeKind,
        ctype: ContainerType,
        attributes: AttributeMap<'t>,
        scored: bool,
    ) {
        let position = self.scopes.len();
        let previous_same_kind = self.top_mut(kind).replace(position);
        let previous_active = self.active_tail.replace(position);
        self.scopes.push(ActiveScope {
            ctype,
            attributes,
            scored,
            has_content: false,
            previous_same_kind,
            previous_active,
            next_active: None,
        });
        if let Some(previous_active) = previous_active {
            self.scopes[previous_active].next_active = Some(position);
        }
    }

    fn remove(&mut self, kind: ScopeKind) {
        if let Some(position) = *self.top(kind) {
            let scope = &self.scopes[position];
            let previous_same_kind = scope.previous_same_kind;
            let previous_active = scope.previous_active;
            let next_active = scope.next_active;

            if let Some(previous_active) = previous_active {
                self.scopes[previous_active].next_active = next_active;
            }
            if let Some(next_active) = next_active {
                self.scopes[next_active].previous_active = previous_active;
            } else {
                self.active_tail = previous_active;
            }
            *self.top_mut(kind) = previous_same_kind;
        }
    }

    fn iter_active_rev(&self) -> ActiveScopeRevIter<'_, 't> {
        ActiveScopeRevIter {
            scopes: &self.scopes,
            next: self.active_tail,
        }
    }

    fn top_is_scored(&self, kind: ScopeKind) -> bool {
        self.top(kind)
            .is_some_and(|position| self.scopes[position].scored)
    }

    fn top_has_content(&self, kind: ScopeKind) -> bool {
        self.top(kind)
            .is_some_and(|position| self.scopes[position].has_content)
    }

    fn mark_active_content(&mut self) {
        let mut position = self.active_tail;
        while let Some(current) = position {
            self.scopes[current].has_content = true;
            position = self.scopes[current].previous_active;
        }
    }

    fn has_active_scope(&self) -> bool {
        self.active_tail.is_some()
    }

    fn outer_active_position(&self) -> Option<usize> {
        let mut position = self.active_tail?;
        while let Some(previous) = self.scopes[position].previous_active {
            position = previous;
        }
        Some(position)
    }

    fn has_scored_scope(&self) -> bool {
        self.iter_active_rev().any(|scope| scope.scored)
    }

    fn wikidot_label_spans(&self) -> Vec<AttributeMap<'t>> {
        let mut spans = self
            .iter_active_rev()
            .map(|scope| scope.attributes.clone())
            .collect::<Vec<_>>();
        spans.reverse();
        spans
    }

    fn scope_wrappers(&self) -> Vec<(ContainerType, AttributeMap<'t>)> {
        self.iter_active_rev()
            .map(|scope| (scope.ctype, scope.attributes.clone()))
            .collect()
    }

    fn active_scope_count(&self) -> usize {
        self.iter_active_rev().count()
    }

    fn top(&self, kind: ScopeKind) -> &Option<usize> {
        match kind {
            ScopeKind::Size => &self.top_size,
            ScopeKind::Span => &self.top_span,
        }
    }

    fn top_mut(&mut self, kind: ScopeKind) -> &mut Option<usize> {
        match kind {
            ScopeKind::Size => &mut self.top_size,
            ScopeKind::Span => &mut self.top_span,
        }
    }
}

pub(crate) fn lower_wikidot_inline_size_scopes<'t>(elements: &mut Vec<Element<'t>>) {
    let valid = collect_valid_scope_pairs(elements);

    let mut ordinal = 0;
    let mut active = ActiveScopes::default();
    let mut trim_next_break = false;
    lower_root_sequence(
        elements,
        &valid,
        &mut ordinal,
        &mut active,
        &mut trim_next_break,
    );
}

pub(crate) fn lower_wikidot_inline_size_scopes_inline<'t>(
    elements: &mut Vec<Element<'t>>,
) {
    let valid = collect_valid_scope_pairs(elements);

    let mut ordinal = 0;
    let mut active = ActiveScopes::default();
    let mut trim_next_break = false;
    lower_sequence(
        elements,
        &valid,
        &mut ordinal,
        &mut active,
        &mut trim_next_break,
        false,
    );
}

fn collect_valid_scope_pairs(elements: &[Element<'_>]) -> BTreeSet<usize> {
    let mut ordinal = 0;
    let mut open_sizes = Vec::new();
    let mut open_spans = Vec::new();
    let mut active_count = 0;
    let mut valid = BTreeSet::new();
    collect_valid_pairs(
        elements,
        &mut ordinal,
        &mut open_sizes,
        &mut open_spans,
        &mut active_count,
        &mut valid,
    );

    valid
}

#[derive(Default)]
struct SequenceEffects {
    scored_span_opened_at_start: bool,
    scored_span_opened_unwrapped_at_start: bool,
    scored_span_closed: bool,
}

fn lower_root_sequence<'t>(
    elements: &mut Vec<Element<'t>>,
    valid: &BTreeSet<usize>,
    ordinal: &mut usize,
    active: &mut ActiveScopes<'t>,
    trim_next_break: &mut bool,
) {
    let mut output = Vec::with_capacity(elements.len());
    let mut paragraph_group: Option<(bool, AttributeMap<'t>, Vec<Element<'t>>)> = None;
    let mut previous_scored_close = false;

    for mut element in mem::take(elements) {
        if let Element::Container(container) = &mut element
            && container.ctype() == ContainerType::Paragraph
        {
            let paragraph_attributes = container.attributes().clone();
            let starts_in_scored_span = active.has_scored_scope();
            let starts_in_inline_scope = active.has_active_scope();
            let reopens_paragraph_after_separator = starts_in_inline_scope
                && output.last().is_some_and(|element| {
                    matches!(element, Element::HorizontalRule)
                        || is_wikidot_content_separator(element)
                });
            if starts_in_inline_scope
                && !reopens_paragraph_after_separator
                && !output.is_empty()
                && paragraph_group.is_none()
            {
                container.elements_mut().insert(0, Element::LineBreak);
            }
            let continued_from_paragraph = paragraph_group.is_some();
            let effects = lower_sequence(
                container.elements_mut(),
                valid,
                ordinal,
                active,
                trim_next_break,
                false,
            );
            let joins_previous =
                previous_scored_close || effects.scored_span_opened_at_start;

            if !joins_previous {
                flush_paragraph_group(&mut output, &mut paragraph_group);
            }
            let (_, _, group) = paragraph_group.get_or_insert_with(|| {
                (
                    reopens_paragraph_after_separator
                        || !effects.scored_span_opened_unwrapped_at_start
                            && !starts_in_scored_span
                            && (!starts_in_inline_scope || continued_from_paragraph),
                    paragraph_attributes,
                    Vec::new(),
                )
            });
            group.append(container.elements_mut());
            previous_scored_close = effects.scored_span_closed;
            continue;
        }

        flush_paragraph_group(&mut output, &mut paragraph_group);
        let scope_before = active.outer_active_position();
        let mut sequence = vec![element];
        lower_sequence(&mut sequence, valid, ordinal, active, trim_next_break, true);
        let scope_continues =
            scope_before.is_some() && scope_before == active.outer_active_position();
        append_root_sequence(&mut output, &mut sequence, scope_continues);
        previous_scored_close = false;
    }

    flush_paragraph_group(&mut output, &mut paragraph_group);
    *elements = output;
}

fn is_wikidot_content_separator(element: &Element<'_>) -> bool {
    element.is_content_separator()
}

fn append_root_sequence<'t>(
    output: &mut Vec<Element<'t>>,
    sequence: &mut Vec<Element<'t>>,
    scope_continues: bool,
) {
    if scope_continues
        && let (Some(Element::Container(previous)), Some(Element::Container(next))) =
            (output.last_mut(), sequence.first_mut())
        && matches!(previous.ctype(), ContainerType::Span | ContainerType::Size)
        && previous.ctype() == next.ctype()
        && previous.attributes() == next.attributes()
    {
        previous.elements_mut().append(next.elements_mut());
        sequence.remove(0);
    }
    output.append(sequence);
}

fn flush_paragraph_group<'t>(
    output: &mut Vec<Element<'t>>,
    paragraph_group: &mut Option<(bool, AttributeMap<'t>, Vec<Element<'t>>)>,
) {
    let Some((wrapped, attributes, elements)) = paragraph_group.take() else {
        return;
    };
    if elements.is_empty() {
        return;
    }
    if wrapped {
        output.push(Element::Container(Container::new(
            ContainerType::Paragraph,
            elements,
            attributes,
        )));
    } else {
        output.extend(elements);
    }
}

fn collect_valid_pairs(
    elements: &[Element<'_>],
    ordinal: &mut usize,
    open_sizes: &mut Vec<PendingScope>,
    open_spans: &mut Vec<PendingScope>,
    active_count: &mut usize,
    valid: &mut BTreeSet<usize>,
) {
    for element in elements {
        match element {
            Element::Partial(PartialElement::InlineSizeOpen(_)) => {
                push_pending_scope(open_sizes, ordinal, active_count);
            }
            Element::Partial(PartialElement::InlineSizeClose(_)) => {
                let close = *ordinal;
                *ordinal += 1;
                if let Some(open) = open_sizes.pop() {
                    accept_pending_pair(open, close, active_count, valid);
                }
            }
            Element::Partial(PartialElement::InlineSpanOpen(_)) => {
                push_pending_scope(open_spans, ordinal, active_count);
            }
            Element::Partial(PartialElement::InlineSpanClose(_)) => {
                let close = *ordinal;
                *ordinal += 1;
                if let Some(open) = open_spans.pop() {
                    accept_pending_pair(open, close, active_count, valid);
                }
            }
            _ => {
                let mut visit = |children: &[Element<'_>]| {
                    collect_valid_pairs(
                        children,
                        ordinal,
                        open_sizes,
                        open_spans,
                        active_count,
                        valid,
                    )
                };
                visit_children(element, &mut visit);
            }
        }
    }
}

fn push_pending_scope(
    stack: &mut Vec<PendingScope>,
    ordinal: &mut usize,
    active_count: &mut usize,
) {
    let accepted = *active_count < MAX_ACTIVE_INLINE_SCOPES;
    if accepted {
        *active_count += 1;
    }
    stack.push(PendingScope {
        ordinal: *ordinal,
        accepted,
    });
    *ordinal += 1;
}

fn accept_pending_pair(
    open: PendingScope,
    close: usize,
    active_count: &mut usize,
    valid: &mut BTreeSet<usize>,
) {
    if open.accepted {
        *active_count -= 1;
        valid.insert(open.ordinal);
        valid.insert(close);
    }
}

fn lower_sequence<'t>(
    elements: &mut Vec<Element<'t>>,
    valid: &BTreeSet<usize>,
    ordinal: &mut usize,
    active: &mut ActiveScopes<'t>,
    trim_next_break: &mut bool,
    root_structural_boundary: bool,
) -> SequenceEffects {
    let mut output = Vec::with_capacity(elements.len());
    let mut run = Vec::new();
    let mut effects = SequenceEffects::default();
    let mut last_run_outer_scope = None;
    let mut preserve_space_before_footnote = false;

    let mut source = mem::take(elements).into_iter().peekable();
    while let Some(mut element) = source.next() {
        if *trim_next_break {
            if matches!(element, Element::LineBreak | Element::LineBreaks(_)) {
                continue;
            }
            *trim_next_break = false;
        }
        if matches!(
            element,
            Element::Partial(PartialElement::WikidotEmptyInlineOwner)
        ) {
            preserve_space_before_footnote =
                matches!(source.peek(), Some(Element::Footnote(_)))
                    && sequence_starts_with_repeated_list_marker(&output, &run)
                    && sequence_ends_with_space(&output, &run);
            continue;
        }
        if let Element::Partial(PartialElement::InlineSizeOpen(style)) = element {
            flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
            let current = *ordinal;
            *ordinal += 1;
            if valid.contains(&current) {
                let leading_space =
                    matches!(source.peek(), Some(Element::Text(text)) if text == " ");
                if leading_space {
                    source.next();
                    if !elements_end_with_space(&output) {
                        run.push(text!(" "));
                        flush_run(
                            &mut output,
                            &mut run,
                            active,
                            &mut last_run_outer_scope,
                        );
                    }
                }
                let mut attributes = AttributeMap::new();
                attributes.insert("style", style);
                active.push(ScopeKind::Size, ContainerType::Size, attributes, false);
            }
            continue;
        }
        if let Element::Partial(PartialElement::InlineSizeClose(close_source)) = element {
            let empty = !active.top_has_content(ScopeKind::Size) && run.is_empty();
            let trailing_space =
                matches!(run.last(), Some(Element::Text(text)) if text == " ");
            let next_starts_with_space = matches!(
                source.peek(),
                Some(Element::Text(text)) if text.starts_with(' ')
            );
            let current = *ordinal;
            *ordinal += 1;
            if valid.contains(&current) {
                if trailing_space {
                    run.pop();
                }
                flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
                trim_trailing_space(&mut output);
                active.remove(ScopeKind::Size);
                if trailing_space
                    && !next_starts_with_space
                    && !empty
                    && source.peek().is_some()
                {
                    run.push(text!(" "));
                }
            } else {
                flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
                output.push(Element::Text(close_source));
            }
            continue;
        }
        if matches!(element, Element::Footnote(_)) {
            if matches!(run.last(), Some(Element::LineBreak))
                && sequence_starts_with_repeated_list_marker(&output, &run)
            {
                run.pop();
            }
            if preserve_space_before_footnote {
                preserve_space_before_footnote = false;
            } else {
                trim_one_trailing_text_space(&mut run);
            }
        }
        if active.has_active_scope()
            && let Element::Collapsible {
                wikidot_label_spans,
                ..
            } = &mut element
        {
            *wikidot_label_spans = active.wikidot_label_spans();
        }
        if let Element::Partial(PartialElement::InlineSpanOpen(mut attributes)) = element
        {
            let at_start = output.is_empty() && run.is_empty();
            let scored = attributes.remove("data-ftml-score-span").is_some();
            let scored_starts_next_physical_line = attributes
                .remove(WIKIDOT_SCORE_SPAN_UNWRAPPED_ATTRIBUTE)
                .is_some();
            if scored {
                while matches!(
                    run.last(),
                    Some(Element::LineBreak | Element::LineBreaks(_))
                ) {
                    run.pop();
                }
            }
            flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
            let current = *ordinal;
            *ordinal += 1;
            if valid.contains(&current) {
                let leading_space =
                    matches!(source.peek(), Some(Element::Text(text)) if text == " ");
                if leading_space {
                    source.next();
                    if span_leading_space_hoists(at_start, active, &output) {
                        run.push(text!(" "));
                        flush_run(
                            &mut output,
                            &mut run,
                            active,
                            &mut last_run_outer_scope,
                        );
                    }
                }
                active.push(ScopeKind::Span, ContainerType::Span, attributes, scored);
                if scored {
                    effects.scored_span_opened_at_start |= at_start;
                    effects.scored_span_opened_unwrapped_at_start |=
                        at_start && scored_starts_next_physical_line;
                    *trim_next_break = true;
                }
            }
            continue;
        }
        if let Element::Partial(PartialElement::InlineSpanClose(close_source)) = element {
            let scored = active.top_is_scored(ScopeKind::Span);
            let empty = !active.top_has_content(ScopeKind::Span) && run.is_empty();
            let trailing_space =
                !scored && matches!(run.last(), Some(Element::Text(text)) if text == " ");
            let next_starts_with_space = matches!(
                source.peek(),
                Some(Element::Text(text)) if text.starts_with(' ')
            );
            if scored {
                while matches!(
                    run.last(),
                    Some(Element::LineBreak | Element::LineBreaks(_))
                ) {
                    run.pop();
                }
            } else if trailing_space && next_starts_with_space {
                run.pop();
            }
            flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
            let preserve_empty_owner_space = empty
                && !scored
                && matches!(source.peek(), Some(Element::Footnote(_)))
                && sequence_starts_with_repeated_list_marker(&output, &run)
                && sequence_ends_with_space(&output, &run);
            if empty && !scored && !preserve_empty_owner_space {
                trim_trailing_space(&mut output);
            }
            let current = *ordinal;
            *ordinal += 1;
            if valid.contains(&current) {
                if !(preserve_empty_owner_space || empty && scored) {
                    trim_trailing_space(&mut output);
                }
                active.remove(ScopeKind::Span);
                if trailing_space
                    && !next_starts_with_space
                    && !empty
                    && !scored
                    && source.peek().is_some()
                {
                    output.push(text!(" "));
                }
                if scored {
                    effects.scored_span_closed = true;
                    *trim_next_break = true;
                }
                preserve_space_before_footnote |= preserve_empty_owner_space;
            } else {
                output.push(Element::Text(close_source));
            }
            continue;
        }
        let list_scope_wrappers = if root_structural_boundary
            && active.has_active_scope()
            && matches!(&element, Element::List { .. })
        {
            Some(active.scope_wrappers())
        } else {
            None
        };
        if active.has_active_scope()
            && element.paragraph_safe()
            && contains_inline_scope_control(&element)
            && inline_scope_controls_are_self_contained(&element)
        {
            let mut nested_active = ActiveScopes::default();
            if lower_children(
                &mut element,
                valid,
                ordinal,
                &mut nested_active,
                trim_next_break,
            ) {
                debug_assert!(!nested_active.has_active_scope());
                run.push(element);
            } else {
                flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
                output.push(element);
                last_run_outer_scope = None;
            }
        } else if !matches!(element, Element::Partial(_))
            && element.paragraph_safe()
            && !contains_inline_scope_control(&element)
        {
            if matches!(element, Element::LineBreak | Element::LineBreaks(_))
                && matches!(run.last(), Some(Element::Text(text)) if text == " ")
            {
                run.pop();
            }
            run.push(element);
        } else if lower_children(&mut element, valid, ordinal, active, trim_next_break) {
            flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
            if let Some(scopes) = list_scope_wrappers {
                let active_after = active.active_scope_count();
                if active_after == scopes.len() {
                    let wrapped = wrap_scope_elements(&scopes, vec![Element::LineBreak]);
                    output.push(Element::Container(Container::new(
                        ContainerType::Paragraph,
                        wrapped,
                        AttributeMap::new(),
                    )));
                    // The physical opener-line break is moved before the list
                    // by Wikidot; do not also retain it after the list.
                    *trim_next_break = true;
                    last_run_outer_scope = None;
                } else if active_after == 0 {
                    // When the inline owner closes from inside the list, the
                    // same opener-line break is unwrapped but still belongs to
                    // the inline scope.
                    output.extend(wrap_scope_elements(&scopes, vec![Element::LineBreak]));
                    last_run_outer_scope = None;
                }
            }
            if !is_empty_paragraph(&element) {
                output.push(element);
                last_run_outer_scope = None;
            }
        } else {
            flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
            output.push(element);
            last_run_outer_scope = None;
        }
    }
    flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
    *elements = output;
    effects
}

fn wrap_scope_elements<'t>(
    scopes: &[(ContainerType, AttributeMap<'t>)],
    mut elements: Vec<Element<'t>>,
) -> Vec<Element<'t>> {
    for (ctype, attributes) in scopes {
        elements = vec![Element::Container(Container::new(
            *ctype,
            elements,
            attributes.clone(),
        ))];
    }
    elements
}

fn run_merges_across_separator(
    output: &[Element<'_>],
    next: &Container<'_>,
    outer_scope: Option<usize>,
    last_run_outer_scope: Option<usize>,
) -> bool {
    if !matches!(output.last(), Some(Element::Text(text)) if text == " ") {
        return false;
    }
    let Some(Element::Container(previous)) = output
        .len()
        .checked_sub(2)
        .and_then(|index| output.get(index))
    else {
        return false;
    };
    matches!(previous.ctype(), ContainerType::Span | ContainerType::Size)
        && outer_scope.is_some()
        && outer_scope == last_run_outer_scope
        && previous.ctype() == next.ctype()
        && previous.attributes() == next.attributes()
}

fn flush_run<'t>(
    output: &mut Vec<Element<'t>>,
    run: &mut Vec<Element<'t>>,
    active: &mut ActiveScopes<'t>,
    last_run_outer_scope: &mut Option<usize>,
) {
    if run.is_empty() {
        return;
    }
    let outer_scope = active.outer_active_position();
    active.mark_active_content();
    let mut wrapped = mem::take(run);
    for scope in active.iter_active_rev() {
        wrapped = vec![Element::Container(Container::new(
            scope.ctype,
            wrapped,
            scope.attributes.clone(),
        ))];
    }
    let [Element::Container(next)] = wrapped.as_mut_slice() else {
        output.extend(wrapped);
        *last_run_outer_scope = None;
        return;
    };
    let merge_across_separator =
        run_merges_across_separator(output, next, outer_scope, *last_run_outer_scope);
    if merge_across_separator {
        let Some(Element::Text(separator)) = output.pop() else {
            unreachable!("separator space checked above");
        };
        let Some(Element::Container(previous)) = output.last_mut() else {
            unreachable!("merge-compatible container checked above");
        };
        let next = wrapped.pop().expect("wrapped scope container");
        let Element::Container(next) = next else {
            unreachable!("checked wrapped scope container");
        };
        previous.elements_mut().push(Element::Text(separator));
        previous.elements_mut().extend(Vec::from(next));
        *last_run_outer_scope = outer_scope;
        return;
    }
    let Some(Element::Container(previous)) = output.last_mut() else {
        output.extend(wrapped);
        *last_run_outer_scope = outer_scope;
        return;
    };
    let inline_scope =
        matches!(previous.ctype(), ContainerType::Span | ContainerType::Size);
    if inline_scope
        && outer_scope.is_some()
        && outer_scope == *last_run_outer_scope
        && previous.ctype() == next.ctype()
        && previous.attributes() == next.attributes()
    {
        let next = wrapped.pop().expect("wrapped scope container");
        let Element::Container(next) = next else {
            unreachable!("checked wrapped scope container");
        };
        previous.elements_mut().extend(Vec::from(next));
    } else {
        output.extend(wrapped);
    }
    *last_run_outer_scope = outer_scope;
}

fn trim_trailing_space(elements: &mut Vec<Element<'_>>) {
    let Some(last) = elements.last_mut() else {
        return;
    };
    if let Element::Container(container) = last
        && matches!(container.ctype(), ContainerType::Span | ContainerType::Size)
    {
        trim_trailing_space(container.elements_mut());
        if container.elements().is_empty() {
            elements.pop();
        }
        return;
    }
    if matches!(last, Element::Text(text) if text == " ") {
        elements.pop();
    }
}

fn trim_one_trailing_text_space(elements: &mut Vec<Element<'_>>) {
    let Some(Element::Text(text)) = elements.last_mut() else {
        return;
    };
    if text.ends_with(' ') {
        text.to_mut().pop();
        if text.is_empty() {
            elements.pop();
        }
    }
}

fn sequence_starts_with_repeated_list_marker(
    output: &[Element<'_>],
    run: &[Element<'_>],
) -> bool {
    output
        .iter()
        .chain(run)
        .next()
        .is_some_and(|element| {
            matches!(element, Element::Text(text) if matches!(text.as_ref(), "**" | "##"))
        })
}

fn sequence_ends_with_space(output: &[Element<'_>], run: &[Element<'_>]) -> bool {
    run.last().or_else(|| output.last()).is_some_and(
        |element| matches!(element, Element::Text(text) if text.ends_with(' ')),
    )
}

fn span_leading_space_hoists(
    at_start: bool,
    active: &ActiveScopes<'_>,
    output: &[Element<'_>],
) -> bool {
    !at_start && !active.has_active_scope() && !elements_end_with_space(output)
}

fn elements_end_with_space(elements: &[Element<'_>]) -> bool {
    match elements.last() {
        Some(Element::Text(text)) => text.ends_with(' '),
        Some(Element::Container(container)) => {
            elements_end_with_space(container.elements())
        }
        _ => false,
    }
}

fn is_empty_paragraph(element: &Element<'_>) -> bool {
    matches!(
        element,
        Element::Container(container)
            if container.ctype() == ContainerType::Paragraph && container.elements().is_empty()
    )
}

fn contains_inline_scope_control(element: &Element<'_>) -> bool {
    if matches!(
        element,
        Element::Partial(
            PartialElement::InlineSizeOpen(_)
                | PartialElement::InlineSizeClose(_)
                | PartialElement::InlineSpanOpen(_)
                | PartialElement::InlineSpanClose(_)
        )
    ) {
        return true;
    }
    let mut contains = false;
    visit_children(element, &mut |children| {
        contains |= children.iter().any(contains_inline_scope_control);
    });
    contains
}

fn inline_scope_controls_are_self_contained(element: &Element<'_>) -> bool {
    fn visit(element: &Element<'_>, sizes: &mut usize, spans: &mut usize) -> bool {
        match element {
            Element::Partial(PartialElement::InlineSizeOpen(_)) => {
                *sizes += 1;
                true
            }
            Element::Partial(PartialElement::InlineSizeClose(_)) => {
                if *sizes == 0 {
                    false
                } else {
                    *sizes -= 1;
                    true
                }
            }
            Element::Partial(PartialElement::InlineSpanOpen(_)) => {
                *spans += 1;
                true
            }
            Element::Partial(PartialElement::InlineSpanClose(_)) => {
                if *spans == 0 {
                    false
                } else {
                    *spans -= 1;
                    true
                }
            }
            _ => {
                let mut valid = true;
                visit_children(element, &mut |children| {
                    for child in children {
                        valid &= visit(child, sizes, spans);
                    }
                });
                valid
            }
        }
    }

    let mut sizes = 0;
    let mut spans = 0;
    visit(element, &mut sizes, &mut spans) && sizes == 0 && spans == 0
}

fn lower_children<'t>(
    element: &mut Element<'t>,
    valid: &BTreeSet<usize>,
    ordinal: &mut usize,
    active: &mut ActiveScopes<'t>,
    trim_next_break: &mut bool,
) -> bool {
    if let Element::Container(container) = element
        && matches!(container.ctype(), ContainerType::Header(_))
    {
        let scope_crosses_heading = active.has_active_scope();
        lower_sequence(
            container.elements_mut(),
            valid,
            ordinal,
            active,
            trim_next_break,
            false,
        );
        if scope_crosses_heading {
            insert_wikidot_heading_label_span(container.elements_mut());
            *trim_next_break = true;
        }
        return true;
    }

    let mut lowered = false;
    let mut visit = |children: &mut Vec<Element<'t>>| {
        lowered = true;
        lower_sequence(children, valid, ordinal, active, trim_next_break, false);
    };
    visit_children_mut(element, &mut visit);
    lowered
}

fn insert_wikidot_heading_label_span(elements: &mut Vec<Element<'_>>) {
    if let [Element::Container(container)] = elements.as_mut_slice()
        && matches!(container.ctype(), ContainerType::Span | ContainerType::Size)
    {
        insert_wikidot_heading_label_span(container.elements_mut());
        return;
    }
    let label = mem::take(elements);
    elements.push(Element::Container(Container::new(
        ContainerType::HeadingLabel,
        label,
        AttributeMap::new(),
    )));
}

fn visit_children<'t>(element: &Element<'t>, visit: &mut dyn FnMut(&[Element<'t>])) {
    match element {
        Element::Container(container) => visit(container.elements()),
        Element::Table(table) => {
            for row in &table.rows {
                for cell in &row.cells {
                    visit(&cell.elements);
                }
            }
        }
        Element::TabView(tabs) => {
            for tab in tabs {
                visit(&tab.elements);
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
                    ListItem::SubList { element } => visit_children(element, visit),
                }
            }
        }
        Element::DefinitionList(items) => {
            for item in items {
                visit(&item.key_elements);
                visit(&item.value_elements);
            }
        }
        Element::Partial(partial) => match partial {
            PartialElement::ListItem(ListItem::Elements { elements, .. }) => {
                visit(elements)
            }
            PartialElement::ListItem(ListItem::SubList { element }) => {
                visit_children(element, visit)
            }
            PartialElement::TableRow(row) => {
                for cell in &row.cells {
                    visit(&cell.elements);
                }
            }
            PartialElement::TableCell(cell) => visit(&cell.elements),
            PartialElement::Tab(tab) => visit(&tab.elements),
            PartialElement::RubyText(ruby_text) => visit(&ruby_text.elements),
            PartialElement::WikidotEmptyInlineOwner
            | PartialElement::InlineSizeOpen(_)
            | PartialElement::InlineSizeClose(_)
            | PartialElement::InlineSpanOpen(_)
            | PartialElement::InlineSpanClose(_) => {}
        },
        _ => {}
    }
}

fn visit_children_mut<'t>(
    element: &mut Element<'t>,
    visit: &mut dyn FnMut(&mut Vec<Element<'t>>),
) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};
    use std::time::{Duration, Instant};

    fn render_wikidot(source: &str) -> String {
        use crate::render::{Render, html::HtmlRender};

        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = source.to_owned();
        crate::preprocess_for_layout(&mut source, settings.layout);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:#?}");
        HtmlRender.render(&tree, &page_info, &settings).body
    }

    #[test]
    fn wikidot_inline_scopes_reopen_in_paragraphs_after_separators() {
        for (source, expected) in [
            (
                "[[size 120%]]A\n====\nB[[/size]]",
                concat!(
                    r#"<p><span style="font-size:120%;">A</span></p>"#,
                    r#"<div class="content-separator" style="display: none:"></div>"#,
                    r#"<p><span style="font-size:120%;">B</span></p>"#,
                ),
            ),
            (
                "[[size 120%]]A\n----\nB[[/size]]",
                concat!(
                    r#"<p><span style="font-size:120%;">A</span></p>"#,
                    "<hr>",
                    r#"<p><span style="font-size:120%;">B</span></p>"#,
                ),
            ),
            (
                "[[span]]A\n====\nB[[/span]]",
                concat!(
                    "<p><span>A</span></p>",
                    r#"<div class="content-separator" style="display: none:"></div>"#,
                    "<p><span>B</span></p>",
                ),
            ),
            (
                "[[span]]A\n----\nB[[/span]]",
                "<p><span>A</span></p><hr><p><span>B</span></p>",
            ),
        ] {
            assert_eq!(render_wikidot(source), expected, "{source:?}");
        }
    }

    #[test]
    fn wikidot_nested_span_edge_spaces_keep_outer_span_whole() {
        // ListPages set-record pattern: the inner spans' edge spaces must not
        // split the outer span into sibling copies (ftml #758 follow-up), and
        // a nested inner span's leading space is discarded (legacy behavior),
        // so no space lands between the outer open tag and the inner span
        // (wikidot.py set-record contract). The trailing separator space is
        // absorbed between the inner spans inside the single outer span.
        for (source, expected) in [
            (
                "[[span class=\"set category\"]][[span class=\"name\"]] category [[/span]][[span class=\"value\"]] _default [[/span]][[/span]]",
                concat!(
                    "<p>",
                    "<span class=\"set category\">",
                    "<span class=\"name\">category</span>",
                    " ",
                    "<span class=\"value\">_default</span>",
                    "</span>",
                    "</p>",
                ),
            ),
            (
                "Record:[[span class=\"set category\"]][[span class=\"name\"]] category [[/span]][[span class=\"value\"]] _default [[/span]][[/span]]",
                concat!(
                    "<p>",
                    "Record:",
                    "<span class=\"set category\">",
                    "<span class=\"name\">category</span>",
                    " ",
                    "<span class=\"value\">_default</span>",
                    "</span>",
                    "</p>",
                ),
            ),
            (
                "Record:[[span class=\"set comments\"]][[span class=\"name\"]] comments [[/span]][[span class=\"value\"]] 0 [[/span]][[/span]]",
                concat!(
                    "<p>",
                    "Record:",
                    "<span class=\"set comments\">",
                    "<span class=\"name\">comments</span>",
                    " ",
                    "<span class=\"value\">0</span>",
                    "</span>",
                    "</p>",
                ),
            ),
        ] {
            assert_eq!(render_wikidot(source), expected, "{source:?}");
        }
    }

    #[test]
    fn repeated_separator_scope_restarts_stay_bounded() {
        let unit = "[[size 120%]]A\n====\nB[[/size]]\n";
        let source = unit.repeat(1_024);
        let started = Instant::now();
        let html = render_wikidot(&source);
        let elapsed = started.elapsed();

        assert_eq!(html.matches("content-separator").count(), 1_024, "{html}");
        assert_eq!(html.matches("font-size:120%").count(), 2_048, "{html}");
        assert!(
            elapsed < Duration::from_secs(3),
            "separator scope lowering took {elapsed:?}",
        );
    }

    #[test]
    fn lowering_traverses_partial_list_items_without_paragraph_safety_checks() {
        let mut elements = vec![Element::Partial(PartialElement::ListItem(
            ListItem::Elements {
                attributes: AttributeMap::new(),
                elements: vec![
                    Element::Partial(PartialElement::InlineSizeOpen(cow!("170%"))),
                    text!("partial body"),
                    Element::Partial(PartialElement::InlineSizeClose(cow!("[[/size]]"))),
                ],
            },
        ))];

        lower_wikidot_inline_size_scopes(&mut elements);

        let Element::Partial(PartialElement::ListItem(ListItem::Elements {
            elements: nested,
            ..
        })) = &elements[0]
        else {
            panic!("partial list item was not preserved: {elements:#?}");
        };
        let Element::Container(container) = &nested[0] else {
            panic!("inline size scope was not lowered: {nested:#?}");
        };
        assert_eq!(container.ctype(), ContainerType::Size);
        assert_eq!(container.elements(), &[text!("partial body")]);
    }

    #[test]
    fn legacy_parse_reports_malformed_list_cell_without_panicking() {
        let source = "[[size 170%]]heading[[/size]]\n* [[/cell]]\n";
        let tokens = crate::tokenize(source);
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        let (tree, errors) = crate::parse(&tokens, &page_info, &settings).into();

        assert!(!tree.elements.is_empty());
        assert!(!errors.is_empty());
        assert!(
            format!("{errors:#?}").contains("NoRulesMatch"),
            "{errors:#?}",
        );
    }

    #[test]
    fn lowering_caps_active_inline_scope_depth() {
        let mut elements = Vec::new();
        for _ in 0..(MAX_ACTIVE_INLINE_SCOPES + 1) {
            elements.push(Element::Partial(PartialElement::InlineSizeOpen(cow!(
                "font-size: larger;"
            ))));
        }
        elements.push(text!("bounded"));
        for _ in 0..(MAX_ACTIVE_INLINE_SCOPES + 1) {
            elements.push(Element::Partial(PartialElement::InlineSizeClose(cow!(
                "[[/size]]"
            ))));
        }

        lower_wikidot_inline_size_scopes(&mut elements);

        assert_eq!(
            max_inline_scope_container_depth(&elements),
            MAX_ACTIVE_INLINE_SCOPES
        );
    }

    #[test]
    fn size_close_at_sequence_end_drops_trailing_space() {
        let mut elements = vec![Element::Container(Container::new(
            ContainerType::Paragraph,
            vec![
                Element::Partial(PartialElement::InlineSizeOpen(cow!("font-size:0%;"))),
                text!("literal"),
                text!(" "),
                Element::Partial(PartialElement::InlineSizeClose(cow!("[[/size]]"))),
            ],
            AttributeMap::new(),
        ))];

        lower_wikidot_inline_size_scopes(&mut elements);

        let [Element::Container(paragraph)] = elements.as_slice() else {
            panic!("expected one paragraph: {elements:#?}");
        };
        let [Element::Container(container)] = paragraph.elements() else {
            panic!("expected only a lowered size container: {paragraph:#?}");
        };
        assert_eq!(container.ctype(), ContainerType::Size);
        assert_eq!(container.elements(), &[text!("literal")]);
    }

    #[test]
    fn lowering_caps_mixed_inline_scope_depth() {
        let kinds = (0..(MAX_ACTIVE_INLINE_SCOPES + 2))
            .map(|index| {
                if index % 2 == 0 {
                    ScopeKind::Size
                } else {
                    ScopeKind::Span
                }
            })
            .collect::<Vec<_>>();
        let mut elements = Vec::new();
        for kind in &kinds {
            match kind {
                ScopeKind::Size => elements.push(Element::Partial(
                    PartialElement::InlineSizeOpen(cow!("font-size: larger;")),
                )),
                ScopeKind::Span => elements.push(Element::Partial(
                    PartialElement::InlineSpanOpen(AttributeMap::new()),
                )),
            }
        }
        elements.push(text!("mixed bounded"));
        for kind in kinds.iter().rev() {
            match kind {
                ScopeKind::Size => {
                    elements.push(Element::Partial(PartialElement::InlineSizeClose(
                        cow!("[[/size]]"),
                    )));
                }
                ScopeKind::Span => elements.push(Element::Partial(
                    PartialElement::InlineSpanClose(cow!("[[/span]]")),
                )),
            }
        }

        lower_wikidot_inline_size_scopes(&mut elements);

        assert_eq!(
            max_inline_scope_container_depth(&elements),
            MAX_ACTIVE_INLINE_SCOPES
        );
    }

    #[test]
    fn unmatched_over_limit_inline_scopes_fail_closed() {
        let mut elements = Vec::new();
        for _ in 0..(MAX_ACTIVE_INLINE_SCOPES + 1) {
            elements.push(Element::Partial(PartialElement::InlineSizeOpen(cow!(
                "font-size: larger;"
            ))));
        }
        elements.push(text!("unmatched"));

        lower_wikidot_inline_size_scopes(&mut elements);

        assert_eq!(elements, vec![text!("unmatched")]);
        assert_eq!(max_inline_scope_container_depth(&elements), 0);
    }

    fn max_inline_scope_container_depth(elements: &[Element<'_>]) -> usize {
        elements
            .iter()
            .map(|element| match element {
                Element::Container(container)
                    if matches!(
                        container.ctype(),
                        ContainerType::Size | ContainerType::Span
                    ) =>
                {
                    1 + max_inline_scope_container_depth(container.elements())
                }
                Element::Container(container) => {
                    max_inline_scope_container_depth(container.elements())
                }
                _ => 0,
            })
            .max()
            .unwrap_or(0)
    }
}
