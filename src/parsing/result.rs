/*
 * parsing/result.rs
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

use crate::parsing::exception::{ParseException, ParseError};
use crate::parsing::Parser;
use crate::tree::{Element, Elements};
use std::marker::PhantomData;

pub type ParseResult<'r, 't, T> = Result<ParseSuccess<'r, 't, T>, ParseError>;
pub type ParseSuccessTuple<T> = (T, Vec<ParseException>, bool);

#[must_use]
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ParseSuccess<'r, 't, T>
where
    T: 't,
    'r: 't,
{
    pub item: T,
    pub exceptions: Vec<ParseException>,
    pub paragraph_safe: bool,

    // Marker fields to assert that the 'r lifetime is at least as long as 't.
    #[doc(hidden)]
    _ref_marker: PhantomData<&'r ()>,
    #[doc(hidden)]
    _text_marker: PhantomData<&'t str>,
}

impl<'r, 't, T> ParseSuccess<'r, 't, T> {
    #[inline]
    pub fn new(item: T, exceptions: Vec<ParseException>, paragraph_safe: bool) -> Self {
        ParseSuccess {
            item,
            exceptions,
            paragraph_safe,
            _ref_marker: PhantomData,
            _text_marker: PhantomData,
        }
    }

    pub fn chain(
        self,
        all_exceptions: &mut Vec<ParseException>,
        all_paragraph_safe: &mut bool,
    ) -> T {
        let ParseSuccess {
            item,
            mut exceptions,
            paragraph_safe,
            ..
        } = self;

        // Append previous exceptions
        all_exceptions.append(&mut exceptions);

        // Update paragraph safety
        *all_paragraph_safe &= paragraph_safe;

        // Return resultant item
        item
    }
}

impl<'r, 't, T> ParseSuccess<'r, 't, T> {
    pub fn map<F, U>(self, f: F) -> ParseSuccess<'r, 't, U>
    where
        F: FnOnce(T) -> U,
    {
        let ParseSuccess {
            item,
            exceptions,
            paragraph_safe,
            ..
        } = self;

        let new_item = f(item);

        ParseSuccess {
            item: new_item,
            exceptions,
            paragraph_safe,
            _ref_marker: PhantomData,
            _text_marker: PhantomData,
        }
    }

    #[inline]
    pub fn map_ok<F, U>(self, f: F) -> ParseResult<'r, 't, U>
    where
        F: FnOnce(T) -> U,
    {
        Ok(self.map(f))
    }
}

impl<'r, 't> ParseSuccess<'r, 't, Elements<'t>> {
    pub fn check_partials(&self, parser: &Parser) -> Result<(), ParseError> {
        for element in &self.item {
            // This check only applies if the element is a partial.
            if let Element::Partial(partial) = element {
                // Check if the current rule is looking for a partial.
                if !parser.accepts_partial().matches(partial) {
                    // Found a partial when not looking for one. Raise the appropriate warning.
                    return Err(parser.make_warn(partial.parse_warning_kind()));
                }
            }
        }

        Ok(())
    }
}

impl<'r, 't> ParseSuccess<'r, 't, ()> {
    #[inline]
    pub fn into_exceptions(self) -> Vec<ParseException> {
        self.exceptions
    }
}

impl<'r, 't, T> From<ParseSuccess<'r, 't, T>> for ParseSuccessTuple<T> {
    #[inline]
    fn from(success: ParseSuccess<'r, 't, T>) -> ParseSuccessTuple<T> {
        let ParseSuccess {
            item,
            exceptions,
            paragraph_safe,
            ..
        } = success;

        (item, exceptions, paragraph_safe)
    }
}
