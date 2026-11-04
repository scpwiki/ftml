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


use super::Element;
use super::clone::{elements_to_owned, string_to_owned};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::collections::HashMap;

const RESIDUAL_LABEL: &str = "\0";

#[derive(Debug, Clone, Default)]
pub struct Bibliography<'t> {
    entries: Vec<(Cow<'t, str>, Vec<Element<'t>>)>,
    label_index: HashMap<String, IndexedReference>,
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
        };
        bibliography.rebuild_label_index();
        bibliography
    }

    fn rebuild_label_index(&mut self) {
        self.label_index.clear();
        let mut citation = 0;
        for (position, (label, _)) in self.entries.iter().enumerate() {
            if Self::is_residual(label) {
                continue;
            }
            citation += 1;
            self.label_index.insert(
                label.as_ref().to_owned(),
                IndexedReference { citation, position },
            );
        }
    }

    pub fn add(&mut self, label: Cow<'t, str>, elements: Vec<Element<'t>>) {
        let citation = self
            .entries
            .iter()
            .filter(|(existing, _)| !Self::is_residual(existing))
            .count()
            + 1;
        let position = self.entries.len();
        self.label_index.insert(
            label.as_ref().to_owned(),
            IndexedReference { citation, position },
        );
        self.entries.push((label, elements));
    }

    pub(crate) fn add_residual(&mut self, element: Element<'t>) {
        if let Some((label, elements)) = self.entries.last_mut()
            && Self::is_residual(label)
        {
            elements.push(element);
        } else {
            self.entries
                .push((Cow::Borrowed(RESIDUAL_LABEL), vec![element]));
        }
    }

    pub(crate) fn is_residual(label: &str) -> bool {
        label == RESIDUAL_LABEL
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
        let mut index = 0;
        let mut found: Option<(usize, &[Element<'t>])> = None;
        for (ref_label, elements) in &self.entries {
            if Self::is_residual(ref_label) {
                continue;
            }
            index += 1;
            if label == ref_label {
                found = Some((index, elements));
            }
        }

        found
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
        Bibliography::from_entries(
            self.entries
                .iter()
                .map(|(label, elements)| {
                    (string_to_owned(label), elements_to_owned(elements))
                })
                .collect(),
        )
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

    assert_eq!(bibliography.get("alpha"), Some((2, duplicate.as_slice())));
    assert_eq!(bibliography.get("beta"), Some((3, second.as_slice())));
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
