/*
 * parse/rule/impls/dash.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2020 Ammon Smith
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

use super::prelude::*;

pub const RULE_DASH: Rule = Rule {
    name: "dash",
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    log: &slog::Logger,
    _extracted: &'r ExtractedToken<'t>,
    remaining: &'r [ExtractedToken<'t>],
    _full_text: FullText<'t>,
) -> Consumption<'r, 't> {
    debug!(log, "Consuming token to create an em dash");

    // — - EM DASH
    Consumption::ok(text!("\u{2014}"), remaining)
}
