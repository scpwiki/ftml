/*
 * tree/bibliography.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2022 Wikijump Team
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

use crate::tree::Element;
use std::borrow::Cow;

#[derive(Debug, Clone, Default)]
pub struct Bibliography<'t> {
    references: Vec<(Cow<'t, str>, Vec<Element<'t>>)>,
}

impl<'t> Bibliography<'t> {
    pub fn new() -> Self {
        Bibliography::default()
    }

    pub fn add(&mut self, label: Cow<'t, str>, elements: Vec<Element<'t>>) {
        // If the reference already exists, it is *not* overwritten.
        //
        // This maintains the invariant that the first reference with a given label,
        // across any bibliography, is the one which is used.
        if self.get(&label).is_some() {
            warn!("Duplicate reference in bibliography: {label}");
            return;
        }

        self.references.push((label, elements));
    }

    pub fn get(&self, label: &str) -> Option<(usize, &[Element<'t>])> {
        // References are maintained as a list, which means that searching
        // for a particular label is O(n), but this is fine as the number
        // of references is always going to be bounded. Even at 100 references
        // this would run at essentially the same speed.
        //
        // This also gives us free indexing based on this order, and the
        // order based on it, so we don't need a two-index map here.
        for (index, (ref_label, elements)) in self.references.iter().enumerate() {
            if label == ref_label {
                // Change from zero-indexing to one-indexing
                return Some((index + 1, elements));
            }
        }

        None
    }
}

#[derive(Debug, Clone, Default)]
pub struct BibliographyList<'t> {
    bibliographies: Vec<Bibliography<'t>>,
}

impl<'t> BibliographyList<'t> {
    pub fn new() -> Self {
        BibliographyList::default()
    }

    pub fn push(&mut self, bibliography: Bibliography<'t>) {
        self.bibliographies.push(bibliography);
    }

    pub fn get(&self, label: &str) -> Option<(usize, &[Element<'t>])> {
        for bibliography in &self.bibliographies {
            // Find the first entry with the label, per the above invariant.
            let reference = bibliography.get(label);
            if reference.is_some() {
                return reference;
            }
        }

        None
    }
}
