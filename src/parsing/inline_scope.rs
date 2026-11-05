use crate::tree::{
    AnchorTarget, AttributeMap, Bibliography, BibliographyList, BibliographyScopeWrapper,
    Container, ContainerType, Element, InlineFormatKind, ListItem, PartialElement,
    WIKIDOT_WS_MONO_VANISH_MARKER, WikidotBlockCloseKind, WikidotBlockShell,
    WikidotFrameLink,
};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::mem;

const MAX_ACTIVE_INLINE_SCOPES: usize = 101;
pub(super) const WIKIDOT_SCORE_SPAN_UNWRAPPED_ATTRIBUTE: &str =
    "data-ftml-score-span-own-line";
pub(super) const WIKIDOT_SCORE_ANCHOR_ATTRIBUTE: &str = "data-ftml-score-anchor";
pub(super) const WIKIDOT_SCORE_ANCHOR_UNWRAPPED_ATTRIBUTE: &str =
    "data-ftml-score-anchor-own-line";

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    Size,
    Span,
    Format(InlineFormatKind),
    Color,
    Anchor,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InlineTag {
    Span,
    Strong,
    Emphasis,
    Monospace,
    Subscript,
    Superscript,
    Anchor,
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
    Anchor {
        attributes: AttributeMap<'t>,
        target: Option<AnchorTarget>,
    },
}

impl<'t> ScopeWrapper<'t> {
    fn tag(&self) -> InlineTag {
        match self {
            ScopeWrapper::Container { ctype, .. } => inline_tag_for_ctype(*ctype),
            ScopeWrapper::Color { .. } => InlineTag::Span,
            ScopeWrapper::Anchor { .. } => InlineTag::Anchor,
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
            ScopeWrapper::Anchor { attributes, target } => Element::Anchor {
                target: *target,
                attributes: attributes.clone(),
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
    pending: bool,
    materialized: bool,
    origin: usize,
    previous_active: Option<usize>,
    next_active: Option<usize>,
}

#[derive(Clone)]
enum IstEntry<'t> {
    Scope {
        tag: InlineTag,
        position: usize,
        kind: ScopeKind,
        wrapper: ScopeWrapper<'t>,
        scored: bool,
        origin: usize,
    },
    Inline {
        tag: InlineTag,
        wrapper: ScopeWrapper<'t>,
    },
}

impl<'t> IstEntry<'t> {
    fn tag(&self) -> InlineTag {
        match self {
            IstEntry::Scope { tag, .. } => *tag,
            IstEntry::Inline { tag, .. } => *tag,
        }
    }
}

#[derive(Default)]
struct ActiveScopes<'t> {
    scopes: Vec<ActiveScope<'t>>,
    active_tail: Option<usize>,
    watches: Vec<(usize, Vec<InlineTag>)>,
    space_eject_floor: Option<Option<usize>>,
    close_pairs: BTreeMap<usize, usize>,
    lifo_close_pairs: BTreeMap<usize, usize>,
    open_instances: BTreeMap<usize, usize>,
    consumed: BTreeSet<usize>,
    istack: Vec<IstEntry<'t>>,
    deferred_closes: Vec<usize>,
    cell_local: bool,
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
            pending: false,
            materialized: false,
            origin,
            previous_active,
            next_active: None,
        });
        if let Some(previous_active) = previous_active {
            self.scopes[previous_active].next_active = Some(position);
        }
        let (tag, kind, wrapper, scored, origin) = {
            let scope = &self.scopes[position];
            (
                scope.wrapper.tag(),
                scope.kind,
                scope.wrapper.clone(),
                scope.scored,
                scope.origin,
            )
        };
        if !self.istack.iter().any(|entry| entry.tag() == tag) {
            self.istack.push(IstEntry::Scope {
                tag,
                position,
                kind,
                wrapper,
                scored,
                origin,
            });
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
        for entry in &drops {
            self.unlink(*entry);
        }
        self.istack.retain(|entry| match entry {
            IstEntry::Scope { position, .. } => !drops.contains(position),
            IstEntry::Inline { .. } => true,
        });
    }

    fn drop_consumed_scopes(&mut self) {
        if self.consumed.is_empty() {
            return;
        }
        let mut drops = Vec::new();
        let mut position = self.active_tail;
        while let Some(current) = position {
            if self.consumed.contains(&self.scopes[current].origin) {
                drops.push(current);
            }
            position = self.scopes[current].previous_active;
        }
        if drops.is_empty() {
            return;
        }
        for entry in &drops {
            self.unlink(*entry);
        }
        self.istack.retain(|entry| match entry {
            IstEntry::Scope { position, .. } => !drops.contains(position),
            IstEntry::Inline { .. } => true,
        });
    }

    fn note_close_fired(&mut self, close_ordinal: usize) {
        let lineage = self
            .lifo_close_pairs
            .get(&close_ordinal)
            .and_then(|open| self.open_instances.get(open))
            .copied();
        if let Some(lineage) = lineage {
            self.consumed.insert(lineage);
        }
    }

    fn truncate_to_outermost(&mut self) {
        self.drop_consumed_scopes();
        let Some(outermost) = self.outer_active_position() else {
            return;
        };
        let mut drops = Vec::new();
        let mut position = self.active_tail;
        while let Some(current) = position {
            if current != outermost {
                drops.push(current);
            }
            position = self.scopes[current].previous_active;
        }
        if drops.is_empty() {
            return;
        }
        for entry in &drops {
            self.unlink(*entry);
        }
        self.istack.retain(|entry| match entry {
            IstEntry::Scope { position, .. } => !drops.contains(position),
            IstEntry::Inline { .. } => true,
        });
    }

    /// End scopes left open by a deferred crossed close. Called once the
    /// enclosing paragraph has been flushed, so the scope's own close has
    /// already been rendered and it must not reopen in the next paragraph.
    fn drop_deferred_closes(&mut self) {
        if self.deferred_closes.is_empty() {
            return;
        }
        for position in mem::take(&mut self.deferred_closes) {
            let linked = self.active_tail == Some(position)
                || self.scopes[position].next_active.is_some();
            if linked {
                self.kill_scope(position);
            }
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
        if self.cell_local
            && let Some(open) = self.close_pairs.get(&close_ordinal)
            && !self.open_instances.contains_key(open)
        {
            return None;
        }
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

    fn anchor_position(&self) -> Option<usize> {
        let mut position = self.active_tail;
        while let Some(current) = position {
            if self.scopes[current].kind == ScopeKind::Anchor {
                return Some(current);
            }
            position = self.scopes[current].previous_active;
        }
        None
    }

    fn has_anchor_above(&self, position: usize) -> bool {
        let mut cursor = self.scopes[position].next_active;
        while let Some(current) = cursor {
            if self.scopes[current].kind == ScopeKind::Anchor {
                return true;
            }
            cursor = self.scopes[current].next_active;
        }
        false
    }

    fn drop_anchor_scopes(&mut self) {
        let mut positions = Vec::new();
        let mut current = self.active_tail;
        while let Some(position) = current {
            if self.scopes[position].kind == ScopeKind::Anchor {
                positions.push(position);
            }
            current = self.scopes[position].previous_active;
        }
        for position in positions {
            self.unlink(position);
        }
    }

    fn commit_close_no_reopen(&mut self, position: usize) {
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
        if let Some(below) = below {
            self.scopes[below].next_active = None;
        }
        self.active_tail = below;
        self.scopes[position].previous_active = None;
        self.scopes[position].next_active = None;
        for entry in above {
            self.scopes[entry].previous_active = None;
            self.scopes[entry].next_active = None;
        }
    }

    fn scope_has_content(&self, position: usize) -> bool {
        self.scopes[position].has_content
    }

    fn scope_is_scored(&self, position: usize) -> bool {
        self.scopes[position].scored
    }

    fn note_scope_closed(&mut self, position: usize) {
        let closed = &self.scopes[position];
        let closed_origin = closed.origin;
        let closed_tag = closed.wrapper.tag();
        for (watermark, tags) in &mut self.watches {
            if closed_origin >= *watermark && !tags.contains(&closed_tag) {
                tags.push(closed_tag);
            }
        }
    }

    fn end_scope(&mut self, position: usize) {
        self.unlink(position);
    }

    fn tidy_close(&mut self, tag: InlineTag) {
        let mut ended: Vec<usize> = Vec::new();
        let mut insert_at: Option<usize> = None;
        while let Some(top) = self.active_tail {
            let top_tag = self.scopes[top].wrapper.tag();
            if top_tag == tag {
                self.istack.pop();
                self.end_scope(top);
                ended.push(top);
                break;
            }
            let element_index =
                self.istack.iter().rposition(|entry| entry.tag() == top_tag);
            let close_index = self.istack.iter().position(|entry| entry.tag() == tag);
            match (element_index, close_index) {
                (Some(element), Some(close)) if close < element => {
                    self.istack.swap(element, close);
                    insert_at = Some(close);
                    self.end_scope(top);
                    ended.push(top);
                }
                _ => {
                    // The close target sits above the current element in the
                    // istack, so its own close is deferred: the scope stays
                    // open to wrap the reopened elements, and is ended at the
                    // enclosing paragraph boundary instead of leaking into the
                    // next paragraph.
                    if let Some(close) = close_index
                        && let IstEntry::Scope { position, .. } = &self.istack[close]
                    {
                        let position = *position;
                        if !self.deferred_closes.contains(&position) {
                            self.deferred_closes.push(position);
                        }
                    }
                    self.istack.pop();
                    self.end_scope(top);
                    ended.push(top);
                    break;
                }
            }
        }
        if ended.is_empty() {
            return;
        }
        let base = self.active_tail;
        let mut reopened: Vec<usize> = Vec::new();
        if let Some(at) = insert_at {
            let entries: Vec<(ScopeKind, ScopeWrapper<'t>, bool, usize, Option<usize>)> =
                self.istack
                    .iter()
                    .skip(at)
                    .map(|entry| match entry {
                        IstEntry::Scope {
                            position,
                            kind,
                            wrapper,
                            scored,
                            origin,
                            ..
                        } => (*kind, wrapper.clone(), *scored, *origin, Some(*position)),
                        IstEntry::Inline { wrapper, .. } => (
                            scope_kind_of_wrapper(wrapper),
                            wrapper.clone(),
                            false,
                            self.scopes.len(),
                            None,
                        ),
                    })
                    .collect();
            for (kind, wrapper, scored, origin, source) in entries {
                self.push_with_origin(kind, wrapper, scored, origin);
                if let Some(source) = source {
                    reopened.push(source);
                }
            }
        }
        for position in &ended {
            if !reopened.contains(position) {
                self.note_scope_closed(*position);
            }
        }
        if ended.len() >= 2 {
            self.space_eject_floor = Some(base);
        }
    }

    fn istack_push_inline(&mut self, tag: InlineTag, wrapper: ScopeWrapper<'t>) {
        if !self.istack.iter().any(|entry| entry.tag() == tag) {
            self.istack.push(IstEntry::Inline { tag, wrapper });
        }
    }

    fn apply_subtree_istack(&mut self, element: &Element<'t>) {
        if let Some((tag, wrapper)) = inline_scope_parts(element) {
            self.istack_push_inline(tag, wrapper);
            if let Some(children) = subtree_children(element) {
                for child in children {
                    self.apply_subtree_istack(child);
                }
            }
            self.istack.pop();
        } else if let Some(children) = subtree_children(element) {
            for child in children {
                self.apply_subtree_istack(child);
            }
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
            self.scopes[current].materialized = true;
            self.scopes[current].pending = false;
            position = self.scopes[current].previous_active;
        }
    }

    fn any_live_unmaterialized(&self) -> bool {
        let mut position = self.active_tail;
        while let Some(current) = position {
            let scope = &self.scopes[current];
            if !scope.pending && !scope.materialized {
                return true;
            }
            position = scope.previous_active;
        }
        false
    }

    fn copies_from(&self, position: usize) -> Vec<Element<'t>> {
        let positions = self.active_positions();
        let Some(index) = positions.iter().position(|entry| *entry == position) else {
            return Vec::new();
        };
        let mut copies: Vec<Element<'t>> = Vec::new();
        for entry in positions[index..].iter().rev() {
            copies = vec![self.scopes[*entry].wrapper.wrap(copies)];
        }
        copies
    }

    fn any_unmaterialized(&self) -> bool {
        let mut position = self.active_tail;
        while let Some(current) = position {
            if !self.scopes[current].materialized {
                return true;
            }
            position = self.scopes[current].previous_active;
        }
        false
    }

    fn pend_all(&mut self) {
        let mut position = self.active_tail;
        while let Some(current) = position {
            self.scopes[current].pending = true;
            self.scopes[current].materialized = false;
            position = self.scopes[current].previous_active;
        }
    }

    fn enter_block(&mut self) {
        let mut position = self.active_tail;
        while let Some(current) = position {
            self.scopes[current].pending = false;
            self.scopes[current].materialized = false;
            position = self.scopes[current].previous_active;
        }
    }

    fn scope_materialized(&self, position: usize) -> bool {
        self.scopes[position].materialized
    }

    fn scope_is_pending(&self, position: usize) -> bool {
        self.scopes[position].pending
    }

    fn kill_from(&mut self, position: usize) {
        let mut killed = Vec::new();
        let mut cursor = Some(position);
        while let Some(current) = cursor {
            killed.push(current);
            cursor = self.scopes[current].next_active;
        }
        for &entry in &killed {
            let closed = &self.scopes[entry];
            let closed_origin = closed.origin;
            let closed_tag = closed.wrapper.tag();
            for (watermark, tags) in &mut self.watches {
                if closed_origin >= *watermark && !tags.contains(&closed_tag) {
                    tags.push(closed_tag);
                }
            }
        }
        let below = self.scopes[position].previous_active;
        if let Some(below) = below {
            self.scopes[below].next_active = None;
        }
        if self.active_tail.is_some_and(|tail| killed.contains(&tail)) {
            self.active_tail = below;
        }
        for &entry in &killed {
            self.scopes[entry].previous_active = None;
            self.scopes[entry].next_active = None;
        }
        self.istack.retain(|entry| match entry {
            IstEntry::Scope { position, .. } => !killed.contains(position),
            IstEntry::Inline { .. } => true,
        });
    }

    fn kill_scope(&mut self, position: usize) {
        let closed = &self.scopes[position];
        let closed_origin = closed.origin;
        let closed_tag = closed.wrapper.tag();
        for (watermark, tags) in &mut self.watches {
            if closed_origin >= *watermark && !tags.contains(&closed_tag) {
                tags.push(closed_tag);
            }
        }
        self.unlink(position);
        self.istack.retain(|entry| match entry {
            IstEntry::Scope {
                position: entry_position,
                ..
            } => *entry_position != position,
            IstEntry::Inline { .. } => true,
        });
    }

    fn nested_empty_copies(&self) -> Vec<Element<'t>> {
        let mut copies: Vec<Element<'t>> = Vec::new();
        for position in self.active_positions() {
            copies = vec![self.scopes[position].wrapper.wrap(copies)];
        }
        copies
    }

    fn has_anchor_scope(&self) -> bool {
        self.anchor_position().is_some()
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
                ScopeWrapper::Anchor { .. } => None,
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
                ScopeWrapper::Anchor { .. } => None,
            })
            .collect()
    }

    fn label_scope_count(&self) -> usize {
        self.iter_active_rev()
            .filter(|scope| matches!(scope.kind, ScopeKind::Size | ScopeKind::Span))
            .count()
    }

    fn retire_scope_by_lineage(&mut self, lineage: usize) {
        let mut position = self.active_tail;
        while let Some(current) = position {
            if self.scopes[current].origin == lineage {
                self.unlink(current);
                return;
            }
            position = self.scopes[current].previous_active;
        }
    }

    fn bibliography_wrappers(&self) -> Vec<BibliographyScopeWrapper<'t>> {
        self.active_positions()
            .into_iter()
            .filter_map(|position| match &self.scopes[position].wrapper {
                ScopeWrapper::Container { ctype, attributes } => {
                    Some(BibliographyScopeWrapper::Container {
                        ctype: *ctype,
                        attributes: attributes.clone(),
                    })
                }
                ScopeWrapper::Color { color, background } => {
                    Some(BibliographyScopeWrapper::Color {
                        color: color.clone(),
                        background: *background,
                    })
                }
                ScopeWrapper::Anchor { .. } => None,
            })
            .collect()
    }
}

struct BibliographyLowering<'b, 't> {
    bibliographies: &'b mut BibliographyList<'t>,
    lowered: &'b mut Vec<bool>,
}

#[cfg(test)]
fn lower_wikidot_inline_size_scopes<'t>(elements: &mut Vec<Element<'t>>) {
    lower_wikidot_inline_size_scopes_with_bibliographies(elements, None);
}

pub(crate) fn lower_wikidot_inline_size_scopes_with_bibliographies<'t>(
    elements: &mut Vec<Element<'t>>,
    mut bibliographies: Option<&mut BibliographyList<'t>>,
) -> Vec<bool> {
    flatten_crossing_inline_containers(elements);
    build_wikidot_block_frames(elements, true);
    let mut lowered = Vec::new();
    let (valid, close_pairs, lifo_close_pairs) = match bibliographies.as_deref_mut() {
        Some(list) => {
            lowered = vec![false; list.slice().len()];
            for bibliography in list.slice_mut() {
                for (_, items) in bibliography.slice_mut() {
                    flatten_crossing_inline_containers(items);
                    build_wikidot_block_frames(items, false);
                }
            }
            collect_valid_scope_pairs_with_bibliographies(elements, list)
        }
        None => collect_valid_scope_pairs(elements),
    };

    let mut ordinal = 0;
    let mut active = ActiveScopes {
        close_pairs,
        lifo_close_pairs,
        ..Default::default()
    };
    let mut trim_next_break = false;
    let mut bib = bibliographies.map(|list| BibliographyLowering {
        bibliographies: list,
        lowered: &mut lowered,
    });
    lower_root_sequence(
        elements,
        &valid,
        &mut ordinal,
        &mut active,
        &mut trim_next_break,
        &mut bib,
    );
    drop_empty_inline_scopes(elements);
    drop_vanished_paragraphs(elements);
    splice_wikidot_foster(elements);
    lowered
}

pub(crate) fn lower_wikidot_inline_size_scopes_inline<'t>(
    elements: &mut Vec<Element<'t>>,
) {
    flatten_crossing_inline_containers(elements);
    build_wikidot_block_frames(elements, false);
    let (valid, close_pairs, lifo_close_pairs) = collect_valid_scope_pairs(elements);

    let mut ordinal = 0;
    let mut active = ActiveScopes {
        close_pairs,
        lifo_close_pairs,
        ..Default::default()
    };
    let mut trim_next_break = false;
    lower_sequence(
        elements,
        &valid,
        &mut ordinal,
        &mut active,
        &mut trim_next_break,
        false,
        false,
        &mut None,
    );
    eject_segment_trailing_space(elements);
    drop_empty_inline_scopes(elements);
    drop_vanished_paragraphs(elements);
    splice_wikidot_foster(elements);
}

pub(crate) fn lower_wikidot_bibliography_scopes<'t>(bibliography: &mut Bibliography<'t>) {
    for (_, elements) in bibliography.slice_mut() {
        flatten_crossing_inline_containers(elements);
        build_wikidot_block_frames(elements, false);
    }
    let mut ordinal = 0;
    let mut pending = PendingScopes::default();
    let mut valid = BTreeSet::new();
    for (_, elements) in bibliography.slice() {
        collect_valid_pairs(
            elements,
            &mut ordinal,
            &mut pending,
            &mut valid,
            None,
            false,
        );
    }
    pending.finish_weak(&mut valid);

    let mut active = ActiveScopes {
        close_pairs: pending.pairs,
        lifo_close_pairs: pending.lifo_pairs,
        ..Default::default()
    };
    let mut ordinal = 0;
    let mut trim_next_break = false;
    let mut reopened = Vec::with_capacity(bibliography.slice().len());
    for (_, elements) in bibliography.slice_mut() {
        reopened.push(active.bibliography_wrappers());
        lower_sequence(
            elements,
            &valid,
            &mut ordinal,
            &mut active,
            &mut trim_next_break,
            false,
            false,
            &mut None,
        );
        eject_segment_trailing_space(elements);
        drop_empty_inline_scopes(elements);
        splice_wikidot_foster(elements);
    }
    bibliography.set_reopened_wrappers(reopened);
}

fn splice_wikidot_foster<'t>(elements: &mut Vec<Element<'t>>) {
    for element in elements.iter_mut() {
        visit_children_mut(element, &mut splice_wikidot_foster);
    }
    if !elements.iter().any(|element| {
        matches!(element, Element::Partial(PartialElement::WikidotFoster(_)))
    }) {
        return;
    }
    let mut spliced = Vec::with_capacity(elements.len());
    for element in elements.drain(..) {
        match element {
            Element::Partial(PartialElement::WikidotFoster(tail)) => spliced.extend(tail),
            element => spliced.push(element),
        }
    }
    *elements = spliced;
}

fn collect_valid_scope_pairs(
    elements: &[Element<'_>],
) -> (
    BTreeSet<usize>,
    BTreeMap<usize, usize>,
    BTreeMap<usize, usize>,
) {
    let mut ordinal = 0;
    let mut pending = PendingScopes::default();
    let mut valid = BTreeSet::new();
    collect_valid_pairs(
        elements,
        &mut ordinal,
        &mut pending,
        &mut valid,
        None,
        false,
    );
    pending.finish_weak(&mut valid);

    (valid, pending.pairs, pending.lifo_pairs)
}

fn collect_valid_scope_pairs_with_bibliographies(
    elements: &[Element<'_>],
    bibliographies: &BibliographyList<'_>,
) -> (
    BTreeSet<usize>,
    BTreeMap<usize, usize>,
    BTreeMap<usize, usize>,
) {
    let mut ordinal = 0;
    let mut pending = PendingScopes::default();
    let mut valid = BTreeSet::new();
    collect_valid_pairs(
        elements,
        &mut ordinal,
        &mut pending,
        &mut valid,
        Some(bibliographies),
        false,
    );
    pending.finish_weak(&mut valid);

    (valid, pending.pairs, pending.lifo_pairs)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScopeClass {
    Size,
    Span,
    Format(usize),
    Color,
    Anchor,
}

#[derive(Default)]
struct PendingScopes {
    sizes: VecDeque<PendingScope>,
    spans: VecDeque<PendingScope>,
    formats: [VecDeque<PendingScope>; 7],
    colors: VecDeque<PendingScope>,
    anchors: VecDeque<PendingScope>,
    active_count: usize,
    pairs: BTreeMap<usize, usize>,
    lifo_pairs: BTreeMap<usize, usize>,
    lifo_sizes: Vec<usize>,
    lifo_spans: Vec<usize>,
    weak: Vec<WeakPair>,
}

struct WeakPair {
    class: ScopeClass,
    open: usize,
    close: usize,
}

impl PendingScopes {
    fn stack_mut(&mut self, class: ScopeClass) -> &mut VecDeque<PendingScope> {
        match class {
            ScopeClass::Size => &mut self.sizes,
            ScopeClass::Span => &mut self.spans,
            ScopeClass::Format(index) => &mut self.formats[index],
            ScopeClass::Color => &mut self.colors,
            ScopeClass::Anchor => &mut self.anchors,
        }
    }

    fn open(&mut self, class: ScopeClass, ordinal: &mut usize) {
        let accepted = self.active_count < MAX_ACTIVE_INLINE_SCOPES;
        if accepted {
            self.active_count += 1;
        }
        self.stack_mut(class).push_back(PendingScope {
            ordinal: *ordinal,
            accepted,
        });
        match class {
            ScopeClass::Size => self.lifo_sizes.push(*ordinal),
            ScopeClass::Span => self.lifo_spans.push(*ordinal),
            _ => {}
        }
        *ordinal += 1;
    }

    fn close(
        &mut self,
        class: ScopeClass,
        ordinal: &mut usize,
        valid: &mut BTreeSet<usize>,
        synthetic: bool,
    ) {
        let close = *ordinal;
        *ordinal += 1;
        let lifo_open = match class {
            ScopeClass::Size => self.lifo_sizes.pop(),
            ScopeClass::Span => self.lifo_spans.pop(),
            _ => None,
        };
        if let Some(open) = lifo_open {
            self.lifo_pairs.insert(close, open);
        }
        let stack = self.stack_mut(class);
        let popped = if !synthetic
            && matches!(class, ScopeClass::Span | ScopeClass::Size)
            && !stack.is_empty()
        {
            stack.pop_front()
        } else {
            stack.pop_back()
        };
        if let Some(open) = popped {
            if open.accepted {
                self.active_count -= 1;
                if synthetic {
                    self.weak.push(WeakPair {
                        class,
                        open: open.ordinal,
                        close,
                    });
                } else {
                    valid.insert(open.ordinal);
                    valid.insert(close);
                    self.pairs.insert(close, open.ordinal);
                }
            }
            return;
        }
        if !synthetic
            && let Some(index) = self.weak.iter().rposition(|weak| weak.class == class)
        {
            let weak = self.weak.remove(index);
            valid.insert(weak.open);
            valid.insert(close);
            self.pairs.insert(close, weak.open);
        }
    }

    fn finish_weak(&mut self, valid: &mut BTreeSet<usize>) {
        for weak in std::mem::take(&mut self.weak) {
            valid.insert(weak.open);
            valid.insert(weak.close);
            self.pairs.insert(weak.close, weak.open);
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
        if let Some(kind) = format_kind_literalised_around_flow_block(&element)
            && let Element::Container(container) = &mut element
        {
            let children = mem::take(container.elements_mut());
            output.push(Element::Text(Cow::Borrowed(kind.open_marker())));
            output.extend(children);
            output.push(Element::Text(Cow::Borrowed(kind.close_marker())));
            continue;
        }
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

fn is_wikidot_flow_block(element: &Element<'_>) -> bool {
    match element {
        Element::Container(container) => matches!(
            container.ctype(),
            ContainerType::Div | ContainerType::Note | ContainerType::Align(_)
        ),
        Element::Collapsible { .. }
        | Element::TabView(_)
        | Element::WikidotBlockFrame { .. } => true,
        Element::Partial(
            PartialElement::WikidotBlockOpen(_) | PartialElement::WikidotBlockClose(_),
        ) => true,
        _ => false,
    }
}

fn format_kind_literalised_around_flow_block(
    element: &Element<'_>,
) -> Option<InlineFormatKind> {
    let Element::Container(container) = element else {
        return None;
    };
    let kind = format_kind_of(container.ctype())?;
    container
        .elements()
        .iter()
        .any(is_wikidot_flow_block)
        .then_some(kind)
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
    bib: &mut Option<BibliographyLowering<'_, 't>>,
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
            bib,
        ) {
            previous_scored_close = false;
        }
        if let Element::Container(container) = &mut element
            && container.ctype() == ContainerType::Paragraph
        {
            if paragraph_group.is_none() || !previous_scored_close {
                active.truncate_to_outermost();
            }
            if paragraph_group.is_none()
                && let Some(position) = active.anchor_position()
                && !active.scope_is_pending(position)
            {
                let dying: Vec<usize> = {
                    let positions = active.active_positions();
                    let index = positions
                        .iter()
                        .position(|entry| *entry == position)
                        .unwrap_or(positions.len());
                    positions[index..].to_vec()
                };
                if dying.iter().any(|entry| !active.scope_materialized(*entry)) {
                    output.append(&mut active.copies_from(position));
                }
                active.kill_from(position);
            }
            let paragraph_attributes = container.attributes().clone();
            let starts_in_scored_span = active.has_scored_scope();
            let starts_in_inline_scope = active.has_active_scope();
            let previous_is_own_chunk_block = matches!(
                output.last(),
                Some(Element::List { .. } | Element::Table(_))
            ) || matches!(
                output.last(),
                Some(Element::Container(container))
                    if matches!(
                        container.ctype(),
                        ContainerType::Header(_) | ContainerType::Blockquote
                    )
            );
            let reopens_paragraph_after_separator = starts_in_inline_scope
                && output.last().is_some_and(|element| {
                    matches!(element, Element::HorizontalRule)
                        || is_wikidot_content_separator(element)
                });
            let follows_table = matches!(
                output.last(),
                Some(
                    Element::Table(_)
                        | Element::BibliographyBlock { .. }
                        | Element::List { .. }
                )
            );
            if starts_in_inline_scope
                && !reopens_paragraph_after_separator
                && !follows_table
                && !previous_is_own_chunk_block
                && !output.is_empty()
                && paragraph_group.is_none()
            {
                container.elements_mut().insert(0, Element::LineBreak);
            }
            let continued_from_paragraph =
                paragraph_group.is_some() || previous_is_own_chunk_block;
            active.take_space_eject_floor();
            let effects = lower_sequence(
                container.elements_mut(),
                valid,
                ordinal,
                active,
                trim_next_break,
                false,
                true,
                bib,
            );
            if !effects.scored_span_opened_unwrapped_at_start {
                active.pend_all();
            }
            let joins_previous =
                previous_scored_close || effects.scored_span_opened_at_start;

            if !joins_previous {
                flush_paragraph_group(&mut output, &mut paragraph_group);
                active.drop_deferred_closes();
            }
            let (_, _, group) = paragraph_group.get_or_insert_with(|| {
                (
                    follows_table
                        || reopens_paragraph_after_separator
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
        if align_head_joins_previous_chunk(&output, &element, active)
            && let Some(position) = active.anchor_position()
        {
            active.kill_from(position);
        }
        let scope_before = active.outer_active_position();
        let is_explicit_block_list = matches!(
            &element,
            Element::List {
                wikidot_explicit_block: true,
                ..
            }
        );
        let mut sequence = vec![element];
        lower_sequence(
            &mut sequence,
            valid,
            ordinal,
            active,
            trim_next_break,
            true,
            true,
            bib,
        );
        if is_explicit_block_list {
            fold_scope_break_unit(&mut output, &mut sequence);
        }
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
        bib,
    );
    flush_paragraph_group(&mut output, &mut paragraph_group);
    *elements = output;
}

fn scope_tail_ends_with_break(elements: &[Element<'_>]) -> bool {
    let mut current = elements.last();
    loop {
        match current {
            Some(Element::LineBreak) => return true,
            Some(Element::Container(container))
                if is_inline_scope_ctype(container.ctype()) =>
            {
                current = container.elements().last();
            }
            Some(Element::Color { elements, .. })
            | Some(Element::Anchor { elements, .. }) => {
                current = elements.last();
            }
            _ => return false,
        }
    }
}

fn paragraph_group_has_content(elements: &[Element<'_>]) -> bool {
    elements.iter().any(|element| {
        !matches!(element, Element::Partial(_))
            && !matches!(element, Element::Text(text) if text.is_empty())
    })
}

fn scope_break_unit_chain<'t>(
    element: &Element<'t>,
) -> Option<Vec<(ContainerType, AttributeMap<'t>)>> {
    let mut current = element;
    if let Element::Container(container) = current
        && container.ctype() == ContainerType::Paragraph
    {
        if container.elements().len() != 1 {
            return None;
        }
        current = &container.elements()[0];
    }
    let mut chain = Vec::new();
    loop {
        match current {
            Element::Container(container)
                if matches!(
                    container.ctype(),
                    ContainerType::Span | ContainerType::Size
                ) =>
            {
                chain.push((container.ctype(), container.attributes().clone()));
                if container.elements().len() != 1 {
                    return None;
                }
                current = &container.elements()[0];
            }
            Element::LineBreak => break,
            _ => return None,
        }
    }
    if chain.is_empty() { None } else { Some(chain) }
}

fn amend_scope_tail<'t>(
    elements: &mut Vec<Element<'t>>,
    chain: &[(ContainerType, AttributeMap<'t>)],
) -> bool {
    if chain.is_empty() {
        if elements.iter().all(|element| {
            matches!(
                element,
                Element::Partial(PartialElement::WikidotEmptyInlineOwner)
            )
        }) {
            return false;
        }
        if !matches!(elements.last(), Some(Element::LineBreak)) {
            elements.push(Element::LineBreak);
        }
        return true;
    }
    let Some(Element::Container(container)) = elements.last_mut() else {
        return false;
    };
    if container.ctype() != chain[0].0 || container.attributes() != &chain[0].1 {
        return false;
    }
    amend_scope_tail(container.elements_mut(), &chain[1..])
}

fn fold_scope_break_unit<'t>(
    output: &mut Vec<Element<'t>>,
    sequence: &mut Vec<Element<'t>>,
) {
    let Some(first) = sequence.first() else {
        return;
    };
    let Some(chain) = scope_break_unit_chain(first) else {
        return;
    };
    if let Some(Element::Container(container)) = output.last()
        && container.ctype() == ContainerType::Paragraph
        && !scope_tail_ends_with_break(container.elements())
        && paragraph_group_has_content(container.elements())
    {
        sequence.remove(0);
        return;
    }
    let amended = match output.last_mut() {
        Some(Element::Container(container))
            if container.ctype() == ContainerType::Paragraph =>
        {
            amend_scope_tail(container.elements_mut(), &chain)
        }
        _ => amend_scope_tail(output, &chain),
    };
    if amended {
        sequence.remove(0);
    }
}

fn flush_root_pending<'t>(
    output: &mut Vec<Element<'t>>,
    paragraph_group: &mut Option<(bool, AttributeMap<'t>, Vec<Element<'t>>)>,
    pending: &mut Vec<Element<'t>>,
    valid: &BTreeSet<usize>,
    ordinal: &mut usize,
    active: &mut ActiveScopes<'t>,
    trim_next_break: &mut bool,
    bib: &mut Option<BibliographyLowering<'_, 't>>,
) -> bool {
    if pending.is_empty() {
        return false;
    }
    flush_paragraph_group(output, paragraph_group);
    active.drop_consumed_scopes();
    let scope_before = active.outer_active_position();
    let scope_open_at_start = active.has_active_scope();
    let mut sequence = mem::take(pending);
    lower_sequence(
        &mut sequence,
        valid,
        ordinal,
        active,
        trim_next_break,
        false,
        true,
        bib,
    );
    drop_segment_trailing_space(&mut sequence);
    if scope_open_at_start
        && matches!(
            output.last(),
            Some(Element::Table(_) | Element::BibliographyBlock { .. })
        )
    {
        strip_leading_line_break(&mut sequence);
        if sequence_has_content(&sequence) {
            sequence = vec![Element::Container(Container::new(
                ContainerType::Paragraph,
                sequence,
                AttributeMap::new(),
            ))];
        }
    }
    let scope_continues =
        scope_before.is_some() && scope_before == active.outer_active_position();
    append_root_sequence(output, &mut sequence, scope_continues);
    true
}

fn strip_leading_line_break(elements: &mut Vec<Element<'_>>) -> bool {
    match elements.first_mut() {
        Some(Element::LineBreak) => {
            elements.remove(0);
            true
        }
        Some(Element::Container(container)) => {
            strip_leading_line_break(container.elements_mut())
        }
        Some(Element::Color { elements, .. }) => strip_leading_line_break(elements),
        _ => false,
    }
}

fn sequence_has_content(elements: &[Element<'_>]) -> bool {
    elements.iter().any(|element| match element {
        Element::LineBreak | Element::LineBreaks(_) => false,
        Element::Container(container) => sequence_has_content(container.elements()),
        Element::Color { elements, .. } => sequence_has_content(elements),
        Element::Text(text) => !text.is_empty(),
        _ => true,
    })
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
    strip_trailing_run_before_newline_tail(&mut elements);
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
    pending: &mut PendingScopes,
    valid: &mut BTreeSet<usize>,
    bibliographies: Option<&BibliographyList<'_>>,
    inside_bibliography: bool,
) {
    for element in elements {
        match element {
            Element::Partial(PartialElement::InlineSizeOpen(..)) => {
                pending.open(ScopeClass::Size, ordinal);
            }
            Element::Partial(PartialElement::InlineSizeClose(source)) => {
                pending.close(ScopeClass::Size, ordinal, valid, source.is_empty());
            }
            Element::Partial(PartialElement::InlineSpanOpen(..)) => {
                pending.open(ScopeClass::Span, ordinal);
            }
            Element::Partial(PartialElement::InlineSpanClose(source)) => {
                pending.close(ScopeClass::Span, ordinal, valid, source.is_empty());
            }
            Element::Partial(PartialElement::InlineFormatOpen(kind)) => {
                pending.open(ScopeClass::Format(format_class_index(*kind)), ordinal);
            }
            Element::Partial(PartialElement::InlineFormatClose(kind)) => {
                pending.close(
                    ScopeClass::Format(format_class_index(*kind)),
                    ordinal,
                    valid,
                    false,
                );
            }
            Element::Partial(PartialElement::InlineColorOpen(..)) => {
                pending.open(ScopeClass::Color, ordinal);
            }
            Element::Partial(PartialElement::InlineColorClose) => {
                pending.close(ScopeClass::Color, ordinal, valid, false);
            }
            Element::Partial(PartialElement::InlineAnchorOpen(..)) => {
                pending.open(ScopeClass::Anchor, ordinal);
            }
            Element::Partial(PartialElement::InlineAnchorClose(source)) => {
                pending.close(ScopeClass::Anchor, ordinal, valid, source.is_empty());
            }
            Element::BibliographyBlock { index, .. }
                if !inside_bibliography && bibliographies.is_some() =>
            {
                if let Some(bibliography) = bibliographies
                    .expect("checked above")
                    .get_bibliography_opt(*index)
                {
                    for (_, items) in bibliography.slice() {
                        collect_valid_pairs(
                            items,
                            ordinal,
                            pending,
                            valid,
                            bibliographies,
                            true,
                        );
                    }
                }
            }
            _ => {
                let mut visit = |children: &[Element<'_>]| {
                    collect_valid_pairs(
                        children,
                        ordinal,
                        pending,
                        valid,
                        bibliographies,
                        inside_bibliography,
                    )
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
    segment: bool,
    bib: &mut Option<BibliographyLowering<'_, 't>>,
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
        if let Element::Partial(PartialElement::InlineSizeOpen(style, head_source)) =
            element
        {
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
            } else if !head_source.is_empty() {
                run.push(Element::Text(head_source));
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
            if valid.contains(ordinal)
                && resolved.is_some_and(|position| active.has_anchor_above(position))
            {
                *ordinal += 1;
                continue;
            }
            let current = *ordinal;
            *ordinal += 1;
            if valid.contains(&current) {
                active.note_close_fired(current);
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
                let (last_is_space, rest_ends_with_space) = match output.split_last() {
                    Some((Element::Text(text), rest)) if text == " " => {
                        (true, elements_end_with_space(rest))
                    }
                    _ => (false, false),
                };
                let keep_empty_space = empty
                    && last_is_space
                    && !next_starts_with_space
                    && !matches!(
                        source.peek(),
                        None | Some(Element::LineBreak | Element::LineBreaks(_))
                    )
                    && !rest_ends_with_space;
                if !keep_empty_space {
                    trim_trailing_space(&mut output);
                }
                if resolved.is_some() || active.has_active_scope() {
                    active.tidy_close(InlineTag::Span);
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
        if active.has_active_scope()
            && let Element::FootnoteBlock {
                wikidot_wrappers, ..
            } = &mut element
        {
            *wikidot_wrappers = active.bibliography_wrappers();
        }
        if let Element::Partial(PartialElement::InlineSpanOpen(
            mut attributes,
            head_source,
            body_separator,
        )) = element
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
                if body_separator {
                    run.push(text!(" "));
                    flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
                }
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
            } else if !scored && !head_source.is_empty() {
                run.push(Element::Text(head_source));
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
            let empty_id_attributes = if empty && !scored {
                resolved.and_then(|position| match &active.scopes[position].wrapper {
                    ScopeWrapper::Container {
                        ctype: ContainerType::Span,
                        attributes,
                    } if attributes.get().contains_key("id") => Some(attributes.clone()),
                    _ => None,
                })
            } else {
                None
            };
            let trailing_space =
                !scored && matches!(run.last(), Some(Element::Text(text)) if text == " ");
            let trailing_break =
                !scored && matches!(run.last(), Some(Element::LineBreak));
            let next_starts_with_space = matches!(
                source.peek(),
                Some(Element::Text(text)) if text.starts_with(' ')
            );
            if valid.contains(ordinal)
                && resolved.is_some_and(|position| active.has_anchor_above(position))
            {
                *ordinal += 1;
                continue;
            }
            active.note_close_fired(current);
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
            let (last_is_space, rest_ends_with_space) = match output.split_last() {
                Some((Element::Text(text), rest)) if text == " " => {
                    (true, elements_end_with_space(rest))
                }
                _ => (false, false),
            };
            let keep_empty_space = empty
                && !scored
                && last_is_space
                && !next_starts_with_space
                && !matches!(
                    source.peek(),
                    None | Some(Element::LineBreak | Element::LineBreaks(_))
                )
                && !rest_ends_with_space;
            if empty && !scored && !preserve_empty_owner_space && !keep_empty_space {
                trim_trailing_space(&mut output);
            }
            if !(preserve_empty_owner_space || empty && scored) && !keep_empty_space {
                trim_trailing_space(&mut output);
            }
            if resolved.is_some() || active.has_active_scope() {
                active.tidy_close(InlineTag::Span);
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
            if trailing_break
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
            if let Some(attributes) = empty_id_attributes {
                output.push(Element::Container(Container::new(
                    ContainerType::Span,
                    Vec::new(),
                    attributes,
                )));
            }
            continue;
        }
        if let Element::Partial(PartialElement::InlineAnchorOpen(
            mut attributes,
            target,
        )) = element
        {
            let at_start = output.is_empty() && run.is_empty();
            let scored = attributes.remove(WIKIDOT_SCORE_ANCHOR_ATTRIBUTE).is_some();
            let scored_starts_next_physical_line = attributes
                .remove(WIKIDOT_SCORE_ANCHOR_UNWRAPPED_ATTRIBUTE)
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
                if let Some(position) = active.anchor_position() {
                    if !active.scope_has_content(position)
                        && run.iter().all(|element| {
                            matches!(element, Element::Text(text) if text.trim().is_empty())
                        })
                    {
                        run.clear();
                        run.push(Element::Partial(
                            PartialElement::WikidotEmptyInlineOwner,
                        ));
                    }
                    flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
                    let anchor_trailing_space = matches!(
                        output.last(),
                        Some(Element::Anchor { elements: inner, .. })
                            if matches!(inner.last(), Some(Element::Text(text)) if text == " ")
                    );
                    if anchor_trailing_space {
                        trim_trailing_space(&mut output);
                        output.push(text!(" "));
                    }
                    active.commit_close_no_reopen(position);
                }
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
                    ScopeKind::Anchor,
                    ScopeWrapper::Anchor { attributes, target },
                    scored,
                );
                if scored {
                    effects.scored_span_opened_at_start |= at_start;
                    effects.scored_span_opened_unwrapped_at_start |=
                        at_start && scored_starts_next_physical_line;
                    *trim_next_break = true;
                }
            }
            continue;
        }
        if let Element::Partial(PartialElement::InlineAnchorClose(close_source)) = element
        {
            let resolved = active.peek_close(InlineTag::Anchor);
            let scored =
                resolved.is_some_and(|position| active.scope_is_scored(position));
            let empty = !resolved
                .is_some_and(|position| active.scope_has_content(position))
                && run.iter().all(|element| {
                    matches!(element, Element::Text(text) if text.trim().is_empty())
                });
            let trailing_space =
                !scored && matches!(run.last(), Some(Element::Text(text)) if text == " ");
            let next_starts_with_space = matches!(
                source.peek(),
                Some(Element::Text(text)) if text.starts_with(' ')
            );
            if trailing_space && resolved.is_none() && valid.contains(ordinal) {
                run.pop();
            }
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
            let scope_materialized =
                resolved.is_some_and(|position| active.scope_has_content(position));
            let current = *ordinal;
            *ordinal += 1;
            if valid.contains(&current) {
                if !(preserve_empty_owner_space || empty && scored) && scope_materialized
                {
                    trim_trailing_space(&mut output);
                }
                if let Some(position) = resolved {
                    if !active.scope_has_content(position)
                        || !active.scope_materialized(position)
                    {
                        run.push(Element::Partial(
                            PartialElement::WikidotEmptyInlineOwner,
                        ));
                        flush_run(
                            &mut output,
                            &mut run,
                            active,
                            &mut last_run_outer_scope,
                        );
                    }
                    active.commit_close_no_reopen(position);
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
            } else {
                run.push(Element::Text(close_source));
            }
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
                    ScopeKind::Format(kind),
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
                    ScopeKind::Color,
                    current,
                    source.peek(),
                );
            } else {
                run.push(text!("##"));
            }
            continue;
        }
        if let Element::BibliographyBlock { index, .. } = &element
            && let Some(context) = bib.as_mut()
            && !context.lowered.get(*index).copied().unwrap_or(true)
        {
            flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
            lower_bibliography_block(
                context,
                *index,
                valid,
                ordinal,
                active,
                trim_next_break,
            );
            output.push(element);
            last_run_outer_scope.reset();
            continue;
        }
        let list_scope_wrappers = if root_structural_boundary
            && active.has_active_scope()
            && matches!(
                &element,
                Element::List {
                    wikidot_explicit_block: true,
                    ..
                }
            ) {
            let wrappers = active.scope_wrappers();
            if wrappers.is_empty() {
                None
            } else {
                Some(wrappers)
            }
        } else {
            None
        };
        if active.has_active_scope()
            && element.paragraph_safe()
            && contains_inline_scope_control(&element)
            && inline_scope_controls_are_self_contained(&element)
        {
            let mut nested_active = ActiveScopes::default();
            let root_parts = inline_scope_parts(&element);
            if let Some((tag, wrapper)) = &root_parts {
                nested_active.istack.clone_from(&active.istack);
                nested_active.istack_push_inline(*tag, wrapper.clone());
            }
            let lowered = lower_children(
                &mut element,
                valid,
                ordinal,
                &mut nested_active,
                trim_next_break,
                false,
                bib,
            );
            if lowered {
                if let Some((tag, wrapper)) = &root_parts {
                    let stolen = nested_active.has_active_scope();
                    if stolen {
                        nested_active.tidy_close(*tag);
                    }
                    nested_active.istack.pop();
                    active.istack = std::mem::take(&mut nested_active.istack);
                    if stolen {
                        flush_run(
                            &mut output,
                            &mut run,
                            active,
                            &mut last_run_outer_scope,
                        );
                        active.push_with_origin(
                            scope_kind_of_wrapper(wrapper),
                            wrapper.clone(),
                            false,
                            *ordinal,
                        );
                        let children = match &mut element {
                            Element::Container(data) => {
                                std::mem::take(data.elements_mut())
                            }
                            Element::Color { elements, .. } => std::mem::take(elements),
                            _ => Vec::new(),
                        };
                        run.extend(children);
                        run_has_content = true;
                        flush_run(
                            &mut output,
                            &mut run,
                            active,
                            &mut last_run_outer_scope,
                        );
                        let leftovers: Vec<(ScopeKind, ScopeWrapper<'t>, bool, usize)> =
                            nested_active
                                .iter_active_rev()
                                .map(|scope| {
                                    (
                                        scope.kind,
                                        scope.wrapper.clone(),
                                        scope.scored,
                                        scope.origin,
                                    )
                                })
                                .collect();
                        for (kind, leftover_wrapper, scored, origin) in
                            leftovers.into_iter().rev()
                        {
                            active.push_with_origin(
                                kind,
                                leftover_wrapper,
                                scored,
                                origin,
                            );
                        }
                        continue;
                    }
                }
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
            if inline_scope_parts(&element).is_some() {
                active.apply_subtree_istack(&element);
            }
            run.push(element);
            run_has_content = true;
        } else if has_lowerable_children(&element)
            || matches!(element, Element::Code(_) | Element::Math { .. })
        {
            let list_legacy = matches!(element, Element::List { .. });
            if list_legacy {
                flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
                lower_children(
                    &mut element,
                    valid,
                    ordinal,
                    active,
                    trim_next_break,
                    segment,
                    bib,
                );
            } else {
                let block_boundary = is_block_boundary(&element);
                if block_boundary && active.has_active_scope() {
                    strip_trailing_space_from_vec(&mut run);
                }
                flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
                let is_paragraph = matches!(
                    &element,
                    Element::Container(container)
                        if container.ctype() == ContainerType::Paragraph
                );
                if block_boundary {
                    if !segment {
                        active.drop_anchor_scopes();
                    } else if !matches!(element, Element::Table(_)) {
                        if align_head_joins_previous_chunk(&output, &element, active)
                            && let Some(position) = active.anchor_position()
                        {
                            active.kill_from(position);
                        }
                        close_live_anchor(
                            &mut output,
                            &mut run,
                            active,
                            &mut last_run_outer_scope,
                        );
                        active.enter_block();
                        if let Element::Collapsible {
                            wikidot_block_open_elements,
                            ..
                        } = &mut element
                            && active.has_anchor_scope()
                        {
                            *wikidot_block_open_elements = active.nested_empty_copies();
                            if let Some(position) = active.anchor_position() {
                                active.kill_from(position);
                            }
                        }
                        if let Element::Code(code_block) = &mut element
                            && active.has_anchor_scope()
                        {
                            code_block.wikidot_block_open_elements =
                                active.nested_empty_copies();
                            if let Some(position) = active.anchor_position() {
                                active.kill_from(position);
                            }
                        }
                        if let Element::Math {
                            wikidot_block_open_elements,
                            ..
                        } = &mut element
                            && active.has_anchor_scope()
                        {
                            *wikidot_block_open_elements = active.nested_empty_copies();
                            if let Some(position) = active.anchor_position() {
                                active.kill_from(position);
                            }
                        }
                    }
                } else if is_paragraph && segment {
                    close_live_anchor(
                        &mut output,
                        &mut run,
                        active,
                        &mut last_run_outer_scope,
                    );
                    active.pend_all();
                }
                if let Element::WikidotBlockFrame {
                    collapsible_link: Some(link),
                    ..
                } = &mut element
                {
                    link.spans = active.wikidot_label_spans();
                }
                let watermark = if block_boundary {
                    Some(active.begin_watch())
                } else {
                    None
                };
                let root_parts = inline_scope_parts(&element);
                if let Some((tag, wrapper)) = &root_parts {
                    active.istack_push_inline(*tag, wrapper.clone());
                }
                lower_children(
                    &mut element,
                    valid,
                    ordinal,
                    active,
                    trim_next_break,
                    segment,
                    bib,
                );
                if root_parts.is_some() {
                    active.istack.pop();
                }
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
                    last_run_outer_scope.reset();
                } else if active_after == 0 {
                    output.extend(wrap_scope_elements(&scopes, vec![Element::LineBreak]));
                    last_run_outer_scope.reset();
                }
            }
            if !is_empty_paragraph(&element) && !is_vanished_empty_div(&element) {
                output.push(element);
                last_run_outer_scope.reset();
            }
        } else {
            flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
            if !is_vanished_empty_div(&element) {
                output.push(element);
            }
            last_run_outer_scope.reset();
        }
    }
    flush_run(&mut output, &mut run, active, &mut last_run_outer_scope);
    if segment && active.any_live_unmaterialized() {
        emit_segment_copies(&mut output, &mut run, active, &mut last_run_outer_scope);
    }
    *elements = output;
    effects
}

fn lower_bibliography_block<'t>(
    bib: &mut BibliographyLowering<'_, 't>,
    index: usize,
    valid: &BTreeSet<usize>,
    ordinal: &mut usize,
    active: &mut ActiveScopes<'t>,
    trim_next_break: &mut bool,
) {
    bib.lowered[index] = true;
    active.truncate_to_outermost();
    let bibliography = &mut bib.bibliographies.slice_mut()[index];
    bibliography.set_title_wrappers(active.bibliography_wrappers());
    let mut reopened = Vec::with_capacity(bibliography.slice().len());
    for (_, elements) in bibliography.slice_mut() {
        reopened.push(active.bibliography_wrappers());
        lower_sequence(
            elements,
            valid,
            ordinal,
            active,
            trim_next_break,
            false,
            false,
            &mut None,
        );
        eject_segment_trailing_space(elements);
        drop_empty_inline_scopes(elements);
        splice_wikidot_foster(elements);
    }
    bibliography.set_reopened_wrappers(reopened);
}

fn scope_kind_of_wrapper(wrapper: &ScopeWrapper<'_>) -> ScopeKind {
    match wrapper {
        ScopeWrapper::Container { ctype, .. } => match ctype {
            ContainerType::Size => ScopeKind::Size,
            ContainerType::Span => ScopeKind::Span,
            _ => ScopeKind::Format(
                format_kind_of(*ctype).unwrap_or(InlineFormatKind::Bold),
            ),
        },
        ScopeWrapper::Color { .. } => ScopeKind::Color,
        ScopeWrapper::Anchor { .. } => ScopeKind::Anchor,
    }
}

fn inline_scope_parts<'t>(
    element: &Element<'t>,
) -> Option<(InlineTag, ScopeWrapper<'t>)> {
    match element {
        Element::Container(container) if is_inline_scope_ctype(container.ctype()) => {
            Some((
                inline_tag_for_ctype(container.ctype()),
                ScopeWrapper::Container {
                    ctype: container.ctype(),
                    attributes: container.attributes().clone(),
                },
            ))
        }
        Element::Color {
            color, background, ..
        } => Some((
            InlineTag::Span,
            ScopeWrapper::Color {
                color: color.clone(),
                background: *background,
            },
        )),
        _ => None,
    }
}

fn subtree_children<'a, 't>(element: &'a Element<'t>) -> Option<&'a [Element<'t>]> {
    match element {
        Element::Container(container) => Some(container.elements()),
        Element::Color { elements, .. } => Some(elements),
        _ => None,
    }
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
        (
            Element::Anchor {
                target: x_target,
                attributes: x_attributes,
                ..
            },
            Element::Anchor {
                target: y_target,
                attributes: y_attributes,
                ..
            },
        ) => x_target == y_target && x_attributes == y_attributes,
        _ => false,
    }
}

fn scope_children_mut<'a, 't>(element: &'a mut Element<'t>) -> &'a mut Vec<Element<'t>> {
    match element {
        Element::Container(container) => container.elements_mut(),
        Element::Color { elements, .. } => elements,
        Element::Anchor { elements, .. } => elements,
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
        (
            Element::Anchor {
                target: previous_target,
                attributes: previous_attributes,
                elements: previous_elements,
            },
            Element::Anchor {
                target: next_target,
                attributes: next_attributes,
                elements: next_elements,
            },
        ) if previous_target == &next_target
            && previous_attributes == &next_attributes =>
        {
            previous_elements.extend(next_elements);
            true
        }
        _ => false,
    }
}

fn elements_render_empty(elements: &[Element<'_>]) -> bool {
    elements.iter().all(|element| {
        matches!(
            element,
            Element::Partial(PartialElement::WikidotEmptyInlineOwner)
        )
    })
}

fn scope_wrapper_is_empty(element: &Element<'_>) -> bool {
    match element {
        Element::Container(container) => elements_render_empty(container.elements()),
        Element::Color { elements, .. } => elements_render_empty(elements),
        Element::Anchor { elements, .. } => elements_render_empty(elements),
        _ => false,
    }
}

fn empty_inline_scope_vanishes(element: &Element<'_>) -> bool {
    match element {
        Element::Container(container) => {
            is_inline_scope_ctype(container.ctype())
                && elements_render_empty(container.elements())
                && !(container.ctype() == ContainerType::Span
                    && container.attributes().get().contains_key("id"))
        }
        Element::Color { elements, .. } => elements_render_empty(elements),
        _ => false,
    }
}

fn drop_empty_inline_scopes(elements: &mut Vec<Element>) {
    hoist_closed_container_leading_spaces(elements);
    for element in elements.iter_mut() {
        visit_children_mut(element, &mut |children| drop_empty_inline_scopes(children));
        strip_leading_run_for_block_parent(element);
    }
    hoist_closed_container_leading_spaces(elements);
    let mut index = 0;
    while index < elements.len() {
        if empty_inline_scope_vanishes(&elements[index])
            || emptied_block_vanishes(&elements[index])
        {
            let dropped_color = matches!(&elements[index], Element::Color { .. });
            let dropped_empty_mono = matches!(
                &elements[index],
                Element::Container(container)
                    if container.ctype() == ContainerType::Monospace
                        && container.elements().is_empty()
                        && container
                            .attributes()
                            .get()
                            .contains_key(WIKIDOT_WS_MONO_VANISH_MARKER)
            );
            let forward_deletes = (dropped_color || dropped_empty_mono)
                && vanished_scope_deletes_preceding_run(elements, index);
            elements.remove(index);
            if (dropped_color || dropped_empty_mono) && index > 0 {
                let preceding_is_text = matches!(&elements[index - 1], Element::Text(_));
                if !preceding_is_text || forward_deletes {
                    strip_vanish_run_tail(&mut elements[index - 1]);
                    if matches!(
                        &elements[index - 1],
                        Element::Text(text) if text.is_empty()
                    ) || empty_inline_scope_vanishes(&elements[index - 1])
                    {
                        elements.remove(index - 1);
                        index -= 1;
                    }
                }
            }
            merge_space_across_vanished(elements, index);
        } else if index + 1 < elements.len()
            && is_separator_run(&elements[index])
            && is_separator_run(&elements[index + 1])
        {
            elements.remove(index + 1);
            elements[index] = Element::Text(Cow::Borrowed(" "));
        } else {
            index += 1;
        }
    }
}

fn is_separator_run(element: &Element) -> bool {
    matches!(element, Element::Text(text) if !text.is_empty() && text.chars().all(|c| c == ' ' || c == '\t'))
}

fn hoist_closed_container_leading_spaces(elements: &mut Vec<Element>) {
    let mut index = 0;
    while index < elements.len() {
        let hoist = matches!(
            &elements[index],
            Element::Container(container)
                if matches!(
                    container.ctype(),
                    ContainerType::Bold
                        | ContainerType::Italics
                        | ContainerType::Monospace
                        | ContainerType::Strikethrough
                        | ContainerType::Superscript
                        | ContainerType::Subscript
                        | ContainerType::Underline
                ) && container.elements().first().is_some_and(is_separator_run)
        );
        if hoist {
            let Element::Container(container) = &mut elements[index] else {
                unreachable!("hoist check requires a container");
            };
            container.elements_mut().remove(0);
            elements.insert(index, Element::Text(Cow::Borrowed(" ")));
        }
        index += 1;
    }
}

fn strip_vanish_run_tail(element: &mut Element) {
    match element {
        Element::Text(_) => strip_trailing_separators(element),
        Element::Container(container)
            if matches!(container.ctype(), ContainerType::Span | ContainerType::Size) =>
        {
            strip_scope_run_tail(container.elements_mut());
        }
        Element::Color { elements, .. } | Element::Anchor { elements, .. } => {
            strip_scope_run_tail(elements);
        }
        _ => {}
    }
}

fn strip_scope_run_tail(elements: &mut Vec<Element>) {
    if let Some(last) = elements.last_mut() {
        strip_vanish_run_tail(last);
    }
    while elements.last().is_some_and(|element| {
        matches!(element, Element::Text(text) if text.is_empty())
            || empty_inline_scope_vanishes(element)
    }) {
        elements.pop();
    }
}

fn vanished_scope_deletes_preceding_run(elements: &[Element], index: usize) -> bool {
    let mut next = index + 1;
    while next < elements.len()
        && matches!(
            &elements[next],
            Element::Color { elements, .. } if elements_render_empty(elements)
        )
    {
        next += 1;
    }
    match elements.get(next) {
        None => true,
        Some(Element::LineBreak | Element::LineBreaks(_)) => true,
        Some(Element::Text(text)) if text.chars().all(|c| c == '\n' || c == '\r') => true,
        Some(Element::Text(text)) => text.chars().all(|c| c == ' ' || c == '\t'),
        Some(_) => false,
    }
}

fn strip_leading_run_for_block_parent(element: &mut Element) {
    match element {
        Element::Container(container)
            if container.ctype() == ContainerType::Paragraph =>
        {
            strip_leading_separators(container.elements_mut());
        }
        Element::Table(table) => {
            for row in &mut table.rows {
                for cell in &mut row.cells {
                    strip_leading_separators(&mut cell.elements);
                }
            }
        }
        _ => {}
    }
}

fn strip_leading_separators(elements: &mut Vec<Element>) {
    let Some(Element::Text(text)) = elements.first() else {
        return;
    };
    let stripped = text.trim_start_matches([' ', '\t']);
    if stripped.len() == text.len() {
        return;
    }
    if stripped.is_empty() {
        elements.remove(0);
    } else {
        elements[0] = Element::Text(Cow::Owned(stripped.to_owned()));
    }
}

fn strip_trailing_separators(element: &mut Element) {
    if let Element::Text(text) = element {
        let keep = text.trim_end_matches([' ', '\t']).len();
        text.to_mut().truncate(keep);
    }
}

fn strip_trailing_run_before_newline_tail(elements: &mut Vec<Element>) {
    let mut index = elements.len();
    while index > 0 {
        match &elements[index - 1] {
            Element::Text(text) if text.chars().all(|c| c == '\n' || c == '\r') => {
                index -= 1;
            }
            Element::Text(_) => {
                strip_trailing_separators(&mut elements[index - 1]);
                if matches!(
                    &elements[index - 1],
                    Element::Text(text) if text.is_empty()
                ) {
                    elements.remove(index - 1);
                }
                return;
            }
            _ => return,
        }
    }
}

fn drop_vanished_paragraphs(elements: &mut Vec<Element>) {
    for element in elements.iter_mut() {
        visit_children_mut(element, &mut |children| drop_vanished_paragraphs(children));
        if let Element::List { items, .. } = element {
            items.retain(|item| !vanished_list_item(item));
        }
    }
    elements.retain(|element| !vanished_block(element));
}

fn all_whitespace_text(elements: &[Element]) -> bool {
    elements.iter().all(|child| {
        matches!(child, Element::Text(text) if text.chars().all(|c| matches!(c as u32, 32 | 9 | 10 | 13)))
    })
}

fn vanished_block(element: &Element) -> bool {
    match element {
        Element::Container(container) => {
            matches!(
                container.ctype(),
                ContainerType::Paragraph | ContainerType::Blockquote
            ) && container.attributes().get().is_empty()
                && all_whitespace_text(container.elements())
        }
        Element::List { items, .. } => items.is_empty(),
        _ => false,
    }
}

fn vanished_list_item(item: &ListItem) -> bool {
    match item {
        ListItem::Elements {
            attributes,
            elements,
        } => attributes.get().is_empty() && all_whitespace_text(elements),
        ListItem::SubList { .. } => false,
    }
}

fn emptied_block_vanishes(element: &Element<'_>) -> bool {
    matches!(
        element,
        Element::Container(container)
            if matches!(
                container.ctype(),
                ContainerType::Paragraph | ContainerType::Div
            ) && container.attributes().get().is_empty()
                && container.elements().is_empty()
    ) || matches!(
        element,
        Element::WikidotBlockFrame {
            attributes,
            children,
            collapsible_link: None,
            ..
        } if attributes.get().is_empty() && children.is_empty()
    )
}

fn merge_space_across_vanished(elements: &mut Vec<Element>, index: usize) {
    if index == 0 || index >= elements.len() {
        return;
    }
    let previous_ends_space = matches!(
        &elements[index - 1],
        Element::Text(text) if text.ends_with(' ')
    );
    let next_starts_space = matches!(
        &elements[index],
        Element::Text(text) if text.starts_with(' ')
    );
    if !previous_ends_space || !next_starts_space {
        return;
    }
    let Element::Text(text) = &mut elements[index] else {
        return;
    };
    let stripped: String = text.chars().skip(1).collect();
    if stripped.is_empty() {
        elements.remove(index);
    } else {
        *text = Cow::Owned(stripped);
    }
}

fn strip_scope_trailing_space(element: &mut Element<'_>) -> bool {
    match element {
        Element::Container(container) if is_inline_scope_ctype(container.ctype()) => {
            strip_trailing_space_from_vec(container.elements_mut())
        }
        Element::Color { elements, .. } => strip_trailing_space_from_vec(elements),
        Element::Anchor { elements, .. } => strip_trailing_space_from_vec(elements),
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

fn drop_segment_trailing_space(elements: &mut Vec<Element<'_>>) {
    if let Some(Element::Anchor {
        elements: inner, ..
    }) = elements.last_mut()
    {
        strip_trailing_space_from_vec(inner);
        return;
    }
    let stripped = match elements.last_mut() {
        Some(element) => strip_scope_trailing_space(element),
        None => false,
    };
    if stripped && elements.last().is_some_and(scope_wrapper_is_empty) {
        elements.pop();
    }
    strip_trailing_run_before_newline_tail(elements);
}

fn eject_segment_trailing_space(elements: &mut Vec<Element<'_>>) {
    if let Some(Element::Anchor {
        elements: inner, ..
    }) = elements.last_mut()
    {
        strip_trailing_space_from_vec(inner);
        return;
    }
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
                | ContainerType::Note
                | ContainerType::Align(_)
        ),
        Element::Collapsible { .. }
        | Element::List { .. }
        | Element::Table(_)
        | Element::TabView(_)
        | Element::WikidotBlockFrame { .. }
        | Element::Code(_)
        | Element::Math { .. }
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
    close_kind: ScopeKind,
    close_ordinal: usize,
    next: Option<&Element<'t>>,
) {
    let next_starts_with_space =
        matches!(next, Some(Element::Text(text)) if text.starts_with(' '));
    let has_next = next.is_some();
    let resolved = active.resolve_close(close_ordinal, tag);
    if resolved.is_some_and(|position| active.has_anchor_above(position)) {
        return;
    }
    let empty = !resolved.is_some_and(|position| active.scope_has_content(position))
        && run.is_empty();
    let trailing_space = matches!(run.last(), Some(Element::Text(text)) if text == " ");
    if trailing_space {
        run.pop();
    }
    flush_close_run(output, run, active, last_run_outer_scope, run_has_content);
    let crossed = resolved.is_some_and(|position| {
        active.active_tail != Some(position) || active.scopes[position].kind != close_kind
    });
    let keep_trailing_space = crossed
        && !next_starts_with_space
        && output.last().is_some_and(element_has_real_content);
    if !keep_trailing_space {
        trim_trailing_space(output);
    }
    if resolved.is_some() {
        active.tidy_close(tag);
    }
    if trailing_space && !next_starts_with_space && !empty && has_next {
        run.push(text!(" "));
        *run_has_content = true;
    }
}

fn emit_segment_copies<'t>(
    output: &mut Vec<Element<'t>>,
    run: &mut Vec<Element<'t>>,
    active: &mut ActiveScopes<'t>,
    last_run_outer_scope: &mut RunMergeState,
) {
    if active.any_unmaterialized() {
        run.push(Element::Partial(PartialElement::WikidotEmptyInlineOwner));
        flush_run(output, run, active, last_run_outer_scope);
    }
}

fn close_live_anchor<'t>(
    output: &mut Vec<Element<'t>>,
    run: &mut Vec<Element<'t>>,
    active: &mut ActiveScopes<'t>,
    last_run_outer_scope: &mut RunMergeState,
) {
    let Some(position) = active.anchor_position() else {
        return;
    };
    if active.scope_is_pending(position) {
        return;
    }
    emit_segment_copies(output, run, active, last_run_outer_scope);
    active.kill_from(position);
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

fn element_has_real_content(element: &Element<'_>) -> bool {
    match element {
        Element::Text(text) => !text.trim().is_empty(),
        Element::Container(container) => {
            container.elements().iter().any(element_has_real_content)
        }
        Element::Color { elements, .. } => elements.iter().any(element_has_real_content),
        Element::Anchor { elements, .. } => elements.iter().any(element_has_real_content),
        Element::Partial(_) => false,
        _ => true,
    }
}

fn trim_trailing_space(elements: &mut Vec<Element<'_>>) {
    let Some(last) = elements.last_mut() else {
        return;
    };
    if let Element::Container(container) = last
        && is_inline_scope_ctype(container.ctype())
    {
        let was_empty_mono = container.elements().is_empty()
            && container.ctype() == ContainerType::Monospace
            && container
                .attributes()
                .get()
                .contains_key(WIKIDOT_WS_MONO_VANISH_MARKER);
        trim_trailing_space(container.elements_mut());
        if container.elements().is_empty() {
            elements.pop();
            if was_empty_mono && let Some(previous) = elements.last_mut() {
                strip_vanish_run_tail(previous);
            }
        }
        return;
    }
    if let Element::Color {
        elements: inner, ..
    } = last
    {
        let was_empty = inner.is_empty();
        trim_trailing_space(inner);
        if inner.is_empty() {
            elements.pop();
            if was_empty && let Some(previous) = elements.last_mut() {
                strip_vanish_run_tail(previous);
            }
        }
        return;
    }
    if let Element::Anchor {
        elements: inner, ..
    } = last
    {
        trim_trailing_space(inner);
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

fn align_head_joins_previous_chunk(
    output: &[Element<'_>],
    element: &Element<'_>,
    active: &ActiveScopes<'_>,
) -> bool {
    if !matches!(
        element,
        Element::Container(container)
            if matches!(container.ctype(), ContainerType::Align(_))
    ) {
        return false;
    }
    let Some(position) = active.anchor_position() else {
        return false;
    };
    if !active.scope_is_pending(position) {
        return false;
    }
    let Some(Element::Container(paragraph)) = output.last() else {
        return false;
    };
    if paragraph.ctype() != ContainerType::Paragraph {
        return false;
    }
    let Some(Element::Anchor { elements, .. }) = paragraph.elements().last() else {
        return false;
    };
    matches!(elements.as_slice(), [Element::LineBreak])
}

fn is_vanished_empty_div(element: &Element<'_>) -> bool {
    matches!(
        element,
        Element::Container(container)
            if container.ctype() == ContainerType::Div
                && container.attributes().get().is_empty()
                && container.elements().is_empty()
    ) || matches!(
        element,
        Element::WikidotBlockFrame {
            attributes,
            children,
            collapsible_link: None,
            ..
        } if attributes.get().is_empty() && children.is_empty()
    )
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
            PartialElement::InlineSizeOpen(..)
                | PartialElement::InlineSizeClose(_)
                | PartialElement::InlineSpanOpen(..)
                | PartialElement::InlineSpanClose(_)
                | PartialElement::InlineFormatOpen(_)
                | PartialElement::InlineFormatClose(_)
                | PartialElement::InlineColorOpen(..)
                | PartialElement::InlineColorClose
                | PartialElement::InlineAnchorOpen(..)
                | PartialElement::InlineAnchorClose(_)
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
    fn visit(element: &Element<'_>, counts: &mut [i32; 11]) -> bool {
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

    let mut counts = [0; 11];
    visit(element, &mut counts) && counts.iter().all(|count| *count == 0)
}

fn partial_scope_class(partial: &PartialElement<'_>) -> Option<(usize, bool)> {
    match partial {
        PartialElement::InlineSizeOpen(..) => Some((0, true)),
        PartialElement::InlineSizeClose(_) => Some((0, false)),
        PartialElement::InlineSpanOpen(..) => Some((1, true)),
        PartialElement::InlineSpanClose(_) => Some((1, false)),
        PartialElement::InlineFormatOpen(kind) => {
            Some((2 + format_class_index(*kind), true))
        }
        PartialElement::InlineFormatClose(kind) => {
            Some((2 + format_class_index(*kind), false))
        }
        PartialElement::InlineColorOpen(..) => Some((9, true)),
        PartialElement::InlineColorClose => Some((9, false)),
        PartialElement::InlineAnchorOpen(..) => Some((10, true)),
        PartialElement::InlineAnchorClose(_) => Some((10, false)),
        _ => None,
    }
}

fn lower_cell_like<'t>(
    elements: &mut Vec<Element<'t>>,
    valid: &BTreeSet<usize>,
    ordinal: &mut usize,
    active: &mut ActiveScopes<'t>,
    bib: &mut Option<BibliographyLowering<'_, 't>>,
) {
    let start = *ordinal;
    let mut cell_active = ActiveScopes {
        close_pairs: active.close_pairs.clone(),
        lifo_close_pairs: active.lifo_close_pairs.clone(),
        cell_local: true,
        ..Default::default()
    };
    let mut cell_trim = false;
    lower_sequence(
        elements,
        valid,
        ordinal,
        &mut cell_active,
        &mut cell_trim,
        false,
        false,
        bib,
    );
    strip_trailing_space_from_vec(elements);
    let end = *ordinal;
    let retire: Vec<usize> = active
        .close_pairs
        .iter()
        .filter(|(close, open)| **open < start && start <= **close && **close < end)
        .filter_map(|(_, open)| active.open_instances.get(open).copied())
        .collect();
    for lineage in retire {
        active.retire_scope_by_lineage(lineage);
    }
}

fn lower_children<'t>(
    element: &mut Element<'t>,
    valid: &BTreeSet<usize>,
    ordinal: &mut usize,
    active: &mut ActiveScopes<'t>,
    trim_next_break: &mut bool,
    segment: bool,
    bib: &mut Option<BibliographyLowering<'_, 't>>,
) -> bool {
    if let Element::Table(table) = element {
        for row in &mut table.rows {
            for cell in &mut row.cells {
                lower_cell_like(&mut cell.elements, valid, ordinal, active, bib);
            }
        }
        return true;
    }

    if let Element::Partial(PartialElement::WikidotFoster(tail)) = element {
        lower_cell_like(tail, valid, ordinal, active, bib);
        return true;
    }

    if let Element::Container(container) = element
        && matches!(container.ctype(), ContainerType::Header(_))
    {
        let scope_crosses_heading = active.has_active_scope();
        let entry_positions = active.active_positions();
        lower_sequence(
            container.elements_mut(),
            valid,
            ordinal,
            active,
            trim_next_break,
            false,
            segment,
            bib,
        );
        if segment {
            active.pend_all();
            let mut kill = Vec::new();
            for position in active.active_positions() {
                let scope = &active.scopes[position];
                if scope.wrapper.tag() == InlineTag::Span {
                    let opened_inside = !entry_positions.contains(&position);
                    let produced = opened_inside
                        || tree_contains_wrapper(container.elements(), &scope.wrapper);
                    if produced {
                        kill.push(position);
                    }
                }
            }
            for position in kill {
                active.kill_scope(position);
            }
        }
        if scope_crosses_heading {
            insert_wikidot_heading_label_span(container.elements_mut());
            *trim_next_break = true;
        }
        return true;
    }

    let eject = matches!(element, Element::Collapsible { .. })
        || matches!(element, Element::WikidotBlockFrame { .. })
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
    let truncate_at_entry = matches!(element, Element::List { .. });
    let mut visit = |children: &mut Vec<Element<'t>>| {
        lowered = true;
        if truncate_at_entry {
            active.truncate_to_outermost();
        }
        lower_sequence(
            children,
            valid,
            ordinal,
            active,
            trim_next_break,
            false,
            segment,
            bib,
        );
        if segment {
            active.pend_all();
        }
        if eject {
            eject_segment_trailing_space(children);
            strip_trailing_space_from_vec(children);
        }
    };
    visit_children_mut(element, &mut visit);
    lowered
}

fn is_scope_wrapper_element(element: &Element<'_>) -> bool {
    match element {
        Element::Container(container) => is_inline_scope_ctype(container.ctype()),
        Element::Color { .. } | Element::Anchor { .. } => true,
        _ => false,
    }
}

fn tree_contains_wrapper(elements: &[Element<'_>], wrapper: &ScopeWrapper<'_>) -> bool {
    let probe = wrapper.wrap(Vec::new());
    for element in elements {
        if same_scope_wrapper(element, &probe) {
            return true;
        }
        if let Some(children) = subtree_children(element)
            && tree_contains_wrapper(children, wrapper)
        {
            return true;
        }
    }
    false
}

fn insert_wikidot_heading_label_span(elements: &mut Vec<Element<'_>>) {
    if elements.len() == 1 && is_scope_wrapper_element(&elements[0]) {
        insert_wikidot_heading_label_span(scope_children_mut(&mut elements[0]));
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
        Element::WikidotBlockFrame { children, .. } => visit(children),
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
            PartialElement::WikidotFoster(elements) => visit(elements),
            PartialElement::Tab(tab) => visit(&tab.elements),
            PartialElement::RubyText(ruby_text) => visit(&ruby_text.elements),
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

fn is_wikidot_block_partial(element: &Element<'_>) -> bool {
    matches!(
        element,
        Element::Partial(
            PartialElement::WikidotBlockOpen(_) | PartialElement::WikidotBlockClose(_)
        )
    )
}

fn wikidot_frame_attributes<'a>(pairs: &[(&'a str, &'a str)]) -> AttributeMap<'a> {
    let mut attributes = AttributeMap::new();
    for (key, value) in pairs {
        attributes.insert(key, Cow::Borrowed(value));
    }
    attributes
}

struct WikidotFrameBuild<'t> {
    attributes: AttributeMap<'t>,
    children: Vec<Element<'t>>,
    scored_div: bool,
}

fn unwrap_scored_frame_edges<'t>(children: &mut Vec<Element<'t>>) {
    if children.is_empty() {
        return;
    }
    // Mirror legacy scored-div normalization (edge paragraphs unwrap, middle
    // ones stay); the flat path already carries real breaks, so no newline
    // separators are added.
    let unwrap_at = |children: &mut Vec<Element<'t>>, index: usize| {
        let is_paragraph = matches!(
            children.get(index),
            Some(Element::Container(container))
                if container.ctype() == ContainerType::Paragraph
        );
        if is_paragraph {
            let inner = match &mut children[index] {
                Element::Container(container) => mem::take(container.elements_mut()),
                _ => unreachable!("checked above"),
            };
            children.splice(index..=index, inner);
        }
    };
    let last = children.len() - 1;
    unwrap_at(children, last);
    if last > 0 {
        unwrap_at(children, 0);
    }
}

struct WikidotCollapsibleClose<'t> {
    show_bottom: bool,
    hide_text: Option<Cow<'t, str>>,
}

struct WikidotFrameAssembly<'t> {
    output: Vec<Element<'t>>,
    stack: Vec<WikidotFrameBuild<'t>>,
    collapsible_closes: BTreeMap<usize, WikidotCollapsibleClose<'t>>,
    at_root: bool,
}

impl<'t> WikidotFrameAssembly<'t> {
    fn new(at_root: bool) -> Self {
        WikidotFrameAssembly {
            output: Vec::new(),
            stack: Vec::new(),
            collapsible_closes: BTreeMap::new(),
            at_root,
        }
    }

    fn top(&mut self) -> &mut Vec<Element<'t>> {
        match self.stack.last_mut() {
            Some(frame) => &mut frame.children,
            None => &mut self.output,
        }
    }

    fn push(&mut self, element: Element<'t>) {
        match element {
            Element::Partial(PartialElement::WikidotBlockOpen(shell)) => {
                self.open_shell(shell);
            }
            Element::Partial(PartialElement::WikidotBlockClose(kind)) => {
                self.close_kind(kind);
            }
            element => {
                self.top().push(element);
            }
        }
    }

    fn open_shell(&mut self, shell: WikidotBlockShell<'t>) {
        match shell {
            WikidotBlockShell::Div {
                attributes, scored, ..
            } => {
                self.stack.push(WikidotFrameBuild {
                    attributes,
                    children: Vec::new(),
                    scored_div: scored,
                });
            }
            WikidotBlockShell::Note { .. } => {
                self.stack.push(WikidotFrameBuild {
                    attributes: wikidot_frame_attributes(&[("class", "wiki-note")]),
                    children: Vec::new(),
                    scored_div: false,
                });
            }
            WikidotBlockShell::Align { alignment, .. } => {
                self.stack.push(WikidotFrameBuild {
                    attributes: wikidot_frame_attributes(&[(
                        "style",
                        alignment.wd_html_style(),
                    )]),
                    children: Vec::new(),
                    scored_div: false,
                });
            }
            WikidotBlockShell::Collapsible {
                head,
                start_open,
                show_text,
                hide_text,
                show_top,
                show_bottom,
            } => {
                self.collapsible_closes.insert(
                    head,
                    WikidotCollapsibleClose {
                        show_bottom,
                        hide_text: hide_text.clone(),
                    },
                );
                self.stack.push(WikidotFrameBuild {
                    attributes: wikidot_frame_attributes(&[(
                        "class",
                        "collapsible-block",
                    )]),
                    children: Vec::new(),
                    scored_div: false,
                });
                let folded_hidden = start_open;
                self.push_shell_link(
                    "collapsible-block-folded",
                    folded_hidden,
                    show_text,
                    true,
                );
                let mut unfolded_attributes =
                    wikidot_frame_attributes(&[("class", "collapsible-block-unfolded")]);
                if !start_open {
                    unfolded_attributes.insert("style", Cow::Borrowed("display:none"));
                }
                self.stack.push(WikidotFrameBuild {
                    attributes: unfolded_attributes,
                    children: Vec::new(),
                    scored_div: false,
                });
                if show_top {
                    self.push_shell_link(
                        "collapsible-block-unfolded-link",
                        false,
                        hide_text,
                        false,
                    );
                }
                self.stack.push(WikidotFrameBuild {
                    attributes: wikidot_frame_attributes(&[(
                        "class",
                        "collapsible-block-content",
                    )]),
                    children: Vec::new(),
                    scored_div: false,
                });
            }
        }
    }

    fn push_shell_link(
        &mut self,
        class: &'t str,
        hidden: bool,
        text: Option<Cow<'t, str>>,
        show: bool,
    ) {
        let mut attributes = wikidot_frame_attributes(&[("class", class)]);
        if hidden {
            attributes.insert("style", Cow::Borrowed("display:none"));
        }
        let link = WikidotFrameLink {
            text,
            show,
            spans: Vec::new(),
        };
        self.top().push(Element::WikidotBlockFrame {
            attributes,
            children: Vec::new(),
            collapsible_link: Some(link),
            omit_close: false,
        });
    }

    fn close_kind(&mut self, kind: WikidotBlockCloseKind) {
        match kind {
            WikidotBlockCloseKind::Div { scored, .. } => {
                if scored {
                    self.unwrap_paragraph_before_close();
                } else {
                    self.strip_paragraph_before_close();
                }
                self.pop_frame();
            }
            WikidotBlockCloseKind::Collapsible { head } => {
                self.strip_paragraph_before_close();
                self.pop_frame();
                let close = self.collapsible_closes.remove(&head);
                let (show_bottom, hide_text) = close
                    .map(|close| (close.show_bottom, close.hide_text))
                    .unwrap_or((false, None));
                if show_bottom {
                    self.push_shell_link(
                        "collapsible-block-unfolded-link",
                        false,
                        hide_text,
                        false,
                    );
                }
                self.pop_frame();
                self.pop_frame();
            }
            WikidotBlockCloseKind::Note { .. } | WikidotBlockCloseKind::Align { .. } => {
                self.strip_paragraph_before_close();
                self.pop_frame();
            }
        }
    }

    fn strip_paragraph_before_close(&mut self) {
        let emptied = match self.top().last_mut() {
            Some(Element::Container(container))
                if container.ctype() == ContainerType::Paragraph =>
            {
                let elements = container.elements_mut();
                if matches!(elements.last(), Some(Element::LineBreak)) {
                    elements.pop();
                }
                elements.is_empty()
            }
            _ => return,
        };
        if emptied {
            self.top().pop();
        }
    }

    fn unwrap_paragraph_before_close(&mut self) {
        let is_paragraph = matches!(
            self.top().last(),
            Some(Element::Container(container))
                if container.ctype() == ContainerType::Paragraph
        );
        if !is_paragraph {
            return;
        }
        let paragraph = self.top().pop();
        if let Some(Element::Container(container)) = paragraph {
            let children = container.elements().to_vec();
            self.top().extend(children);
        }
    }

    fn pop_frame(&mut self) {
        let Some(mut frame) = self.stack.pop() else {
            return;
        };
        if frame.scored_div {
            unwrap_scored_frame_edges(&mut frame.children);
        }
        if matches!(frame.children.last(), Some(Element::LineBreak)) {
            frame.children.pop();
        }
        self.top().push(Element::WikidotBlockFrame {
            attributes: frame.attributes,
            children: frame.children,
            collapsible_link: None,
            omit_close: false,
        });
    }

    fn finish(mut self) -> Vec<Element<'t>> {
        while let Some(frame) = self.stack.pop() {
            let mut frame = frame;
            if frame.scored_div {
                unwrap_scored_frame_edges(&mut frame.children);
            }
            if !self.at_root {
                while matches!(frame.children.last(), Some(Element::LineBreak)) {
                    frame.children.pop();
                }
            }
            let omit_close = self.at_root;
            self.top().push(Element::WikidotBlockFrame {
                attributes: frame.attributes,
                children: frame.children,
                collapsible_link: None,
                omit_close,
            });
        }
        self.output
    }
}

fn build_wikidot_block_frames<'t>(elements: &mut Vec<Element<'t>>, at_root: bool) {
    for element in elements.iter_mut() {
        visit_children_mut(element, &mut |children| {
            build_wikidot_block_frames(children, false);
        });
    }
    if !elements.iter().any(is_wikidot_block_partial) {
        return;
    }
    let mut assembly = WikidotFrameAssembly::new(at_root);
    for element in mem::take(elements) {
        assembly.push(element);
    }
    *elements = assembly.finish();
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
                    Element::Partial(PartialElement::InlineSizeOpen(
                        cow!("170%"),
                        cow!(""),
                    )),
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
            elements.push(Element::Partial(PartialElement::InlineSizeOpen(
                cow!("font-size: larger;"),
                cow!(""),
            )));
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
                Element::Partial(PartialElement::InlineSizeOpen(
                    cow!("font-size:0%;"),
                    cow!(""),
                )),
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
                    PartialElement::InlineSizeOpen(cow!("font-size: larger;"), cow!("")),
                )),
                ScopeKind::Span => elements.push(Element::Partial(
                    PartialElement::InlineSpanOpen(AttributeMap::new(), cow!(""), false),
                )),
                ScopeKind::Format(_) | ScopeKind::Color | ScopeKind::Anchor => {
                    unreachable!()
                }
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
                ScopeKind::Format(_) | ScopeKind::Color | ScopeKind::Anchor => {
                    unreachable!()
                }
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
            elements.push(Element::Partial(PartialElement::InlineSizeOpen(
                cow!("font-size: larger;"),
                cow!(""),
            )));
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
