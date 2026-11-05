//! Typed delayed input for runtime-generated ListPages values.
//!
//! Generated values are represented out of band from authored text. They
//! therefore cannot terminate or create authored syntax while FTML is parsing
//! the List-mode stream.

mod seam;
#[cfg(test)]
mod seam_equivalence;
mod toc;

use crate::data::{PageInfo, PageRef};
use crate::parsing::{ParseError, decode_semicolon_entities, parse};
#[cfg(feature = "html")]
use crate::render::html::{HtmlOutput, HtmlRender};
#[cfg(feature = "html")]
use crate::render::{PageExistenceResolver, Render};
use crate::settings::{WikitextMode, WikitextSettings};
use crate::tokenizer::{Tokenization, tokenize_delayed_segments};
use crate::tree::{
    AnchorTarget, AttributeMap, Container, Element, FloatAlignment, ImageSource,
    LinkLabel, LinkLocation, LinkType, ListItem, PartialElement, SyntaxTree,
    run_on_bounded_tree_stack, tree_requires_bounded_tree_stack,
};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::Range;

/// Stable identifier for one generated value in a delayed stream.
#[derive(
    Serialize, Deserialize, Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord,
)]
pub struct SlotId(u32);

impl SlotId {
    #[inline]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }
}

/// The closed set of generated List-mode semantics.
#[derive(Serialize, Deserialize, Debug, Copy, Clone, Hash, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GeneratedKind {
    PageLink,
    TagLinks,
}

/// Provenance of an ordinary text segment.
#[derive(Serialize, Deserialize, Debug, Copy, Clone, Hash, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TextOrigin {
    Authored,
    RuntimeScalar,
    /// Source-identical text retained by a runtime substitution pass.
    ///
    /// Unlike `RuntimeScalar`, these bytes still participate in ordinary
    /// source grammar (for example, as a link target). Their provenance
    /// remains observable so block owners whose live behavior recovers around
    /// delayed values can preserve the complete owner as literal text.
    RuntimeLiteral,
}

/// One generated occurrence in the original source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedInput {
    pub source_range: Range<usize>,
    pub id: SlotId,
    pub kind: GeneratedKind,
    pub occurrence: u32,
}

/// A typed segment of one delayed List-mode input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputSegment {
    Text {
        source_range: Range<usize>,
        origin: TextOrigin,
    },
    Generated(GeneratedInput),
}

impl InputSegment {
    #[inline]
    pub fn text(source_range: Range<usize>, origin: TextOrigin) -> Self {
        Self::Text {
            source_range,
            origin,
        }
    }

    #[inline]
    pub fn generated(input: GeneratedInput) -> Self {
        Self::Generated(input)
    }

    fn source_range(&self) -> &Range<usize> {
        match self {
            Self::Text { source_range, .. } => source_range,
            Self::Generated(input) => &input.source_range,
        }
    }
}

/// Validated segmented source for one List-mode parse.
#[derive(Debug, Clone)]
pub struct DelayedInput<'t> {
    segments: Vec<InputSegment>,
    tokenization: Tokenization<'t>,
}

impl<'t> DelayedInput<'t> {
    pub fn new(
        source: &'t str,
        segments: Vec<InputSegment>,
    ) -> Result<Self, DelayedError> {
        if segments.is_empty() {
            return Err(DelayedError::InvalidSegments);
        }
        let mut cursor = 0;
        let mut occurrences = BTreeSet::new();
        for segment in &segments {
            let range = segment.source_range();
            if range.start != cursor
                || range.start > range.end
                || range.end > source.len()
                || !source.is_char_boundary(range.start)
                || !source.is_char_boundary(range.end)
            {
                return Err(DelayedError::InvalidSegments);
            }
            if let InputSegment::Generated(input) = segment
                && (!occurrences.insert((input.id, input.occurrence))
                    || input.source_range.is_empty())
            {
                return Err(DelayedError::InvalidSegments);
            }
            cursor = range.end;
        }
        if cursor != source.len() {
            return Err(DelayedError::InvalidSegments);
        }
        let tokenization = tokenize_delayed_segments(source, &segments);
        Ok(Self {
            segments,
            tokenization,
        })
    }

    #[inline]
    pub(crate) fn segments(&self) -> &[InputSegment] {
        &self.segments
    }

    #[inline]
    pub(crate) fn tokenization(&self) -> &Tokenization<'t> {
        &self.tokenization
    }
}

/// One resolved tag identity. FTML, rather than the caller, owns its URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTagRef<'a> {
    pub tag: Cow<'a, str>,
}

/// Closed generated values accepted by FTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeneratedValue<'a> {
    PageLink {
        page: PageRef,
        label: Cow<'a, str>,
    },
    TagLinks {
        tags: Cow<'a, [ResolvedTagRef<'a>]>,
        separator: Cow<'a, str>,
    },
}

impl GeneratedValue<'_> {
    fn kind(&self) -> GeneratedKind {
        match self {
            Self::PageLink { .. } => GeneratedKind::PageLink,
            Self::TagLinks { .. } => GeneratedKind::TagLinks,
        }
    }
}

/// Candidate runtime bindings. They are sealed atomically against a parsed
/// schema before any syntax tree is returned for rendering.
#[derive(Debug, Clone)]
pub struct SlotBindings<'a> {
    values: BTreeMap<SlotId, GeneratedValue<'a>>,
}

impl<'a> SlotBindings<'a> {
    pub fn new(values: Vec<(SlotId, GeneratedValue<'a>)>) -> Result<Self, DelayedError> {
        let expected = values.len();
        let values = values.into_iter().collect::<BTreeMap<_, _>>();
        if values.len() != expected {
            return Err(DelayedError::DuplicateBinding);
        }
        Ok(Self { values })
    }

    pub fn empty() -> Self {
        Self {
            values: BTreeMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct DelayedElement<'t> {
    node: DelayedNode<'t>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
enum DelayedNode<'t> {
    Active {
        id: SlotId,
        kind: GeneratedKind,
    },
    Suppressed {
        slots: Vec<(SlotId, GeneratedKind)>,
    },
    TypographyBoundary,
    RuntimeText(Cow<'t, str>),
    Raw {
        atoms: Vec<RecoveryAtom<'t>>,
    },
    Shell {
        atoms: Vec<RecoveryAtom<'t>>,
    },
    PageConditionalRecovery {
        id: SlotId,
        false_branch: Cow<'t, str>,
    },
    TagExternalLabel {
        id: SlotId,
        url: Cow<'t, str>,
    },
    TagImage(Box<DelayedTagImage<'t>>),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct DelayedTagImage<'t> {
    source: ImageSource<'t>,
    link: Option<LinkLocation<'t>>,
    target: Option<AnchorTarget>,
    alignment: Option<FloatAlignment>,
    attributes: AttributeMap<'t>,
    attribute: GeneratedImageAttribute,
    suffix: Cow<'t, str>,
    id: SlotId,
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum GeneratedImageAttribute {
    Alt,
    Link,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
enum RecoveryAtom<'t> {
    Source(Cow<'t, str>),
    LegacySource { id: SlotId, kind: GeneratedKind },
    Active { id: SlotId, kind: GeneratedKind },
    LineBreak,
}

impl<'t> DelayedElement<'t> {
    pub(crate) fn active(id: SlotId, kind: GeneratedKind) -> Self {
        Self {
            node: DelayedNode::Active { id, kind },
        }
    }

    pub(crate) fn suppressed(generated: &[GeneratedInput]) -> Self {
        Self {
            node: DelayedNode::Suppressed {
                slots: generated.iter().map(|slot| (slot.id, slot.kind)).collect(),
            },
        }
    }

    pub(crate) fn runtime_text(text: &'t str) -> Self {
        Self {
            node: DelayedNode::RuntimeText(Cow::Borrowed(text)),
        }
    }

    pub(crate) fn typography_boundary() -> Self {
        Self {
            node: DelayedNode::TypographyBoundary,
        }
    }

    pub(crate) fn is_typography_boundary(&self) -> bool {
        matches!(&self.node, DelayedNode::TypographyBoundary)
    }

    pub(crate) fn is_suppression_seam(&self) -> bool {
        matches!(
            &self.node,
            DelayedNode::Suppressed { .. } | DelayedNode::TypographyBoundary
        )
    }

    pub(crate) fn is_empty_suppression(&self) -> bool {
        matches!(&self.node, DelayedNode::Suppressed { slots } if slots.is_empty())
    }

    pub(crate) fn raw(
        source: &'t str,
        content_range: Range<usize>,
        generated: &[GeneratedInput],
    ) -> Self {
        let mut atoms = Vec::with_capacity(generated.len() * 2 + 1);
        let mut cursor = content_range.start;
        for slot in generated {
            debug_assert!(slot.source_range.start >= cursor);
            debug_assert!(slot.source_range.end <= content_range.end);
            if cursor < slot.source_range.start {
                atoms.push(RecoveryAtom::Source(decode_semicolon_entities(
                    &source[cursor..slot.source_range.start],
                )));
            }
            atoms.push(RecoveryAtom::LegacySource {
                id: slot.id,
                kind: slot.kind,
            });
            cursor = slot.source_range.end;
        }
        if cursor < content_range.end {
            atoms.push(RecoveryAtom::Source(decode_semicolon_entities(
                &source[cursor..content_range.end],
            )));
        }
        Self {
            node: DelayedNode::Raw { atoms },
        }
    }

    pub(crate) fn tag_image(
        source: ImageSource<'t>,
        link: Option<LinkLocation<'t>>,
        target: Option<AnchorTarget>,
        alignment: Option<FloatAlignment>,
        attributes: AttributeMap<'t>,
        attribute: GeneratedImageAttribute,
        suffix: &'t str,
        id: SlotId,
    ) -> Self {
        Self {
            node: DelayedNode::TagImage(Box::new(DelayedTagImage {
                source,
                link,
                target,
                alignment,
                attributes,
                attribute,
                suffix: Cow::Borrowed(suffix),
                id,
            })),
        }
    }

    pub(crate) fn shell(
        source: &'t str,
        owner_range: Range<usize>,
        generated: &[GeneratedInput],
    ) -> Self {
        let mut atoms = Vec::with_capacity(generated.len() * 3 + 1);
        let mut cursor = owner_range.start;
        for slot in generated {
            debug_assert!(slot.source_range.start >= cursor);
            debug_assert!(slot.source_range.end <= owner_range.end);
            append_shell_source(&mut atoms, &source[cursor..slot.source_range.start]);
            atoms.push(RecoveryAtom::Active {
                id: slot.id,
                kind: slot.kind,
            });
            cursor = slot.source_range.end;
        }
        append_shell_source(&mut atoms, &source[cursor..owner_range.end]);
        Self {
            node: DelayedNode::Shell { atoms },
        }
    }

    pub(crate) fn page_conditional_recovery(id: SlotId, false_branch: &'t str) -> Self {
        Self {
            node: DelayedNode::PageConditionalRecovery {
                id,
                false_branch: decode_semicolon_entities(false_branch),
            },
        }
    }

    pub(crate) fn tag_external_label(id: SlotId, url: Cow<'t, str>) -> Self {
        Self {
            node: DelayedNode::TagExternalLabel { id, url },
        }
    }

    pub(crate) fn image_alignment(&self) -> Option<Option<FloatAlignment>> {
        match &self.node {
            DelayedNode::TagImage(image) => Some(image.alignment),
            _ => None,
        }
    }

    fn occurrence_count(&self) -> usize {
        match &self.node {
            DelayedNode::Active { .. }
            | DelayedNode::PageConditionalRecovery { .. }
            | DelayedNode::TagExternalLabel { .. }
            | DelayedNode::TagImage(_) => 1,
            DelayedNode::Suppressed { slots } => slots.len(),
            DelayedNode::TypographyBoundary | DelayedNode::RuntimeText(_) => 0,
            DelayedNode::Raw { atoms } | DelayedNode::Shell { atoms } => atoms
                .iter()
                .filter(|atom| {
                    matches!(
                        atom,
                        RecoveryAtom::LegacySource { .. } | RecoveryAtom::Active { .. }
                    )
                })
                .count(),
        }
    }

    fn active_tag_links_are_empty(
        &self,
        bindings: &BTreeMap<SlotId, GeneratedValue<'_>>,
    ) -> bool {
        let DelayedNode::Active {
            id,
            kind: GeneratedKind::TagLinks,
        } = self.node
        else {
            return false;
        };
        matches!(
            bindings.get(&id),
            Some(GeneratedValue::TagLinks { tags, .. }) if tags.is_empty()
        )
    }

    pub(crate) fn to_owned(&self) -> DelayedElement<'static> {
        let node = match &self.node {
            DelayedNode::Active { id, kind } => DelayedNode::Active {
                id: *id,
                kind: *kind,
            },
            DelayedNode::Suppressed { slots } => DelayedNode::Suppressed {
                slots: slots.clone(),
            },
            DelayedNode::TypographyBoundary => DelayedNode::TypographyBoundary,
            DelayedNode::RuntimeText(text) => {
                DelayedNode::RuntimeText(Cow::Owned(text.to_string()))
            }
            DelayedNode::Raw { atoms } => DelayedNode::Raw {
                atoms: owned_recovery_atoms(atoms),
            },
            DelayedNode::Shell { atoms } => DelayedNode::Shell {
                atoms: owned_recovery_atoms(atoms),
            },
            DelayedNode::PageConditionalRecovery { id, false_branch } => {
                DelayedNode::PageConditionalRecovery {
                    id: *id,
                    false_branch: Cow::Owned(false_branch.to_string()),
                }
            }
            DelayedNode::TagExternalLabel { id, url } => DelayedNode::TagExternalLabel {
                id: *id,
                url: Cow::Owned(url.to_string()),
            },
            DelayedNode::TagImage(image) => {
                DelayedNode::TagImage(Box::new(DelayedTagImage {
                    source: image.source.to_owned(),
                    link: image.link.as_ref().map(LinkLocation::to_owned),
                    target: image.target,
                    alignment: image.alignment,
                    attributes: image.attributes.to_owned(),
                    attribute: image.attribute,
                    suffix: Cow::Owned(image.suffix.to_string()),
                    id: image.id,
                }))
            }
        };
        DelayedElement { node }
    }
}

/// Parsed delayed stream. Its syntax tree cannot be rendered until bindings
/// match the complete generated schema.
#[derive(Debug, Clone)]
pub struct DelayedSyntaxTree<'t> {
    tree: SyntaxTree<'t>,
    schema: BTreeMap<SlotId, GeneratedKind>,
    expected_occurrences: usize,
    delayed_toc_entries: Vec<usize>,
    errors: Vec<ParseError>,
    wikidot_typography: bool,
}

impl<'t> DelayedSyntaxTree<'t> {
    /// Parser recovery diagnostics remain observable but do not by themselves
    /// reject a delayed tree. Wikidot recovery is part of the List-mode
    /// compatibility contract. Binding still fails atomically unless every
    /// generated occurrence was captured by a closed delayed owner.
    pub fn errors(&self) -> &[ParseError] {
        &self.errors
    }

    pub fn bind(
        &self,
        bindings: &SlotBindings<'_>,
    ) -> Result<BoundDelayedSyntaxTree, DelayedError> {
        if bindings.values.len() != self.schema.len() {
            return Err(DelayedError::BindingSchemaMismatch);
        }
        for (id, kind) in &self.schema {
            let Some(value) = bindings.values.get(id) else {
                return Err(DelayedError::BindingSchemaMismatch);
            };
            if value.kind() != *kind {
                return Err(DelayedError::BindingSchemaMismatch);
            }
        }

        let tree = self.tree.to_owned();
        if tree_requires_bounded_tree_stack(&tree) {
            return run_on_bounded_tree_stack("ftml-delayed-bind", move || {
                bind_delayed_tree(
                    tree,
                    bindings,
                    self.expected_occurrences,
                    self.wikidot_typography,
                    self.delayed_toc_entries.clone(),
                )
            });
        }
        bind_delayed_tree(
            tree,
            bindings,
            self.expected_occurrences,
            self.wikidot_typography,
            self.delayed_toc_entries.clone(),
        )
    }
}

fn bind_delayed_tree(
    mut tree: SyntaxTree<'static>,
    bindings: &SlotBindings<'_>,
    expected_occurrences: usize,
    wikidot_typography: bool,
    delayed_toc_entries: Vec<usize>,
) -> Result<BoundDelayedSyntaxTree, DelayedError> {
    let mut resolved_occurrences = 0usize;
    resolve_bound_suppressions(
        &mut tree.elements,
        &bindings.values,
        &mut resolved_occurrences,
        wikidot_typography,
    )?;
    resolve_bound_suppressions(
        &mut tree.table_of_contents,
        &bindings.values,
        &mut resolved_occurrences,
        wikidot_typography,
    )?;
    for footnote in &mut tree.footnotes {
        resolve_bound_suppressions(
            footnote,
            &bindings.values,
            &mut resolved_occurrences,
            wikidot_typography,
        )?;
    }
    for bibliography in tree.bibliographies.slice_mut() {
        for (_, elements) in bibliography.slice_mut() {
            resolve_bound_suppressions(
                elements,
                &bindings.values,
                &mut resolved_occurrences,
                wikidot_typography,
            )?;
        }
    }
    resolve_elements(
        &mut tree.elements,
        &bindings.values,
        &mut resolved_occurrences,
    )?;
    resolve_elements(
        &mut tree.table_of_contents,
        &bindings.values,
        &mut resolved_occurrences,
    )?;
    for footnote in &mut tree.footnotes {
        resolve_elements(footnote, &bindings.values, &mut resolved_occurrences)?;
    }
    for bibliography in tree.bibliographies.slice_mut() {
        for (_, elements) in bibliography.slice_mut() {
            resolve_elements(elements, &bindings.values, &mut resolved_occurrences)?;
        }
    }
    if resolved_occurrences != expected_occurrences
        || elements_contain_delayed(&tree.elements)
        || elements_contain_delayed(&tree.table_of_contents)
        || tree
            .footnotes
            .iter()
            .any(|footnote| elements_contain_delayed(footnote))
        || tree.bibliographies.slice().iter().any(|bibliography| {
            bibliography
                .slice()
                .iter()
                .any(|(_, elements)| elements_contain_delayed(elements))
        })
    {
        return Err(DelayedError::UnresolvedGeneratedOwner);
    }
    Ok(BoundDelayedSyntaxTree {
        tree,
        delayed_toc_entries,
    })
}

/// Bound delayed syntax. Construction is possible only through schema sealing.
#[derive(Debug, Clone)]
#[cfg_attr(not(feature = "html"), allow(dead_code))]
pub struct BoundDelayedSyntaxTree {
    tree: SyntaxTree<'static>,
    delayed_toc_entries: Vec<usize>,
}

#[cfg(feature = "html")]
impl BoundDelayedSyntaxTree {
    pub fn render_html(
        &self,
        page_info: &PageInfo,
        settings: &WikitextSettings,
    ) -> SealedFragment {
        let mut tree = self.tree.clone();
        toc::bind_labels(&mut tree, &self.delayed_toc_entries, page_info, settings);
        SealedFragment {
            output: HtmlRender.render(&tree, page_info, settings),
            html_blocks: tree.html_blocks,
        }
    }

    pub fn render_html_with_page_existence(
        &self,
        page_info: &PageInfo,
        settings: &WikitextSettings,
        page_existence: &dyn PageExistenceResolver,
    ) -> SealedFragment {
        let mut tree = self.tree.clone();
        toc::bind_labels(&mut tree, &self.delayed_toc_entries, page_info, settings);
        SealedFragment {
            output: HtmlRender.render_with_page_existence(
                &tree,
                page_info,
                settings,
                page_existence,
            ),
            html_blocks: tree.html_blocks,
        }
    }
}

/// FTML-rendered output with no public constructor.
#[cfg(feature = "html")]
#[derive(Debug)]
pub struct SealedFragment {
    output: HtmlOutput,
    html_blocks: Vec<Cow<'static, str>>,
}

#[cfg(feature = "html")]
impl SealedFragment {
    pub fn body(&self) -> &str {
        &self.output.body
    }

    pub fn html_blocks(&self) -> &[Cow<'static, str>] {
        &self.html_blocks
    }

    pub fn resource_requirements(
        &self,
    ) -> &[crate::render::html::HtmlResourceRequirement] {
        &self.output.resource_requirements
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelayedError {
    InvalidSegments,
    DuplicateBinding,
    BindingSchemaMismatch,
    UnresolvedGeneratedOwner,
    WrongMode,
}

impl fmt::Display for DelayedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for DelayedError {}

pub fn parse_delayed_list<'t>(
    input: &'t DelayedInput<'t>,
    page_info: &'t PageInfo<'t>,
    settings: &'t WikitextSettings,
) -> Result<DelayedSyntaxTree<'t>, DelayedError> {
    if settings.mode != WikitextMode::List {
        return Err(DelayedError::WrongMode);
    }
    let (mut tree, errors) = parse(input.tokenization(), page_info, settings).into();
    if settings.list_pages_inline {
        let elements = std::mem::take(&mut tree.elements);
        tree.elements = elements
            .into_iter()
            .flat_map(|element| match element {
                Element::Container(container)
                    if container.ctype() == crate::tree::ContainerType::Paragraph =>
                {
                    container.into()
                }
                element => vec![element],
            })
            .collect();
    }
    let mut schema = BTreeMap::new();
    let mut expected_occurrences = 0usize;
    for segment in input.segments() {
        if let InputSegment::Generated(generated) = segment {
            expected_occurrences += 1;
            if let Some(previous) = schema.insert(generated.id, generated.kind)
                && previous != generated.kind
            {
                return Err(DelayedError::InvalidSegments);
            }
        }
    }
    let delayed_toc_entries = toc::entry_indices(&tree);
    Ok(DelayedSyntaxTree {
        tree,
        schema,
        expected_occurrences,
        delayed_toc_entries,
        errors,
        wikidot_typography: settings.layout.legacy(),
    })
}

fn resolve_elements(
    elements: &mut Vec<Element<'static>>,
    bindings: &BTreeMap<SlotId, GeneratedValue<'_>>,
    resolved_occurrences: &mut usize,
) -> Result<(), DelayedError> {
    let mut resolved = Vec::with_capacity(elements.len());
    let element_count = elements.len();
    for (index, mut element) in elements.drain(..).enumerate() {
        match &mut element {
            Element::Delayed(delayed) => {
                *resolved_occurrences += delayed.occurrence_count();
                let trim_preceding_line_end_space = delayed
                    .active_tag_links_are_empty(bindings)
                    && index + 1 == element_count;
                let replacement = resolve_delayed(delayed, bindings)?;
                if trim_preceding_line_end_space && replacement.is_empty() {
                    trim_trailing_ascii_space(&mut resolved);
                }
                resolved.extend(replacement);
                continue;
            }
            Element::Container(container) => {
                let contained_delayed = elements_contain_delayed(container.elements());
                resolve_elements(
                    container.elements_mut(),
                    bindings,
                    resolved_occurrences,
                )?;
                if contained_delayed
                    && container.ctype() == crate::tree::ContainerType::Paragraph
                    && container.elements().is_empty()
                {
                    continue;
                }
            }
            Element::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        resolve_elements(
                            &mut cell.elements,
                            bindings,
                            resolved_occurrences,
                        )?;
                    }
                }
            }
            Element::TabView(tabs) => {
                for item in tabs {
                    resolve_elements(
                        item.elements_mut(),
                        bindings,
                        resolved_occurrences,
                    )?;
                }
            }
            Element::List { items, .. } => {
                for item in items {
                    match item {
                        ListItem::Elements { elements, .. } => {
                            resolve_elements(elements, bindings, resolved_occurrences)?;
                        }
                        ListItem::SubList { element } => {
                            let placeholder = Element::Text(Cow::Borrowed(""));
                            let mut nested =
                                vec![std::mem::replace(element.as_mut(), placeholder)];
                            resolve_elements(
                                &mut nested,
                                bindings,
                                resolved_occurrences,
                            )?;
                            if nested.len() != 1 {
                                return Err(DelayedError::UnresolvedGeneratedOwner);
                            }
                            **element = nested.pop().expect("one nested list");
                        }
                    }
                }
            }
            Element::DefinitionList(items) => {
                for item in items {
                    resolve_elements(
                        &mut item.key_elements,
                        bindings,
                        resolved_occurrences,
                    )?;
                    resolve_elements(
                        &mut item.value_elements,
                        bindings,
                        resolved_occurrences,
                    )?;
                }
            }
            Element::Anchor { elements, .. }
            | Element::Collapsible { elements, .. }
            | Element::Color { elements, .. }
            | Element::Include { elements, .. } => {
                resolve_elements(elements, bindings, resolved_occurrences)?;
            }
            Element::WikidotBlockFrame { children, .. } => {
                resolve_elements(children, bindings, resolved_occurrences)?;
            }
            Element::Partial(partial) => match partial {
                PartialElement::ListItem(ListItem::Elements { elements, .. }) => {
                    resolve_elements(elements, bindings, resolved_occurrences)?;
                }
                PartialElement::ListItem(ListItem::SubList { element }) => {
                    let placeholder = Element::Text(Cow::Borrowed(""));
                    let mut nested =
                        vec![std::mem::replace(element.as_mut(), placeholder)];
                    resolve_elements(&mut nested, bindings, resolved_occurrences)?;
                    if nested.len() != 1 {
                        return Err(DelayedError::UnresolvedGeneratedOwner);
                    }
                    **element = nested.pop().expect("one nested partial list");
                }
                PartialElement::TableRow(row) => {
                    for cell in &mut row.cells {
                        resolve_elements(
                            &mut cell.elements,
                            bindings,
                            resolved_occurrences,
                        )?;
                    }
                }
                PartialElement::TableCell(cell) => {
                    resolve_elements(&mut cell.elements, bindings, resolved_occurrences)?;
                }
                PartialElement::WikidotFoster(elements) => {
                    resolve_elements(elements, bindings, resolved_occurrences)?;
                }
                PartialElement::Tab(tab) => {
                    resolve_elements(&mut tab.elements, bindings, resolved_occurrences)?;
                }
                PartialElement::RubyText(text) => {
                    resolve_elements(&mut text.elements, bindings, resolved_occurrences)?;
                }
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
        resolved.push(element);
    }
    *elements = resolved;
    Ok(())
}

fn trim_trailing_ascii_space(elements: &mut Vec<Element<'static>>) {
    let Some(Element::Text(text)) = elements.last_mut() else {
        return;
    };
    let trimmed_len = text.trim_end_matches([' ', '\t']).len();
    if trimmed_len == text.len() {
        return;
    }
    text.to_mut().truncate(trimmed_len);
    if text.is_empty() {
        elements.pop();
    }
}

pub(crate) fn elements_contain_delayed(elements: &[Element<'_>]) -> bool {
    elements.iter().any(element_contains_delayed)
}

fn element_contains_delayed(element: &Element<'_>) -> bool {
    match element {
        Element::Delayed(_) => true,
        Element::Container(container) => elements_contain_delayed(container.elements()),
        Element::Table(table) => table.rows.iter().any(|row| {
            row.cells
                .iter()
                .any(|cell| elements_contain_delayed(&cell.elements))
        }),
        Element::TabView(tabs) => tabs
            .iter()
            .any(|item| elements_contain_delayed(item.elements())),
        Element::List { items, .. } => items.iter().any(|item| match item {
            ListItem::Elements { elements, .. } => elements_contain_delayed(elements),
            ListItem::SubList { element } => element_contains_delayed(element),
        }),
        Element::DefinitionList(items) => items.iter().any(|item| {
            elements_contain_delayed(&item.key_elements)
                || elements_contain_delayed(&item.value_elements)
        }),
        Element::Anchor { elements, .. }
        | Element::Collapsible { elements, .. }
        | Element::Color { elements, .. }
        | Element::Include { elements, .. } => elements_contain_delayed(elements),
        Element::WikidotBlockFrame { children, .. } => elements_contain_delayed(children),
        Element::Partial(partial) => match partial {
            PartialElement::ListItem(ListItem::Elements { elements, .. }) => {
                elements_contain_delayed(elements)
            }
            PartialElement::ListItem(ListItem::SubList { element }) => {
                element_contains_delayed(element)
            }
            PartialElement::TableRow(row) => row
                .cells
                .iter()
                .any(|cell| elements_contain_delayed(&cell.elements)),
            PartialElement::TableCell(cell) => elements_contain_delayed(&cell.elements),
            PartialElement::WikidotFoster(elements) => elements_contain_delayed(elements),
            PartialElement::Tab(tab) => elements_contain_delayed(&tab.elements),
            PartialElement::RubyText(text) => elements_contain_delayed(&text.elements),
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
            | PartialElement::InlineAnchorClose(_) => false,
        },
        Element::Module(_)
        | Element::ContentSeparator
        | Element::Text(_)
        | Element::Raw(_)
        | Element::Variable(_)
        | Element::Email(_)
        | Element::AnchorName(_)
        | Element::Link { .. }
        | Element::FileLink { .. }
        | Element::Image { .. }
        | Element::Audio { .. }
        | Element::Video { .. }
        | Element::StandaloneButton(_)
        | Element::SocialButtons(_)
        | Element::EmbedVideo(_)
        | Element::Gallery(_)
        | Element::RadioButton { .. }
        | Element::CheckBox { .. }
        | Element::TableOfContents { .. }
        | Element::Footnote(_)
        | Element::FootnoteBlock { .. }
        | Element::BibliographyCite { .. }
        | Element::BibliographyBlock { .. }
        | Element::User { .. }
        | Element::Date { .. }
        | Element::Code(_)
        | Element::Math { .. }
        | Element::MathInline { .. }
        | Element::EquationReference(_)
        | Element::Embed(_)
        | Element::Html { .. }
        | Element::Iframe { .. }
        | Element::Style(_)
        | Element::LineBreak
        | Element::LineBreaks(_)
        | Element::ClearFloat(_)
        | Element::HorizontalRule => false,
    }
}

fn resolve_delayed(
    delayed: &DelayedElement<'_>,
    bindings: &BTreeMap<SlotId, GeneratedValue<'_>>,
) -> Result<Vec<Element<'static>>, DelayedError> {
    match &delayed.node {
        DelayedNode::Active { id, kind } => resolve_active(*id, *kind, bindings),
        DelayedNode::Suppressed { .. } => Err(DelayedError::UnresolvedGeneratedOwner),
        DelayedNode::TypographyBoundary => Err(DelayedError::UnresolvedGeneratedOwner),
        DelayedNode::RuntimeText(text) => {
            Ok(vec![Element::Text(Cow::Owned(text.to_string()))])
        }
        DelayedNode::Raw { atoms } => {
            let mut raw = String::new();
            for atom in atoms {
                match atom {
                    RecoveryAtom::Source(source) => raw.push_str(source),
                    RecoveryAtom::LegacySource { id, kind } => {
                        raw.push_str(&legacy_source(*id, *kind, bindings)?);
                    }
                    RecoveryAtom::Active { .. } | RecoveryAtom::LineBreak => {
                        return Err(DelayedError::BindingSchemaMismatch);
                    }
                }
            }
            Ok(vec![Element::Raw(Cow::Owned(raw))])
        }
        DelayedNode::Shell { atoms } => {
            let mut output = Vec::with_capacity(atoms.len());
            for atom in atoms {
                match atom {
                    RecoveryAtom::Source(source) => {
                        output.push(Element::Text(Cow::Owned(source.to_string())));
                    }
                    RecoveryAtom::Active { id, kind } => {
                        output.extend(resolve_active(*id, *kind, bindings)?);
                    }
                    RecoveryAtom::LineBreak => output.push(Element::LineBreak),
                    RecoveryAtom::LegacySource { .. } => {
                        return Err(DelayedError::BindingSchemaMismatch);
                    }
                }
            }
            Ok(output)
        }
        DelayedNode::PageConditionalRecovery { id, false_branch } => {
            let Some(GeneratedValue::PageLink { page, .. }) = bindings.get(id) else {
                return Err(DelayedError::BindingSchemaMismatch);
            };
            Ok(vec![Element::Text(Cow::Owned(format!(
                "[[[{}] | {}]]",
                legacy_page_target(page),
                false_branch,
            )))])
        }
        DelayedNode::TagExternalLabel { id, url } => {
            let Some(GeneratedValue::TagLinks { tags, separator }) = bindings.get(id)
            else {
                return Err(DelayedError::BindingSchemaMismatch);
            };
            let Some((first, remaining)) = tags.split_first() else {
                return Ok(vec![Element::Link {
                    ltype: LinkType::Direct,
                    link: LinkLocation::Url(Cow::Owned(url.to_string())),
                    label: LinkLabel::Text(Cow::Borrowed("")),
                    target: None,
                }]);
            };

            let first = legacy_tag_source(first);
            let label = first.strip_suffix(']').unwrap_or(&first).to_owned();
            let trailing = if remaining.is_empty() {
                "]".to_owned()
            } else {
                let remaining = remaining
                    .iter()
                    .map(legacy_tag_source)
                    .collect::<Vec<_>>()
                    .join(separator);
                format!("{separator}{remaining}]")
            };
            Ok(vec![
                Element::Link {
                    ltype: LinkType::Direct,
                    link: LinkLocation::Url(Cow::Owned(url.to_string())),
                    label: LinkLabel::Text(Cow::Owned(label)),
                    target: None,
                },
                Element::Text(Cow::Owned(trailing)),
            ])
        }
        DelayedNode::TagImage(image) => {
            let mut generated =
                legacy_source(image.id, GeneratedKind::TagLinks, bindings)?;
            generated.push_str(&image.suffix);
            let mut attributes = image.attributes.to_owned();
            let link = match image.attribute {
                GeneratedImageAttribute::Alt => {
                    attributes.insert("alt", Cow::Owned(generated));
                    image.link.as_ref().map(LinkLocation::to_owned)
                }
                GeneratedImageAttribute::Link => Some(LinkLocation::Url(Cow::Owned(
                    format!("/{}", generated.replace(' ', "%20"),),
                ))),
            };
            Ok(vec![Element::Image {
                source: image.source.to_owned(),
                link,
                alignment: image.alignment,
                attributes,
                target: image.target,
            }])
        }
    }
}

pub(crate) fn resolve_static_suppressions(
    elements: &mut Vec<Element<'_>>,
    apply_typography: bool,
) {
    seam::resolve_static_suppressions(elements, apply_typography);
}

fn resolve_bound_suppressions(
    elements: &mut Vec<Element<'static>>,
    bindings: &BTreeMap<SlotId, GeneratedValue<'_>>,
    resolved_occurrences: &mut usize,
    apply_typography: bool,
) -> Result<(), DelayedError> {
    seam::resolve_bound_suppressions(
        elements,
        bindings,
        resolved_occurrences,
        apply_typography,
    )
}

fn append_shell_source<'t>(atoms: &mut Vec<RecoveryAtom<'t>>, mut source: &'t str) {
    while let Some(newline) = source.find('\n') {
        let has_carriage_return = newline
            .checked_sub(1)
            .and_then(|index| source.as_bytes().get(index))
            == Some(&b'\r');
        let text_end = newline.saturating_sub(usize::from(has_carriage_return));
        if text_end > 0 {
            atoms.push(RecoveryAtom::Source(decode_semicolon_entities(
                &source[..text_end],
            )));
        }
        atoms.push(RecoveryAtom::LineBreak);
        source = &source[newline + 1..];
    }
    if !source.is_empty() {
        atoms.push(RecoveryAtom::Source(decode_semicolon_entities(source)));
    }
}

fn owned_recovery_atoms(atoms: &[RecoveryAtom<'_>]) -> Vec<RecoveryAtom<'static>> {
    atoms
        .iter()
        .map(|atom| match atom {
            RecoveryAtom::Source(source) => {
                RecoveryAtom::Source(Cow::Owned(source.to_string()))
            }
            RecoveryAtom::LegacySource { id, kind } => RecoveryAtom::LegacySource {
                id: *id,
                kind: *kind,
            },
            RecoveryAtom::Active { id, kind } => RecoveryAtom::Active {
                id: *id,
                kind: *kind,
            },
            RecoveryAtom::LineBreak => RecoveryAtom::LineBreak,
        })
        .collect()
}

fn resolve_active(
    id: SlotId,
    kind: GeneratedKind,
    bindings: &BTreeMap<SlotId, GeneratedValue<'_>>,
) -> Result<Vec<Element<'static>>, DelayedError> {
    let value = bindings
        .get(&id)
        .filter(|value| value.kind() == kind)
        .ok_or(DelayedError::BindingSchemaMismatch)?;
    match value {
        GeneratedValue::PageLink { page, label } => Ok(vec![Element::Link {
            ltype: LinkType::Page,
            link: LinkLocation::Page(page.clone()),
            label: LinkLabel::Text(Cow::Owned(label.to_string())),
            target: None,
        }]),
        GeneratedValue::TagLinks { tags, separator } => {
            let mut output = Vec::new();
            for (index, tag) in tags.iter().enumerate() {
                if index > 0 {
                    output.push(Element::Text(Cow::Owned(separator.to_string())));
                }
                output.push(Element::Link {
                    ltype: LinkType::Page,
                    link: LinkLocation::Page(PageRef::page_only(format!(
                        "system:page-tags/tag/{}",
                        tag.tag
                    ))),
                    label: LinkLabel::Text(Cow::Owned(tag.tag.to_string())),
                    target: None,
                });
            }
            Ok(output)
        }
    }
}

fn legacy_source(
    id: SlotId,
    kind: GeneratedKind,
    bindings: &BTreeMap<SlotId, GeneratedValue<'_>>,
) -> Result<String, DelayedError> {
    let value = bindings
        .get(&id)
        .filter(|value| value.kind() == kind)
        .ok_or(DelayedError::BindingSchemaMismatch)?;
    match value {
        GeneratedValue::PageLink { page, label } => Ok(format!(
            "[[[{} | {}]]]",
            legacy_page_target(page),
            label.replace(['[', ']'], ""),
        )),
        GeneratedValue::TagLinks { tags, separator } => Ok(tags
            .iter()
            .map(legacy_tag_source)
            .collect::<Vec<_>>()
            .join(separator)),
    }
}

fn legacy_page_target(page: &PageRef) -> String {
    let mut target = String::new();
    if let Some(site) = page.site() {
        target.push(':');
        target.push_str(site);
        target.push(':');
    }
    target.push_str(page.page());
    if let Some(extra) = page.extra() {
        target.push_str(extra);
    }
    target
}

fn legacy_tag_source(tag: &ResolvedTagRef<'_>) -> String {
    let tag = tag.tag.replace(['[', ']'], "");
    format!("[/system:page-tags/tag/{tag} {tag}]")
}

#[allow(dead_code)]
fn _assert_delayed_types_remain_paragraph_safe(
    _attributes: AttributeMap<'_>,
    _container: Container<'_>,
) {
}
