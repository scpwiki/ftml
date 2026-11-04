/*
 * parsing/rule/impls/block/blocks/ifcategory.rs
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
use crate::data::PageInfo;
use crate::delayed::DelayedElement;
use crate::parsing::{ElementCondition, ElementConditionType};

pub const BLOCK_IFCATEGORY: BlockRule = BlockRule {
    name: "block-ifcategory",
    accepts_names: &["ifcategory"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn,
};

fn parse_conditions<'r, 't>(
    parser: &Parser<'r, 't>,
    spec: Option<std::borrow::Cow<'t, str>>,
) -> Result<Vec<ElementCondition<'t>>, ParseError> {
    let Some(spec) = spec.map(trim_cow).filter(|spec| !spec.is_empty()) else {
        return Err(parser.make_err(ParseErrorKind::BlockMissingArguments));
    };

    let mut conditions = ElementCondition::parse_cow(spec);
    conditions.iter_mut().for_each(|condition| {
        if condition.ctype == ElementConditionType::Required {
            condition.ctype = ElementConditionType::Present;
        }
    });
    Ok(conditions)
}

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing ifcategory block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "IfCategory doesn't allow star flag");
    assert!(!flag_score, "IfCategory doesn't allow score flag");
    assert_block_name(&BLOCK_IFCATEGORY, name);

    if parser.settings().layout.legacy() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let rule = &BLOCK_IFCATEGORY;
    let conditions = parser.get_head_value_resolved(rule, in_head, parse_conditions)?;
    if parser.settings().layout.legacy()
        && !parser.discarding_hidden_body()
        && !parser.has_body_end_block(&BLOCK_IFCATEGORY)
    {
        return ok!(Elements::None);
    }

    let parser_state = parser.get_mutable_state();
    let body_start = parser.current().span.start;
    let body = parser.get_body_elements(&BLOCK_IFCATEGORY, false)?;
    let body_end = parser.current().span.start;
    let (elements, errors, paragraph_safe) = body.into();

    trace!(
        "IfCategory parsed {} conditions and {} elements",
        conditions.len(),
        elements.len(),
    );

    let elements = if check_ifcategory(parser.page_info(), &conditions) {
        trace!("Conditions passed, including elements");
        let mut bounded = Vec::with_capacity(elements.len() + 2);
        bounded.push(Element::Delayed(DelayedElement::typography_boundary()));
        bounded.extend(elements);
        bounded.push(Element::Delayed(DelayedElement::typography_boundary()));
        Elements::Multiple(bounded)
    } else {
        trace!("Conditions failed, excluding elements");
        parser.reset_mutable_state(parser_state);

        let generated = parser.generated_in_range(body_start..body_end);
        Elements::Single(Element::Delayed(DelayedElement::suppressed(&generated)))
    };

    ok!(paragraph_safe; elements, errors)
}

pub fn check_ifcategory(info: &PageInfo, conditions: &[ElementCondition]) -> bool {
    let category = match &info.category {
        Some(category) => category,
        None => "_default",
    };

    trace!("Checking ifcategory (category '{category}')");
    ElementCondition::check(conditions, &[cow!(category)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};
    use std::borrow::Cow;

    #[test]
    fn ifcategory_requires_conditions_and_checks_default_category() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        for input in [
            "[[ifcategory]]body[[/ifcategory]]",
            "[[ifcategory   ]]body[[/ifcategory]]",
        ] {
            let tokenization = crate::tokenize(input);
            let (_tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            assert!(
                errors
                    .iter()
                    .any(|error| error.kind() == ParseErrorKind::BlockMissingArguments),
                "{input} should require non-empty conditions: {errors:?}",
            );
        }

        let default_info = PageInfo {
            category: None,
            ..PageInfo::dummy()
        };
        assert!(check_ifcategory(
            &default_info,
            &ElementCondition::parse("+_default"),
        ));
        assert!(!check_ifcategory(
            &default_info,
            &ElementCondition::parse("other"),
        ));

        let named_info = PageInfo {
            category: Some(Cow::Borrowed("component")),
            ..PageInfo::dummy()
        };
        assert!(check_ifcategory(
            &named_info,
            &ElementCondition::parse("+component -other"),
        ));
    }
}
