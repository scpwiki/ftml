/*
 * tree/element/collection.rs
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
use std::slice;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum Elements<'t> {
    Multiple(Vec<Element<'t>>),
    Single(Element<'t>),
    None,
}

impl Elements<'_> {
    #[inline]
    pub fn is_empty(&self) -> bool {
        match self {
            Elements::Multiple(elements) => elements.is_empty(),
            Elements::Single(_) => false,
            Elements::None => true,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        match self {
            Elements::Multiple(elements) => elements.len(),
            Elements::Single(_) => 1,
            Elements::None => 0,
        }
    }

    pub fn paragraph_safe(&self) -> bool {
        match self {
            Elements::Multiple(elements) => {
                elements.iter().all(|element| element.paragraph_safe())
            }
            Elements::Single(element) => element.paragraph_safe(),
            Elements::None => true,
        }
    }
}

impl<'t> AsRef<[Element<'t>]> for Elements<'t> {
    fn as_ref(&self) -> &[Element<'t>] {
        match self {
            Elements::Multiple(elements) => elements,
            Elements::Single(element) => slice::from_ref(element),
            Elements::None => &[],
        }
    }
}

impl<'t> From<Element<'t>> for Elements<'t> {
    #[inline]
    fn from(element: Element<'t>) -> Elements<'t> {
        Elements::Single(element)
    }
}

impl<'t> From<Option<Element<'t>>> for Elements<'t> {
    #[inline]
    fn from(element: Option<Element<'t>>) -> Elements<'t> {
        match element {
            Some(element) => Elements::Single(element),
            None => Elements::None,
        }
    }
}

impl<'t> From<Vec<Element<'t>>> for Elements<'t> {
    #[inline]
    fn from(elements: Vec<Element<'t>>) -> Elements<'t> {
        Elements::Multiple(elements)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_len_cover_all_shapes() {
        let cases = [
            (Elements::None, true, 0),
            (Elements::Single(text!("single")), false, 1),
            (Elements::Multiple(vec![]), true, 0),
            (Elements::Multiple(vec![text!("a"), text!("b")]), false, 2),
        ];

        for (elements, is_empty, len) in cases {
            assert_eq!(elements.is_empty(), is_empty);
            assert_eq!(elements.len(), len);
        }
    }

    #[test]
    fn paragraph_safe_uses_element_safety_for_each_shape() {
        assert!(Elements::None.paragraph_safe());
        assert!(Elements::Multiple(vec![]).paragraph_safe());
        assert!(Elements::Single(text!("single")).paragraph_safe());
        assert!(
            Elements::Multiple(vec![
                text!("a"),
                Element::Raw(cow!("raw")),
                Element::LineBreak,
            ])
            .paragraph_safe(),
        );
        assert!(!Elements::Single(Element::HorizontalRule).paragraph_safe());
        assert!(
            !Elements::Multiple(vec![text!("a"), Element::HorizontalRule])
                .paragraph_safe(),
        );
    }

    #[test]
    fn as_ref_returns_element_slices_for_each_shape() {
        assert_eq!(Elements::None.as_ref(), &[]);

        let single = Elements::Single(text!("single"));
        assert_eq!(single.as_ref(), &[text!("single")]);

        let multiple = Elements::Multiple(vec![text!("a"), text!("b")]);
        assert_eq!(multiple.as_ref(), &[text!("a"), text!("b")]);
    }

    #[test]
    fn from_element_creates_single() {
        assert_eq!(
            Elements::from(text!("present")),
            Elements::Single(text!("present")),
        );
    }

    #[test]
    fn from_option_element_preserves_some_and_none() {
        assert_eq!(
            Elements::from(Some(text!("present"))),
            Elements::Single(text!("present")),
        );
        assert_eq!(Elements::from(None), Elements::None);
    }

    #[test]
    fn from_vec_element_preserves_vector_shape() {
        assert_eq!(
            Elements::from(vec![text!("a"), text!("b")]),
            Elements::Multiple(vec![text!("a"), text!("b")]),
        );
        assert_eq!(Elements::from(Vec::new()), Elements::Multiple(vec![]));
    }
}
