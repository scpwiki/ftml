/*
 * parsing/rule/impls/block/blocks/target.rs
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

use super::prelude::*;
use crate::id_prefix::isolate_ids;
use std::borrow::Cow;

pub const BLOCK_TARGET: BlockRule = BlockRule {
    name: "block-target",
    accepts_names: &["target", "anchortarget"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing target block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Target doesn't allow star flag");
    assert!(!flag_score, "Target doesn't allow score flag");
    assert_block_name(&BLOCK_TARGET, name);

    let block = &BLOCK_TARGET;
    let name = parser.get_head_value(block, in_head, require_trimmed_block_argument)?;

    if name.is_empty() || name.contains(' ') {
        return Err(parser.make_err(ParseErrorKind::BlockMissingArguments));
    }

    let name = if parser.settings().isolate_user_ids {
        Cow::Owned(isolate_ids(name))
    } else {
        Cow::Borrowed(name)
    };

    success_elements(Element::AnchorName(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn target_block_requires_name_argument() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tokenization = crate::tokenize("[[target]]");
        let (_tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(
            errors
                .iter()
                .any(|error| error.kind() == ParseErrorKind::BlockMissingArguments)
        );
    }
}
