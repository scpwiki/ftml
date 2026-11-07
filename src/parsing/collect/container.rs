/*
 * parsing/collect/container.rs
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
use crate::parsing::collect::collect_consume;
use crate::tree::{AttributeMap, Container, ContainerType, Element};

pub fn collect_container<'r, 't>(
    parser: &mut Parser<'r, 't>,
    rule: Rule,
    container_type: ContainerType,
    closes: &[ParseCondition],
    invalids: &[ParseCondition],
    kind: Option<ParseErrorKind>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let quote_cursor = QuoteBodyCursorInstallation::install(parser);
    let preserve_empty_lists = parser.settings().layout.legacy()
        && container_type == ContainerType::Bold;
    let previous_preserve = if preserve_empty_lists {
        parser.set_wikidot_preserve_empty_lists(true)
    } else {
        false
    };

    let collection = collect_consume(parser, rule, closes, invalids, kind);
    if preserve_empty_lists {
        parser.set_wikidot_preserve_empty_lists(previous_preserve);
    }
    quote_cursor.uninstall(parser);
    let collection = collection?;
    let split_close_leftover = collection.split_close_leftover.clone();
    let (elements, errors, paragraph_safe) = collection.into();

    let container = Container::new(container_type, elements, AttributeMap::new());
    let element = Element::Container(container);
    let safe = paragraph_safe && container_type.paragraph_safe();
    let mut success = ParseSuccess::new(Elements::Single(element), errors, safe);
    success.split_close_leftover = split_close_leftover;
    Ok(success)
}

struct QuoteBodyCursorInstallation {
    installed: bool,
}

impl QuoteBodyCursorInstallation {
    fn install(parser: &mut Parser<'_, '_>) -> Self {
        let installed = if parser.quote_body_cursor().is_none()
            && let Some(required_depth) = parser.native_blockquote_depth()
        {
            parser.install_quote_body_cursor_at_current_line(required_depth);
            true
        } else {
            false
        };
        QuoteBodyCursorInstallation { installed }
    }

    fn uninstall(self, parser: &mut Parser<'_, '_>) {
        if self.installed {
            parser.set_quote_body_cursor(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::rule::impls::RULE_TEXT;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn collect_container_wraps_consumed_elements() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("alpha]]tail");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser.step().expect("identifier should follow input start");

        let success = collect_container(
            &mut parser,
            RULE_TEXT,
            ContainerType::Bold,
            &[ParseCondition::current(Token::RightBlock)],
            &[],
            None,
        )
        .expect("container should collect until the right block token");

        assert!(success.paragraph_safe);
        assert!(success.errors.is_empty());

        match success.item {
            Elements::Single(Element::Container(container)) => {
                assert_eq!(container.ctype(), ContainerType::Bold);
                assert_eq!(container.elements(), &[text!("alpha")]);
                assert!(container.attributes().get().is_empty());
            }
            other => panic!("expected one container, got {other:?}"),
        }

        assert_eq!(parser.current().slice, "tail");
    }
}
