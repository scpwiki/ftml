use crate::tree::{
    AttributeMap, Container, ContainerType, Element, InlineFormatKind, ListItem,
    PartialElement,
};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::mem;

const MAX_ACTIVE_INLINE_SCOPES: usize = 101;
pub(super) const WIKIDOT_SCORE_SPAN_UNWRAPPED_ATTRIBUTE: &str =
    "data-ftml-score-span-own-line";

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    Size,
    Span,
    Format(InlineFormatKind),
    Color,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InlineTag {
    Span,
    Strong,
    Emphasis,
    Monospace,
    Subscript,
    Superscript,
}

fn inline_tag_for_ctype(ctype: ContainerType) -> InlineTag {
    match ctype {
        ContainerType::Bold => InlineTag::Strong,
        ContainerType::Italics => InlineTag::Emphasis,
        ContainerType::Monospace => InlineTag::Monospace,
        ContainerType::Subscript => InlineTag::Subscript,
        ContainerType::Superscript => InlineTag::Superscript,
        _ => InlineTag::Span,
    }
}

#[derive(Clone)]
enum ScopeWrapper<'t> {
    Container {
        ctype: ContainerType,
        attributes: AttributeMap<'t>,
    },
    Color {
        color: Cow<'t, str>,
        background: bool,
    },
}

impl<'t> ScopeWrapper<'t> {
    fn tag(&self) -> InlineTag {
        match self {
            ScopeWrapper::Container { ctype, .. } => inline_tag_for_ctype(*ctype),
            ScopeWrapper::Color { .. } => InlineTag::Span,
        }
    }

    fn wrap(&self, elements: Vec<Element<'t>>) -> Element<'t> {
        match self {
            ScopeWrapper::Container { ctype, attributes } => {
                Element::Container(Container::new(*ctype, elements, attributes.clone()))
            }
            ScopeWrapper::Color { color, background } => Element::Color {
                color: color.clone(),
                background: *background,
                elements,
            },
        }
    }
}

struct PendingScope {
    ordinal: usize,
    accepted: bool,
}

struct ActiveScope<'t> {
    wrapper: ScopeWrapper<'t>,
    kind: ScopeKind,
    scored: bool,
    has_content: bool,
    origin: usize,
    previous_active: Option<usize>,
    next_active: Option<usize>,
}

#[derive(Default)]
struct ActiveScopes<'t> {
    scopes: Vec<ActiveScope<'t>>,
    active_tail: Option<usize>,
    watches: Vec<(usize, Vec<InlineTag>)>,
    space_eject_floor: Option<Option<usize>>,
    close_pairs: BTreeMap<usize, usize>,
    open_instances: BTreeMap<usize, usize>,
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
    fn push(&mut self, kind: ScopeKind, wrapper: ScopeWrapper<'t>, scored: bool) {
        self.push_with_origin(kind, wrapper, scored, self.scopes.len());
    }

    fn push_with_origin(
        &mut self,
        kind: ScopeKind,
        wrapper: ScopeWrapper<'t>,
        scored: bool,
        origin: usize,
    ) {
        let position = self.scopes.len();
        let previous_active = self.active_tail.replace(position);
        self.scopes.push(ActiveScope {
            wrapper,
            kind,
            scored,
            has_content: false,
            origin,
            previous_active,
            next_active: None,
        });
        if let Some(previous_active) = previous_active {
            self.scopes[previous_active].next_active = Some(position);
        }
    }

    fn begin_watch(&mut self) -> usize {
        let watermark = self.scopes.len();
        self.watches.push((watermark, Vec::new()));
        watermark
    }

    fn end_watch(&mut self) -> Vec<InlineTag> {
        self.watches.pop().map(|(_, tags)| tags).unwrap_or_default()
    }

    fn unlink(&mut self, position: usize) {
        let below = self.scopes[position].previous_active;
        let above = self.scopes[position].next_active;
        if let Some(below) = below {
            self.scopes[below].next_active = above;
        }
        if let Some(above) = above {
            self.scopes[above].previous_active = below;
        }
        if self.active_tail == Some(position) {
            self.active_tail = below;
        }
        self.scopes[position].previous_active = None;
        self.scopes[position].next_active = None;
    }

    fn drop_block_boundary_scopes(&mut self, watermark: usize, completed: &[InlineTag]) {
        let mut snapshot = Vec::new();
        let mut position = self.active_tail;
        while let Some(current) = position {
            let scope = &self.scopes[current];
            snapshot.push((current, scope.origin, scope.wrapper.tag()));
            position = scope.previous_active;
        }
        let mut drops = Vec::new();
        for &(entry, origin, tag) in &snapshot {
            if origin >= watermark
                && snapshot
                    .iter()
                    .any(|&(other_entry, other_origin, other_tag)| {
                        other_entry != entry
                            && other_origin < watermark
                            && other_tag == tag
                    })
            {
                drops.push(entry);
            }
        }
        for &(entry, origin, tag) in &snapshot {
            if origin < watermark && completed.contains(&tag) {
                drops.push(entry);
            }
        }
        for entry in drops {
            self.unlink(entry);
        }
    }

    fn record_open_instance(&mut self, event_ordinal: usize) {
        self.open_instances
            .insert(event_ordinal, self.scopes.len() - 1);
    }

    fn lineage_is_active(&self, lineage: usize) -> bool {
        let mut position = self.active_tail;
        while let Some(current) = position {
            let scope = &self.scopes[current];
            if scope.origin == lineage {
                return true;
            }
            position = scope.previous_active;
        }
        false
    }

    fn resolve_close(&self, close_ordinal: usize, tag: InlineTag) -> Option<usize> {
        let orphaned = self.close_pairs.get(&close_ordinal).is_some_and(|open| {
            self.open_instances
                .get(open)
                .is_some_and(|lineage| !self.lineage_is_active(*lineage))
        });
        if orphaned {
            return self.active_tail;
        }
        self.peek_close(tag)
    }

    fn peek_close(&self, tag: InlineTag) -> Option<usize> {
        let mut position = self.active_tail;
        while let Some(current) = position {
            let scope = &self.scopes[current];
            if scope.wrapper.tag() == tag {
                return Some(current);
            }
            position = scope.previous_active;
        }
        None
    }

    fn scope_has_content(&self, position: usize) -> bool {
        self.scopes[position].has_content
    }

    fn scope_is_scored(&self, position: usize) -> bool {
        self.scopes[position].scored
    }

    fn commit_close(&mut self, position: usize) {
        let closed = &self.scopes[position];
        let closed_origin = closed.origin;
        let closed_tag = closed.wrapper.tag();
        for (watermark, tags) in &mut self.watches {
            if closed_origin >= *watermark && !tags.contains(&closed_tag) {
                tags.push(closed_tag);
            }
        }
        let mut above = Vec::new();
        let mut cursor = self.scopes[position].next_active;
        while let Some(current) = cursor {
            above.push(current);
            cursor = self.scopes[current].next_active;
        }
        let below = self.scopes[position].previous_active;
        if above.len() >= 2 {
            self.space_eject_floor = Some(Some(position));
            self.scopes[position].next_active = None;
            self.active_tail = Some(position);
            let mut order = Vec::with_capacity(above.len());
            let (last, rest) = above.split_last().expect("at least two scopes above");
            order.push(*last);
            order.extend_from_slice(rest);
            let mut kept: Vec<usize> = Vec::with_capacity(order.len());
            for (index, entry) in order.iter().enumerate() {
                let tag = self.scopes[*entry].wrapper.tag();
                let shadowed = order[index + 1..]
                    .iter()
                    .any(|later| self.scopes[*later].wrapper.tag() == tag);
                if !shadowed {
                    kept.push(*entry);
                }
            }
            for entry in kept {
                let scope = &self.scopes[entry];
                let wrapper = scope.wrapper.clone();
                let kind = scope.kind;
                let scored = scope.scored;
                let origin = scope.origin;
                self.push_with_origin(kind, wrapper, scored, origin);
            }
            return;
        }
        if above.len() == 1 {
            self.space_eject_floor = Some(below);
        }
        if let Some(below) = below {
            self.scopes[below].next_active = None;
        }
        self.active_tail = below;
        for entry in above {
            let scope = &self.scopes[entry];
            let wrapper = scope.wrapper.clone();
            let kind = scope.kind;
            let scored = scope.scored;
            let origin = scope.origin;
            self.push_with_origin(kind, wrapper, scored, origin);
        }
    }

    fn take_space_eject_floor(&mut self) -> Option<Option<usize>> {
        self.space_eject_floor.take()
    }

    fn iter_active_rev(&self) -> ActiveScopeRevIter<'_, 't> {
        ActiveScopeRevIter {
            scopes: &self.scopes,
            next: self.active_tail,
        }
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

    fn active_positions(&self) -> Vec<usize> {
        let mut positions = Vec::new();
        let mut current = self.outer_active_position();
        while let Some(position) = current {
            positions.push(position);
            current = self.scopes[position].next_active;
        }
        positions
    }

    fn has_scored_scope(&self) -> bool {
        self.iter_active_rev().any(|scope| scope.scored)
    }

    fn wikidot_label_spans(&self) -> Vec<AttributeMap<'t>> {
        let mut spans = self
            .iter_active_rev()
            .filter(|scope| matches!(scope.kind, ScopeKind::Size | ScopeKind::Span))
            .filter_map(|scope| match &scope.wrapper {
                ScopeWrapper::Container { attributes, .. } => Some(attributes.clone()),
                ScopeWrapper::Color { .. } => None,
            })
            .collect::<Vec<_>>();
        spans.reverse();
        spans
    }

    fn scope_wrappers(&self) -> Vec<(ContainerType, AttributeMap<'t>)> {
        self.iter_active_rev()
            .filter(|scope| matches!(scope.kind, ScopeKind::Size | ScopeKind::Span))
            .filter_map(|scope| match &scope.wrapper {
                ScopeWrapper::Container { ctype, attributes } => {
                    Some((*ctype, attributes.clone()))
                }
                ScopeWrapper::Color { .. } => None,
            })
            .collect()
    }

    fn label_scope_count(&self) -> usize {
        self.iter_active_rev()
            .filter(|scope| matches!(scope.kind, ScopeKind::Size | ScopeKind::Span))
            .count()
    }
}

pub(crate) fn lower_wikidot_inline_size_scopes<'t>(elements: &mut Vec<Element<'t>>) {
    flatten_crossing_inline_containers(elements);
    let (valid, close_pairs) = collect_valid_scope_pairs(elements);

    let mut ordinal = 0;
    let mut active = ActiveScopes::default();
    active.close_pairs = close_pairs;
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
    flatten_crossing_inline_containers(elements);
    let (valid, close_pairs) = collect_valid_scope_pairs(elements);

    let mut ordinal = 0;
    let mut active = ActiveScopes::default();
    active.close_pairs = close_pairs;
    let mut trim_next_break = false;
    lower_sequence(
        elements,
        &valid,
        &mut ordinal,
        &mut active,
        &mut trim_next_break,
        false,
    );
    eject_segment_trailing_space(elements);
}

fn collect_valid_scope_pairs(
    elements: &[Element<'_>],
) -> (BTreeSet<usize>, BTreeMap<usize, usize>) {
    let mut ordinal = 0;
    let mut pending = PendingScopes::default();
    let mut valid = BTreeSet::new();
    collect_valid_pairs(elements, &mut ordinal, &mut pending, &mut valid);

    (valid, pending.pairs)
}

#[derive(Clone, Copy)]
enum ScopeClass {
    Size,
    Span,
    Format(usize),
    Color,
}

#[derive(Default)]
struct PendingScopes {
    sizes: Vec<PendingScope>,
    spans: Vec<PendingScope>,
    formats: [Vec<PendingScope>; 7],
    colors: Vec<PendingScope>,
    active_count: usize,
    pairs: BTreeMap<usize, usize>,
}

impl PendingScopes {
    fn stack_mut(&mut self, class: ScopeClass) -> &mut Vec<PendingScope> {
        match class {
            ScopeClass::Size => &mut self.sizes,
            ScopeClass::Span => &mut self.spans,
            ScopeClass::Format(index) => &mut self.formats[index],
            ScopeClass::Color => &mut self.colors,
        }
    }

    fn open(&mut self, class: ScopeClass, ordinal: &mut usize) {
        let accepted = self.active_count < MAX_ACTIVE_INLINE_SCOPES;
        if accepted {
            self.active_count += 1;
        }
        self.stack_mut(class).push(PendingScope {
            ordinal: *ordinal,
            accepted,
        });
        *ordinal += 1;
    }

    fn close(
        &mut self,
        class: ScopeClass,
        ordinal: &mut usize,
        valid: &mut BTreeSet<usize>,
    ) {
        let close = *ordinal;
        *ordinal += 1;
        if let Some(open) = self.stack_mut(class).pop()
            && open.accepted
        {
            self.active_count -= 1;
            valid.insert(open.ordinal);
            valid.insert(close);
            self.pairs.insert(close, open.ordinal);
        }
    }
}

fn format_class_index(kind: InlineFormatKind) -> usize {
    match kind {
        InlineFormatKind::Bold => 0,
        InlineFormatKind::Italics => 1,
        InlineFormatKind::Underline => 2,
        InlineFormatKind::Superscript => 3,
        InlineFormatKind::Subscript => 4,
        InlineFormatKind::Strikethrough => 5,
        InlineFormatKind::Monospace => 6,
    }
}

fn format_kind_of(ctype: ContainerType) -> Option<InlineFormatKind> {
    match ctype {
        ContainerType::Bold => Some(InlineFormatKind::Bold),
        ContainerType::Italics => Some(InlineFormatKind::Italics),
        ContainerType::Underline => Some(InlineFormatKind::Underline),
        ContainerType::Superscript => Some(InlineFormatKind::Superscript),
        ContainerType::Subscript => Some(InlineFormatKind::Subscript),
        ContainerType::Strikethrough => Some(InlineFormatKind::Strikethrough),
        ContainerType::Monospace => Some(InlineFormatKind::Monospace),
        _ => None,
    }
}

fn flatten_crossing_inline_containers<'t>(elements: &mut Vec<Element<'t>>) {
    for element in elements.iter_mut() {
        let mut visit = |children: &mut Vec<Element<'t>>| {
            flatten_crossing_inline_containers(children);
        };
        visit_children_mut(element, &mut visit);
    }
    let mut output = Vec::with_capacity(elements.len());
    for mut element in mem::take(elements) {
        if !container_flattens(&element) {
            output.push(element);
            continue;
        }
        match &mut element {
            Element::Container(container) => {
                let kind = format_kind_of(container.ctype())
                    .expect("flatten candidate has a format kind");
                let children = mem::take(container.elements_mut());
                output.push(Element::Partial(PartialElement::InlineFormatOpen(kind)));
                output.extend(children);
                output.push(Element::Partial(PartialElement::InlineFormatClose(kind)));
            }
            Element::Color {
                color,
                background,
                elements: children,
            } => {
                output.push(Element::Partial(PartialElement::InlineColorOpen(
                    color.clone(),
                    *background,
                )));
                output.extend(mem::take(children));
                output.push(Element::Partial(PartialElement::InlineColorClose));
            }
            _ => unreachable!("flatten candidate checked above"),
        }
    }
    *elements = output;
}

fn container_flattens(element: &Element<'_>) -> bool {
    let candidate = match element {
        Element::Container(container) => format_kind_of(container.ctype()).is_some(),
        Element::Color { .. } => true,
        _ => false,
    };
    candidate
        && contains_inline_scope_control(element)
        && !inline_scope_controls_are_self_contained(element)
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
    let mut pending: Vec<Element<'t>> = Vec::new();

    for mut element in mem::take(elements) {
        let is_paragraph = matches!(
            &element,
            Element::Container(container)
                if container.ctype() == ContainerType::Paragraph
        );
        if !is_paragraph && element.paragraph_safe() {
            pending.push(element);
            continue;
        }
        if flush_root_pending(
            &mut output,
            &mut paragraph_group,
            &mut pending,
            valid,
            ordinal,
            active,
            trim_next_break,
        ) {
            previous_scored_close = false;
        }
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
            active.take_space_eject_floor();
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

    flush_root_pending(
        &mut output,
        &mut paragraph_group,
        &mut pending,
        valid,
        ordinal,
        active,
        trim_next_break,
    );
    flush_paragraph_group(&mut output, &mut paragraph_group);
    *elements = output;
}

fn flush_root_pending<'t>(
    output: &mut Vec<Element<'t>>,
    paragraph_group: &mut Option<(bool, AttributeMap<'t>, Vec<Element<'t>>)>,
    pending: &mut Vec<Element<'t>>,
    valid: &BTreeSet<usize>,
    ordinal: &mut usize,
    active: &mut ActiveScopes<'t>,
    trim_next_break: &mut bool,
) -> bool {
    if pending.is_empty() {
        return false;
    }
    flush_paragraph_group(output, paragraph_group);
    let scope_before = active.outer_active_position();
    let mut sequence = mem::take(pending);
    lower_sequence(
        &mut sequence,
        valid,
        ordinal,
        active,
        trim_next_break,
        false,
    );
    eject_segment_trailing_space(&mut sequence);
    let scope_continues =
        scope_before.is_some() && scope_before == active.outer_active_position();
    append_root_sequence(output, &mut sequence, scope_continues);
    true
}

fn is_wikidot_content_separator(element: &Element<'_>) -> bool {
    element.is_content_separator()
}

fn append_root_sequence<'t>(
    output: &mut Vec<Element<'t>>,
    sequence: &mut Vec<Element<'t>>,
    scope_continues: bool,
) {
    let mergeable = scope_continues
        && matches!(
            (output.last(), sequence.first()),
            (Some(previous), Some(next)) if same_scope_wrapper(previous, next)
        );
    if mergeable {
        let next = sequence.remove(0);
        let previous = output.last_mut().expect("mergeable checked above");
        scope_sibling_merge(previous, next);
    }
    output.append(sequence);
}

fn flush_paragraph_group<'t>(
    output: &mut Vec<Element<'t>>,
    paragraph_group: &mut Option<(bool, AttributeMap<'t>, Vec<Element<'t>>)>,
) {
    let Some((wrapped, attributes, mut elements)) = paragraph_group.take() else {
        return;
    };
    if elements.is_empty() {
        return;
    }
    eject_segment_trailing_space(&mut elements);
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
    pending: &mut PendingScopes,
    valid: &mut BTreeSet<usize>,
) {
    for element in elements {
        match element {
            Element::Partial(PartialElement::InlineSizeOpen(_)) => {
                pending.open(ScopeClass::Size, ordinal);
            }
            Element::Partial(PartialElement::InlineSizeClose(_)) => {
                pending.close(ScopeClass::Size, ordinal, valid);
            }
            Element::Partial(PartialElement::InlineSpanOpen(_)) => {
                pending.open(ScopeClass::Span, ordinal);
            }
            Element::Partial(PartialElement::InlineSpanClose(_)) => {
                pending.close(ScopeClass::Span, ordinal, valid);
            }
            Element::Partial(PartialElement::InlineFormatOpen(kind)) => {
                pending.open(ScopeClass::Format(format_class_index(*kind)), ordinal);
            }
            Element::Partial(PartialElement::InlineFormatClose(kind)) => {
                pending.close(
                    ScopeClass::Format(format_class_index(*kind)),
                    ordinal,
                    valid,
                );
            }
            Element::Partial(PartialElement::InlineColorOpen(..)) => {
                pending.open(ScopeClass::Color, ordinal);
            }
            Element::Partial(PartialElement::InlineColorClose) => {
                pending.close(ScopeClass::Color, ordinal, valid);
            }
            _ => {
                let mut visit = |children: &[Element<'_>]| {
                    collect_valid_pairs(children, ordinal, pending, valid)
                };
                visit_children(element, &mut visit);
            }
        }
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
    let mut run_has_content = false;
    let mut effects = SequenceEffects::default();
    let mut last_run_outer_scope = RunMergeState::default();
    let mut preserve_space_before_footnote = false;

    let mut source = mem::take(elements).into_iter().peekable();
    while let Some(mut element) = source.next() {
        if let Some(floor) = active.take_space_eject_floor() {
            let starts_with_space = matches!(
                &element,
                Element::Text(text) if text.starts_with([' ', '\t'])
            );
            if starts_with_space {
                let Element::Text(text) = mem::replace(&mut element, Element::LineBreak)
                else {
                    unreachable!("text element checked above")
                };
                let source_text: &str = &text;
                let rest = source_text.trim_start_matches([' ', '\t']);
                let ws_len = source_text.len() - rest.len();
                let whitespace = source_text[..ws_len].to_owned();
                let remainder = rest.to_owned();
                drop(text);
                let saved_tail = active.active_tail;
                let saved_next = floor.and_then(|f| active.scopes[f].next_active);
                match floor {
                    Some(f) => {
                        active.scopes[f].next_active = None;
                        active.active_tail = Some(f);
                    }
                    None => active.active_tail = None,
                }
                run.push(Element::Text(Cow::Owned(whitespace)));
                flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
                if let Some(f) = floor {
                    active.scopes[f].next_active = saved_next;
                }
                active.active_tail = saved_tail;
                if remainder.is_empty() {
                    continue;
                }
                element = Element::Text(Cow::Owned(remainder));
            }
        }
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
                active.push(
                    ScopeKind::Size,
                    ScopeWrapper::Container {
                        ctype: ContainerType::Size,
                        attributes,
                    },
                    false,
                );
                active.record_open_instance(current);
            }
            continue;
        }
        if let Element::Partial(PartialElement::InlineSizeClose(close_source)) = element {
            let resolved = active.resolve_close(*ordinal, InlineTag::Span);
            let empty = !resolved
                .is_some_and(|position| active.scope_has_content(position))
                && run.is_empty();
            let trailing_space =
                matches!(run.last(), Some(Element::Text(text)) if text == " ");
            let next_starts_with_space = matches!(
                source.peek(),
                Some(Element::Text(text)) if text.starts_with(' ')
            );
            let next_is_break = matches!(
                source.peek(),
                Some(Element::LineBreak | Element::LineBreaks(_))
            );
            let current = *ordinal;
            *ordinal += 1;
            if valid.contains(&current) {
                if trailing_space {
                    run.pop();
                }
                flush_close_run(
                    &mut output,
                    &mut run,
                    active,
                    &mut last_run_outer_scope,
                    &mut run_has_content,
                );
                trim_trailing_space(&mut output);
                if let Some(position) = resolved {
                    active.commit_close(position);
                }
                if trailing_space
                    && !next_starts_with_space
                    && !next_is_break
                    && !empty
                    && source.peek().is_some()
                {
                    run.push(text!(" "));
                    run_has_content = true;
                }
            } else {
                run.push(Element::Text(close_source));
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
                active.push(
                    ScopeKind::Span,
                    ScopeWrapper::Container {
                        ctype: ContainerType::Span,
                        attributes,
                    },
                    scored,
                );
                active.record_open_instance(current);
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
            let current = *ordinal;
            *ordinal += 1;
            if !valid.contains(&current) {
                run.push(Element::Text(close_source));
                continue;
            }
            let resolved = active.resolve_close(current, InlineTag::Span);
            let scored =
                resolved.is_some_and(|position| active.scope_is_scored(position));
            let empty = !resolved
                .is_some_and(|position| active.scope_has_content(position))
                && run.is_empty();
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
            flush_close_run(
                &mut output,
                &mut run,
                active,
                &mut last_run_outer_scope,
                &mut run_has_content,
            );
            let preserve_empty_owner_space = empty
                && !scored
                && matches!(source.peek(), Some(Element::Footnote(_)))
                && sequence_starts_with_repeated_list_marker(&output, &run)
                && sequence_ends_with_space(&output, &run);
            if empty && !scored && !preserve_empty_owner_space {
                trim_trailing_space(&mut output);
            }
            if !(preserve_empty_owner_space || empty && scored) {
                trim_trailing_space(&mut output);
            }
            if let Some(position) = resolved {
                active.commit_close(position);
            }
            if trailing_space
                && !next_starts_with_space
                && !matches!(
                    source.peek(),
                    Some(Element::LineBreak | Element::LineBreaks(_))
                )
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
            continue;
        }
        if let Element::Partial(PartialElement::InlineFormatOpen(kind)) = element {
            flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
            let current = *ordinal;
            *ordinal += 1;
            if valid.contains(&current) {
                active.push(
                    ScopeKind::Format(kind),
                    ScopeWrapper::Container {
                        ctype: kind.container_type(),
                        attributes: AttributeMap::new(),
                    },
                    false,
                );
                active.record_open_instance(current);
            }
            continue;
        }
        if let Element::Partial(PartialElement::InlineFormatClose(kind)) = element {
            let current = *ordinal;
            *ordinal += 1;
            if valid.contains(&current) {
                close_flattened_scope(
                    &mut output,
                    &mut run,
                    active,
                    &mut last_run_outer_scope,
                    &mut run_has_content,
                    inline_tag_for_ctype(kind.container_type()),
                    current,
                    source.peek(),
                );
            } else {
                run.push(Element::Text(Cow::Borrowed(kind.close_marker())));
            }
            continue;
        }
        if let Element::Partial(PartialElement::InlineColorOpen(color, background)) =
            element
        {
            flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
            let current = *ordinal;
            *ordinal += 1;
            if valid.contains(&current) {
                active.push(
                    ScopeKind::Color,
                    ScopeWrapper::Color { color, background },
                    false,
                );
                active.record_open_instance(current);
            }
            continue;
        }
        if let Element::Partial(PartialElement::InlineColorClose) = element {
            let current = *ordinal;
            *ordinal += 1;
            if valid.contains(&current) {
                close_flattened_scope(
                    &mut output,
                    &mut run,
                    active,
                    &mut last_run_outer_scope,
                    &mut run_has_content,
                    InlineTag::Span,
                    current,
                    source.peek(),
                );
            } else {
                run.push(text!("##"));
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
                run_has_content = true;
            } else {
                flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
                output.push(element);
                last_run_outer_scope.reset();
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
            run_has_content = true;
        } else if has_lowerable_children(&element) {
            let list_legacy = matches!(element, Element::List { .. });
            if list_legacy {
                lower_children(&mut element, valid, ordinal, active, trim_next_break);
                flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
            } else {
                let block_boundary = is_block_boundary(&element);
                let stripped = block_boundary
                    && active.has_active_scope()
                    && strip_trailing_space_from_vec(&mut run);
                flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
                if stripped {
                    output.push(text!(" "));
                }
                let watermark = if block_boundary {
                    Some(active.begin_watch())
                } else {
                    None
                };
                lower_children(&mut element, valid, ordinal, active, trim_next_break);
                if let Some(watermark) = watermark {
                    let completed = active.end_watch();
                    active.drop_block_boundary_scopes(watermark, &completed);
                }
            }
            if let Some(scopes) = list_scope_wrappers {
                let active_after = active.label_scope_count();
                if active_after == scopes.len() {
                    let wrapped = wrap_scope_elements(&scopes, vec![Element::LineBreak]);
                    output.push(Element::Container(Container::new(
                        ContainerType::Paragraph,
                        wrapped,
                        AttributeMap::new(),
                    )));
                    *trim_next_break = true;
                    last_run_outer_scope.reset();
                } else if active_after == 0 {
                    output.extend(wrap_scope_elements(&scopes, vec![Element::LineBreak]));
                    last_run_outer_scope.reset();
                }
            }
            if !is_empty_paragraph(&element) {
                output.push(element);
                last_run_outer_scope.reset();
            }
        } else {
            flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
            output.push(element);
            last_run_outer_scope.reset();
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

fn run_merges_across_separator(output: &[Element<'_>], next: &Element<'_>) -> bool {
    if !matches!(output.last(), Some(Element::Text(text)) if text == " ") {
        return false;
    }
    let Some(previous) = output
        .len()
        .checked_sub(2)
        .and_then(|index| output.get(index))
    else {
        return false;
    };
    same_scope_wrapper(previous, next)
}

fn is_inline_scope_ctype(ctype: ContainerType) -> bool {
    matches!(
        ctype,
        ContainerType::Span
            | ContainerType::Size
            | ContainerType::Bold
            | ContainerType::Italics
            | ContainerType::Underline
            | ContainerType::Superscript
            | ContainerType::Subscript
            | ContainerType::Strikethrough
            | ContainerType::Monospace
    )
}

fn same_scope_wrapper(a: &Element<'_>, b: &Element<'_>) -> bool {
    match (a, b) {
        (Element::Container(x), Element::Container(y)) => {
            x.ctype() == y.ctype()
                && x.attributes() == y.attributes()
                && is_inline_scope_ctype(x.ctype())
        }
        (
            Element::Color {
                color: x_color,
                background: x_background,
                ..
            },
            Element::Color {
                color: y_color,
                background: y_background,
                ..
            },
        ) => x_color == y_color && x_background == y_background,
        _ => false,
    }
}

fn scope_children_mut<'a, 't>(element: &'a mut Element<'t>) -> &'a mut Vec<Element<'t>> {
    match element {
        Element::Container(container) => container.elements_mut(),
        Element::Color { elements, .. } => elements,
        _ => unreachable!("scope wrapper element"),
    }
}

fn scope_sibling_merge<'t>(previous: &mut Element<'t>, next: Element<'t>) -> bool {
    match (previous, next) {
        (Element::Container(previous), Element::Container(next))
            if previous.ctype() == next.ctype()
                && previous.attributes() == next.attributes()
                && is_inline_scope_ctype(previous.ctype()) =>
        {
            previous.elements_mut().extend(Vec::from(next));
            true
        }
        (
            Element::Color {
                color: previous_color,
                background: previous_background,
                elements: previous_elements,
            },
            Element::Color {
                color: next_color,
                background: next_background,
                elements: next_elements,
            },
        ) if previous_color == &next_color && previous_background == &next_background => {
            previous_elements.extend(next_elements);
            true
        }
        _ => false,
    }
}

fn scope_wrapper_is_empty(element: &Element<'_>) -> bool {
    match element {
        Element::Container(container) => container.elements().is_empty(),
        Element::Color { elements, .. } => elements.is_empty(),
        _ => false,
    }
}

fn strip_scope_trailing_space(element: &mut Element<'_>) -> bool {
    match element {
        Element::Container(container) if is_inline_scope_ctype(container.ctype()) => {
            strip_trailing_space_from_vec(container.elements_mut())
        }
        Element::Color { elements, .. } => strip_trailing_space_from_vec(elements),
        _ => false,
    }
}

fn strip_trailing_space_from_vec(elements: &mut Vec<Element<'_>>) -> bool {
    let mut stripped = false;
    loop {
        match elements.last_mut() {
            Some(Element::Text(text)) => {
                let keep = text.trim_end_matches([' ', '\n', '\t', '\r']).len();
                if keep == text.len() {
                    break;
                }
                stripped = true;
                if keep == 0 {
                    elements.pop();
                } else {
                    text.to_mut().truncate(keep);
                    break;
                }
            }
            Some(element) => {
                let inner = strip_scope_trailing_space(element);
                if inner && scope_wrapper_is_empty(element) {
                    elements.pop();
                }
                stripped |= inner;
                break;
            }
            None => break,
        }
    }
    stripped
}

fn eject_segment_trailing_space(elements: &mut Vec<Element<'_>>) {
    let stripped = match elements.last_mut() {
        Some(element) => strip_scope_trailing_space(element),
        None => false,
    };
    if stripped {
        if elements.last().is_some_and(scope_wrapper_is_empty) {
            elements.pop();
        }
        elements.push(text!(" "));
    }
}

fn is_block_boundary(element: &Element<'_>) -> bool {
    !matches!(element, Element::List { .. })
        && parent_is_block_segment(element)
        && !matches!(
            element,
            Element::Container(container)
                if container.ctype() == ContainerType::Paragraph
        )
}

fn has_lowerable_children(element: &Element<'_>) -> bool {
    let mut found = false;
    visit_children(element, &mut |_| found = true);
    found
}

fn parent_is_block_segment(element: &Element<'_>) -> bool {
    match element {
        Element::Container(container) => matches!(
            container.ctype(),
            ContainerType::Div
                | ContainerType::Paragraph
                | ContainerType::Blockquote
                | ContainerType::Align(_)
        ),
        Element::Collapsible { .. }
        | Element::List { .. }
        | Element::Table(_)
        | Element::TabView(_)
        | Element::DefinitionList(_) => true,
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

fn close_flattened_scope<'t>(
    output: &mut Vec<Element<'t>>,
    run: &mut Vec<Element<'t>>,
    active: &mut ActiveScopes<'t>,
    last_run_outer_scope: &mut RunMergeState,
    run_has_content: &mut bool,
    tag: InlineTag,
    close_ordinal: usize,
    next: Option<&Element<'t>>,
) {
    let next_starts_with_space =
        matches!(next, Some(Element::Text(text)) if text.starts_with(' '));
    let has_next = next.is_some();
    let resolved = active.resolve_close(close_ordinal, tag);
    let empty = !resolved.is_some_and(|position| active.scope_has_content(position))
        && run.is_empty();
    let trailing_space = matches!(run.last(), Some(Element::Text(text)) if text == " ");
    if trailing_space {
        run.pop();
    }
    flush_close_run(output, run, active, last_run_outer_scope, run_has_content);
    trim_trailing_space(output);
    if let Some(position) = resolved {
        active.commit_close(position);
    }
    if trailing_space && !next_starts_with_space && !empty && has_next {
        run.push(text!(" "));
        *run_has_content = true;
    }
}

fn flush_close_run<'t>(
    output: &mut Vec<Element<'t>>,
    run: &mut Vec<Element<'t>>,
    active: &mut ActiveScopes<'t>,
    last_run_outer_scope: &mut RunMergeState,
    run_has_content: &mut bool,
) {
    if !*run_has_content && !run.is_empty() {
        output.append(run);
        last_run_outer_scope.reset();
    } else {
        flush_run(output, run, active, last_run_outer_scope);
    }
    *run_has_content = false;
}

#[derive(Default)]
struct RunMergeState {
    outer: Option<usize>,
    chain: Vec<usize>,
}

impl RunMergeState {
    fn reset(&mut self) {
        self.outer = None;
        self.chain.clear();
    }
}

fn deep_scope_merge<'t>(
    previous: &mut Element<'t>,
    next: Element<'t>,
    previous_chain: &[usize],
    next_chain: &[usize],
    depth: usize,
) {
    let mut next = next;
    let next_children = mem::take(scope_children_mut(&mut next));
    let previous_children = scope_children_mut(previous);
    let mut first = true;
    for child in next_children {
        let mergeable = first
            && previous_chain.get(depth + 1) == next_chain.get(depth + 1)
            && previous_chain.get(depth + 1).is_some()
            && matches!(
                (previous_children.last(), &child),
                (Some(last), child) if same_scope_wrapper(last, child)
            );
        if mergeable {
            let last = previous_children.last_mut().expect("merge checked above");
            deep_scope_merge(last, child, previous_chain, next_chain, depth + 1);
        } else {
            previous_children.push(child);
        }
        first = false;
    }
}

fn flush_run<'t>(
    output: &mut Vec<Element<'t>>,
    run: &mut Vec<Element<'t>>,
    active: &mut ActiveScopes<'t>,
    last_run_outer_scope: &mut RunMergeState,
) {
    if run.is_empty() {
        return;
    }
    let outer_scope = active.outer_active_position();
    let chain = active.active_positions();
    active.mark_active_content();
    let mut wrapped = mem::take(run);
    for scope in active.iter_active_rev() {
        wrapped = vec![scope.wrapper.wrap(wrapped)];
    }
    if outer_scope.is_some() && wrapped.len() == 1 {
        if outer_scope == last_run_outer_scope.outer
            && run_merges_across_separator(output, &wrapped[0])
        {
            let Some(Element::Text(separator)) = output.pop() else {
                unreachable!("separator space checked above");
            };
            let next = wrapped.pop().expect("wrapped scope element");
            let previous = output
                .last_mut()
                .expect("merge-compatible element checked above");
            scope_children_mut(previous).push(Element::Text(separator));
            let previous_chain = mem::take(&mut last_run_outer_scope.chain);
            deep_scope_merge(previous, next, &previous_chain, &chain, 0);
            last_run_outer_scope.outer = outer_scope;
            last_run_outer_scope.chain = chain;
            return;
        }
        let next = wrapped.pop().expect("wrapped scope element");
        let can_merge = outer_scope == last_run_outer_scope.outer
            && matches!(
                (output.last(), &next),
                (Some(previous), next) if same_scope_wrapper(previous, next)
            );
        if can_merge {
            let previous = output.last_mut().expect("merge checked above");
            let previous_chain = mem::take(&mut last_run_outer_scope.chain);
            deep_scope_merge(previous, next, &previous_chain, &chain, 0);
            last_run_outer_scope.outer = outer_scope;
            last_run_outer_scope.chain = chain;
            return;
        }
        output.push(next);
        last_run_outer_scope.outer = outer_scope;
        last_run_outer_scope.chain = chain;
        return;
    }
    output.extend(wrapped);
    last_run_outer_scope.reset();
}

fn trim_trailing_space(elements: &mut Vec<Element<'_>>) {
    let Some(last) = elements.last_mut() else {
        return;
    };
    if let Element::Container(container) = last
        && is_inline_scope_ctype(container.ctype())
    {
        trim_trailing_space(container.elements_mut());
        if container.elements().is_empty() {
            elements.pop();
        }
        return;
    }
    if let Element::Color {
        elements: inner, ..
    } = last
    {
        trim_trailing_space(inner);
        if inner.is_empty() {
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
                | PartialElement::InlineFormatOpen(_)
                | PartialElement::InlineFormatClose(_)
                | PartialElement::InlineColorOpen(..)
                | PartialElement::InlineColorClose
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
    fn visit(element: &Element<'_>, counts: &mut [i32; 10]) -> bool {
        if let Element::Partial(partial) = element
            && let Some((class, is_open)) = partial_scope_class(partial)
        {
            if is_open {
                counts[class] += 1;
            } else if counts[class] == 0 {
                return false;
            } else {
                counts[class] -= 1;
            }
            return true;
        }
        let mut valid = true;
        visit_children(element, &mut |children| {
            for child in children {
                valid &= visit(child, counts);
            }
        });
        valid
    }

    let mut counts = [0; 10];
    visit(element, &mut counts) && counts.iter().all(|count| *count == 0)
}

fn partial_scope_class(partial: &PartialElement<'_>) -> Option<(usize, bool)> {
    match partial {
        PartialElement::InlineSizeOpen(_) => Some((0, true)),
        PartialElement::InlineSizeClose(_) => Some((0, false)),
        PartialElement::InlineSpanOpen(_) => Some((1, true)),
        PartialElement::InlineSpanClose(_) => Some((1, false)),
        PartialElement::InlineFormatOpen(kind) => {
            Some((2 + format_class_index(*kind), true))
        }
        PartialElement::InlineFormatClose(kind) => {
            Some((2 + format_class_index(*kind), false))
        }
        PartialElement::InlineColorOpen(..) => Some((9, true)),
        PartialElement::InlineColorClose => Some((9, false)),
        _ => None,
    }
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

    let eject = matches!(element, Element::Collapsible { .. })
        || matches!(
            element,
            Element::Container(container) if matches!(
                container.ctype(),
                ContainerType::Div
                    | ContainerType::Blockquote
                    | ContainerType::Align(_)
            )
        );
    let mut lowered = false;
    let mut visit = |children: &mut Vec<Element<'t>>| {
        lowered = true;
        lower_sequence(children, valid, ordinal, active, trim_next_break, false);
        if eject {
            eject_segment_trailing_space(children);
            strip_trailing_space_from_vec(children);
        }
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
            for item in tabs {
                visit(item.elements());
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
            | PartialElement::InlineSpanClose(_)
            | PartialElement::InlineFormatOpen(_)
            | PartialElement::InlineFormatClose(_)
            | PartialElement::InlineColorOpen(..)
            | PartialElement::InlineColorClose => {}
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
            for item in tabs {
                visit(item.elements_mut());
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
                ScopeKind::Format(_) | ScopeKind::Color => unreachable!(),
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
                ScopeKind::Format(_) | ScopeKind::Color => unreachable!(),
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
