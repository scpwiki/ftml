/*
 * tree/element/iter_owned.rs
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

use super::{Element, Elements};

impl<'t> IntoIterator for Elements<'t> {
    type Item = Element<'t>;
    type IntoIter = OwnedElementsIterator<'t>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            Elements::None => OwnedElementsIterator::empty(),
            Elements::Single(element) => OwnedElementsIterator::single(element),
            Elements::Multiple(elements) => OwnedElementsIterator::multiple(elements),
        }
    }
}

/// Owned iterator implementation for `Elements`.
#[derive(Debug)]
pub struct OwnedElementsIterator<'t> {
    single: Option<Element<'t>>,
    multiple: std::vec::IntoIter<Element<'t>>,
}

impl<'t> OwnedElementsIterator<'t> {
    #[inline]
    fn empty() -> Self {
        OwnedElementsIterator {
            single: None,
            multiple: Vec::new().into_iter(),
        }
    }

    #[inline]
    fn single(element: Element<'t>) -> Self {
        OwnedElementsIterator {
            single: Some(element),
            multiple: Vec::new().into_iter(),
        }
    }

    #[inline]
    fn multiple(elements: Vec<Element<'t>>) -> Self {
        OwnedElementsIterator {
            single: None,
            multiple: elements.into_iter(),
        }
    }
}

impl<'t> Iterator for OwnedElementsIterator<'t> {
    type Item = Element<'t>;

    #[inline]
    fn next(&mut self) -> Option<Element<'t>> {
        if let Some(element) = self.single.take() {
            Some(element)
        } else {
            self.multiple.next()
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = usize::from(self.single.is_some()) + self.multiple.len();
        (len, Some(len))
    }
}

impl ExactSizeIterator for OwnedElementsIterator<'_> {}

#[test]
fn iter() {
    macro_rules! test {
        ($elements:expr, $expected:expr $(,)?) => {{
            let elements = $elements;

            let actual: Vec<Element> = elements.into_iter().collect();
            let expected = $expected;

            assert_eq!(
                actual, expected,
                "Actual element iteration doesn't match expected",
            );
        }};
    }

    test!(Elements::None, vec![]);
    test!(Elements::Single(text!("a")), vec![text!("a")]);
    test!(Elements::Multiple(vec![]), vec![],);
    test!(Elements::Multiple(vec![text!("a")]), vec![text!("a")],);
    test!(
        Elements::Multiple(vec![text!("a"), text!("b")]),
        vec![text!("a"), text!("b")],
    );
    test!(
        Elements::Multiple(vec![text!("a"), text!("b"), text!("c")]),
        vec![text!("a"), text!("b"), text!("c")],
    );
}

#[test]
fn owned_iterator_reports_exact_remaining_len() {
    let mut elements = Elements::Single(text!("a")).into_iter();
    assert_eq!(elements.size_hint(), (1, Some(1)));
    assert_eq!(elements.len(), 1);
    assert_eq!(elements.next(), Some(text!("a")));
    assert_eq!(elements.size_hint(), (0, Some(0)));
    assert_eq!(elements.len(), 0);

    let mut elements = Elements::Multiple(vec![text!("a"), text!("b")]).into_iter();
    assert_eq!(elements.size_hint(), (2, Some(2)));
    assert_eq!(elements.len(), 2);
    assert_eq!(elements.next(), Some(text!("a")));
    assert_eq!(elements.size_hint(), (1, Some(1)));
    assert_eq!(elements.len(), 1);
}
