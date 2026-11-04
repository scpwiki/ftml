/*
 * parsing/rule/impls/comment.rs
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
use crate::delayed::{DelayedElement, GeneratedInput};
use crate::parsing::collect::{TokenWindow, comment_closer_token_count_at};

pub const RULE_COMMENT: Rule = Rule {
    name: "comment",
    position: LineRequirement::Any,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Consuming tokens until end of comment");

    let opener_at_line_start = parser.start_of_line();
    assert_step(parser, Token::LeftComment)?;
    let mut generated = Vec::<GeneratedInput>::new();

    loop {
        if parser.settings().layout.legacy()
            && parser.discarding_hidden_body()
            && parser.at_hidden_body_boundary()
        {
            return Err(parser.make_err(ParseErrorKind::BlockExpectedEnd));
        }

        if parser.discarding_hidden_body()
            && parser
                .wikidot_iftags_body_end()
                .is_some_and(|designated| parser.at_designated_iftags_end(designated))
        {
            return Err(parser.make_err(ParseErrorKind::BlockExpectedEnd));
        }

        let token = parser.current().token;
        if let Some(slot) = parser.current_generated().cloned() {
            generated.push(slot);
        }

        trace!("Received token '{}' inside comment", token.name());

        match token {
            _ if let Some(token_count) = comment_closer_token_count(parser) => {
                trace!("Reached Wikidot comment closer, returning");
                parser.step_n(token_count)?;
                if opener_at_line_start
                    && matches!(
                        parser.current().token,
                        Token::BulletItem
                            | Token::NumberedItem
                            | Token::Heading
                            | Token::LeftComment
                    )
                {
                    parser.mark_virtual_start_of_line();
                }
                if opener_at_line_start
                    && parser.settings().layout.legacy()
                    && parser.current().token == Token::Whitespace
                {
                    parser.mark_virtual_start_of_line();
                }
                return if generated.is_empty() && parser.settings().layout.legacy() {
                    ok!(Elements::None)
                } else {
                    success_elements(Element::Delayed(DelayedElement::suppressed(
                        &generated,
                    )))
                };
            }

            Token::InputEnd => {
                trace!("Reached end of input, aborting");
                return Err(parser.make_err(ParseErrorKind::EndOfInput));
            }

            _ => {
                trace!("Token inside comment received. Discarding.");
                parser.step()?;
            }
        }
    }
}

pub(crate) fn comment_closer_token_count(parser: &Parser<'_, '_>) -> Option<usize> {
    comment_closer_token_count_at(TokenWindow::at(parser), 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn unterminated_comment_candidate_is_tokenized_as_ordinary_syntax() {
        let tokenization = crate::tokenize("[!-- unfinished");

        assert!(
            tokenization
                .tokens()
                .iter()
                .all(|token| token.token != Token::LeftComment),
        );
        assert!(
            tokenization
                .tokens()
                .iter()
                .any(|token| token.token == Token::DoubleDash),
        );
    }

    #[test]
    fn wikidot_unterminated_comment_opener_falls_back_with_typographic_dash() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = "[!-- unfinished".to_owned();
        crate::preprocess_for_layout(&mut source, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(html, "<p>[!\u{2014} unfinished</p>");
    }

    #[test]
    fn wikidot_unmatched_comment_closer_falls_back_with_typographic_dash() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut source = "raw-comment --]".to_owned();
        crate::preprocess_for_layout(&mut source, Layout::Wikidot);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(html, "<p>raw-comment \u{2014}]</p>");
    }

    #[test]
    fn wikidot_unmatched_extended_comment_closers_remain_ordinary_dashes() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for (source, expected) in [
            ("raw ---]", "<p>raw \u{2014}-]</p>"),
            ("raw ----]", "<p>raw \u{2014}\u{2014}]</p>"),
        ] {
            let tokenization = crate::tokenize(source);
            let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;

            assert_eq!(html, expected, "{source:?}");
        }
    }

    #[test]
    fn wikidot_comment_closer_accepts_extra_leading_hyphens() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for source in [
            "[!-- hidden ---]\nvisible",
            "[!----\nhidden module-shaped text\n---]\nvisible",
            "[!-- hidden ----]\nvisible",
            "[!-- hidden -----]\nvisible",
        ] {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            let html = HtmlRender.render(&tree, &page_info, &settings).body;

            assert!(errors.is_empty(), "{source:?}: {errors:?}");
            assert_eq!(html, "<p>visible</p>", "{source:?}");
        }
    }
}
