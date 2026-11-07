/*
 * bibliography.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2026 Wikijump Team
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program. If not, see <http://www.gnu.org/licenses/>.
 */

use super::clone::{elements_to_owned, string_to_owned};
use super::{AttributeMap, ContainerType, Element};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::collections::HashMap;

const RESIDUAL_LABEL: &str = "\0";

/// Prefix for an entry whose item is rendered from a nested tree position
/// (live converts every `: label : text` line in the joined body to a bibitem
/// before the later line rules run, so an item can sit inside a table cell,
/// list item or collapsible body). The rest of the label is the authored label
/// so duplicates keep live's numbering.
const NESTED_LABEL_PREFIX: &str = "\u{1}nested\u{1}";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BibliographyScopeWrapper<'t> {
    Container {
        ctype: ContainerType,
        attributes: AttributeMap<'t>,
    },
    Color {
        color: Cow<'t, str>,
        background: bool,
    },
}

impl BibliographyScopeWrapper<'_> {
    pub fn to_owned(&self) -> BibliographyScopeWrapper<'static> {
        match self {
            BibliographyScopeWrapper::Container { ctype, attributes } => {
                BibliographyScopeWrapper::Container {
                    ctype: *ctype,
                    attributes: attributes.to_owned(),
                }
            }
            BibliographyScopeWrapper::Color { color, background } => {
                BibliographyScopeWrapper::Color {
                    color: string_to_owned(color),
                    background: *background,
                }
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Bibliography<'t> {
    entries: Vec<(Cow<'t, str>, Vec<Element<'t>>)>,
    label_index: HashMap<String, IndexedReference>,
    /// Per-entry citation number, parallel to `entries` (residual entries
    /// hold 0). Derived from the labels, so it is rebuilt whenever entries
    /// are created.
    citations: Vec<usize>,
    reopened_wrappers: Vec<Vec<BibliographyScopeWrapper<'t>>>,
    title_wrappers: Vec<BibliographyScopeWrapper<'t>>,
    split_tail_next: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IndexedReference {
    citation: usize,
    position: usize,
}

impl PartialEq for Bibliography<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}

impl Eq for Bibliography<'_> {}

impl Serialize for Bibliography<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.entries.serialize(serializer)
    }
}

impl<'de, 't> Deserialize<'de> for Bibliography<'t>
where
    Vec<(Cow<'t, str>, Vec<Element<'t>>)>: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let entries = Vec::<(Cow<'t, str>, Vec<Element<'t>>)>::deserialize(deserializer)?;
        Ok(Bibliography::from_entries(entries))
    }
}

impl<'t> Bibliography<'t> {
    pub fn new() -> Self {
        Bibliography::default()
    }

    fn from_entries(entries: Vec<(Cow<'t, str>, Vec<Element<'t>>)>) -> Self {
        let mut bibliography = Bibliography {
            entries,
            label_index: HashMap::new(),
            citations: Vec::new(),
            reopened_wrappers: Vec::new(),
            title_wrappers: Vec::new(),
            split_tail_next: 0,
        };
        bibliography.rebuild_label_index();
        bibliography
    }

    fn rebuild_label_index(&mut self) {
        let entries = &self.entries;
        self.label_index.clear();
        self.citations.clear();
        self.citations.reserve(entries.len());
        for (position, (label, _)) in entries.iter().enumerate() {
            if Self::is_residual(label) {
                self.citations.push(0);
                continue;
            }
            let citation = self.label_index.len() + 1;
            self.citations.push(citation);
            self.label_index.insert(
                Self::real_label(label).to_owned(),
                IndexedReference { citation, position },
            );
        }
    }

    /// The number live Wikidot prints for the definition at `position`:
    /// one plus the number of *distinct* labels defined before it.
    ///
    /// Duplicate labels do not extend the distinct set, so a repeated
    /// definition reuses the next number while the labels after it keep
    /// numbering as if the duplicate still counted. Live renders
    /// `bell, bell, guy` as `1.`, `2.`, `2.` (captured 2026-11-05).
    pub fn citation_at(&self, position: usize) -> usize {
        self.citations.get(position).copied().unwrap_or(0)
    }

    pub fn add(&mut self, label: Cow<'t, str>, elements: Vec<Element<'t>>) {
        let citation = self.label_index.len() + 1;
        let position = self.entries.len();
        self.label_index.insert(
            label.as_ref().to_owned(),
            IndexedReference { citation, position },
        );
        self.citations.push(citation);
        self.entries.push((label, elements));
    }

    pub(crate) fn add_residual(&mut self, element: Element<'t>) {
        if let Some((label, elements)) = self.entries.last_mut()
            && Self::is_residual(label)
        {
            elements.push(element);
        } else {
            self.citations.push(0);
            self.entries
                .push((Cow::Borrowed(RESIDUAL_LABEL), vec![element]));
        }
    }

    pub(crate) fn is_residual(label: &str) -> bool {
        label == RESIDUAL_LABEL
    }

    pub(crate) fn allocate_split_tail_citation(&mut self) -> usize {
        if self.split_tail_next == 0 {
            let real = self
                .entries
                .iter()
                .filter(|(label, _)| !Self::is_residual(label) && !Self::is_nested(label))
                .count();
            self.split_tail_next = real + 1;
        }
        let citation = self.split_tail_next;
        self.split_tail_next += 1;
        citation
    }

    pub(crate) fn add_nested(&mut self, label: Cow<'t, str>) -> usize {
        let citation = self.label_index.len() + 1;
        let position = self.entries.len();
        let real = label.as_ref().to_owned();
        self.label_index
            .insert(real, IndexedReference { citation, position });
        self.citations.push(citation);
        self.entries.push((
            Cow::Owned(format!("{NESTED_LABEL_PREFIX}{label}")),
            Vec::new(),
        ));
        citation
    }

    pub fn is_nested(label: &str) -> bool {
        label.starts_with(NESTED_LABEL_PREFIX)
    }

    fn real_label(label: &str) -> &str {
        label.strip_prefix(NESTED_LABEL_PREFIX).unwrap_or(label)
    }

    pub fn get(&self, label: &str) -> Option<(usize, &[Element<'t>])> {
        if let Some(indexed) = self.label_index.get(label)
            && let Some((ref_label, elements)) = self.entries.get(indexed.position)
            && ref_label == label
        {
            return Some((indexed.citation, elements));
        }

        self.get_by_scan(label)
    }

    fn get_by_scan(&self, label: &str) -> Option<(usize, &[Element<'t>])> {
        let mut found: Option<(usize, &[Element<'t>])> = None;
        for (position, (ref_label, elements)) in self.entries.iter().enumerate() {
            if Self::is_residual(ref_label) {
                continue;
            }
            if label == Self::real_label(ref_label) {
                found = Some((self.citation_at(position), elements));
            }
        }

        found
    }

    pub fn reopened_wrappers(&self, index: usize) -> &[BibliographyScopeWrapper<'t>] {
        self.reopened_wrappers
            .get(index)
            .map_or(&[], |wrappers| wrappers)
    }

    pub(crate) fn set_reopened_wrappers(
        &mut self,
        wrappers: Vec<Vec<BibliographyScopeWrapper<'t>>>,
    ) {
        self.reopened_wrappers = wrappers;
    }

    pub fn title_wrappers(&self) -> &[BibliographyScopeWrapper<'t>] {
        &self.title_wrappers
    }

    pub(crate) fn set_title_wrappers(
        &mut self,
        wrappers: Vec<BibliographyScopeWrapper<'t>>,
    ) {
        self.title_wrappers = wrappers;
    }

    #[inline]
    pub fn slice(&self) -> &[(Cow<'t, str>, Vec<Element<'t>>)] {
        &self.entries
    }

    #[inline]
    pub(crate) fn slice_mut(&mut self) -> &mut [(Cow<'t, str>, Vec<Element<'t>>)] {
        &mut self.entries
    }

    pub fn to_owned(&self) -> Bibliography<'static> {
        let mut bibliography = Bibliography::from_entries(
            self.entries
                .iter()
                .map(|(label, elements)| {
                    (string_to_owned(label), elements_to_owned(elements))
                })
                .collect(),
        );
        bibliography.reopened_wrappers = self
            .reopened_wrappers
            .iter()
            .map(|wrappers| {
                wrappers
                    .iter()
                    .map(BibliographyScopeWrapper::to_owned)
                    .collect()
            })
            .collect();
        bibliography.title_wrappers = self
            .title_wrappers
            .iter()
            .map(BibliographyScopeWrapper::to_owned)
            .collect();
        bibliography
    }
}

#[derive(Serialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct BibliographyList<'t>(Vec<Bibliography<'t>>);

impl<'de, 't> Deserialize<'de> for BibliographyList<'t>
where
    Vec<Bibliography<'t>>: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bibliographies = Vec::<Bibliography<'t>>::deserialize(deserializer)?;
        Ok(BibliographyList(bibliographies))
    }
}

impl<'t> BibliographyList<'t> {
    pub fn new() -> Self {
        BibliographyList::default()
    }

    pub fn push(&mut self, bibliography: Bibliography<'t>) {
        self.0.push(bibliography);
    }

    pub fn append(&mut self, other: &mut Self) {
        self.0.append(&mut other.0);
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        self.0.truncate(len);
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn next_index(&self) -> usize {
        self.0.len()
    }

    pub fn get_reference(&self, label: &str) -> Option<(usize, &[Element<'t>])> {
        for bibliography in &self.0 {
            let reference = bibliography.get(label);
            if reference.is_some() {
                return reference;
            }
        }

        None
    }

    pub(crate) fn last_mut(&mut self) -> Option<&mut Bibliography<'t>> {
        self.0.last_mut()
    }

    pub fn get_bibliography(&self, index: usize) -> &Bibliography<'t> {
        &self.0[index]
    }

    pub fn get_bibliography_opt(&self, index: usize) -> Option<&Bibliography<'t>> {
        self.0.get(index)
    }

    #[inline]
    pub fn slice(&self) -> &[Bibliography<'t>] {
        &self.0
    }

    #[inline]
    pub(crate) fn slice_mut(&mut self) -> &mut [Bibliography<'t>] {
        &mut self.0
    }

    pub fn to_owned(&self) -> BibliographyList<'static> {
        BibliographyList(self.0.iter().map(|b| b.to_owned()).collect())
    }
}

#[test]
fn bibliography_helpers_resolve_latest_reference() {
    let first = vec![Element::Text(cow!("first"))];
    let duplicate = vec![Element::Text(cow!("duplicate"))];
    let second = vec![Element::Text(cow!("second"))];

    let mut bibliography = Bibliography::new();
    assert!(bibliography.get("alpha").is_none());

    bibliography.add(cow!("alpha"), first);
    bibliography.add(cow!("alpha"), duplicate.clone());
    bibliography.add(cow!("beta"), second.clone());

    // Live numbers by distinct labels: the duplicate alpha keeps the next
    // number and does not advance beta, so both end at 2.
    assert_eq!(bibliography.get("alpha"), Some((2, duplicate.as_slice())));
    assert_eq!(bibliography.get("beta"), Some((2, second.as_slice())));
    assert_eq!(bibliography.citation_at(0), 1);
    assert_eq!(bibliography.citation_at(1), 2);
    assert_eq!(bibliography.citation_at(2), 2);
    assert_eq!(bibliography.slice().len(), 3);
    assert_eq!(
        bibliography.to_owned().get("alpha"),
        Some((2, duplicate.as_slice()))
    );
}

#[test]
fn bibliography_list_helpers_cover_lookup_and_append() {
    let alpha = vec![Element::Text(cow!("alpha"))];
    let beta = vec![Element::Text(cow!("beta"))];

    let mut first = Bibliography::new();
    first.add(cow!("alpha"), alpha.clone());

    let mut second = Bibliography::new();
    second.add(cow!("beta"), beta.clone());

    let mut list = BibliographyList::new();
    assert!(list.is_empty());

    list.push(first);
    assert!(!list.is_empty());
    assert_eq!(list.next_index(), 1);
    assert_eq!(list.get_reference("alpha"), Some((1, alpha.as_slice())));
    assert!(list.get_reference("missing").is_none());
    assert_eq!(
        list.get_bibliography(0).get("alpha"),
        Some((1, alpha.as_slice()))
    );

    let mut other = BibliographyList::new();
    other.push(second);
    list.append(&mut other);

    assert_eq!(list.next_index(), 2);
    assert!(other.is_empty());
    assert_eq!(list.get_reference("beta"), Some((1, beta.as_slice())));
    assert_eq!(
        list.to_owned().get_reference("beta"),
        Some((1, beta.as_slice()))
    );
}
