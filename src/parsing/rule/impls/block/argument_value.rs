/*
 * parsing/rule/impls/block/argument_value.rs
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

use super::BlockRule;
use crate::parsing::string::parse_string;
use crate::parsing::{ParseError, ParseErrorKind, Parser, Token};
use std::borrow::Cow;

fn ends_bare_value(token: Token) -> bool {
    matches!(
        token,
        Token::Whitespace | Token::LineBreak | Token::ParagraphBreak | Token::RightBlock
    )
}

impl<'r, 't> Parser<'r, 't>
where
    'r: 't,
{
    /// Parse one block argument value.
    ///
    /// Wikidot accepts both `key="value"` and `key=value`. Bare values end at
    /// whitespace or the block's closing brackets and otherwise preserve the
    /// exact source slice, including syntax-shaped tokens such as `#`.
    pub(super) fn get_block_argument_value(
        &mut self,
        block_rule: &BlockRule,
        key: &str,
    ) -> Result<Cow<'t, str>, ParseError> {
        if self.current().token == Token::DoubleQuote {
            let allow_unclosed =
                self.settings().layout.legacy() && block_rule.name == "block-code";
            return self.get_wikidot_quoted_block_argument(allow_unclosed);
        }

        let image_link =
            block_rule.name == "block-image" && key.eq_ignore_ascii_case("link");
        let advanced_table = self.settings().layout.legacy()
            && matches!(
                block_rule.name,
                "block-table"
                    | "block-table-row"
                    | "block-table-cell-regular"
                    | "block-table-cell-header-body"
            );
        let legacy_code =
            self.settings().layout.legacy() && block_rule.name == "block-code";
        if !image_link && !advanced_table && !legacy_code {
            return Err(self.make_err(ParseErrorKind::BlockMalformedArguments));
        }

        let start = self.current();
        if ends_bare_value(start.token) || start.token == Token::InputEnd {
            return Err(self.make_err(ParseErrorKind::BlockMalformedArguments));
        }

        while !ends_bare_value(self.current().token) {
            if self.current().token == Token::InputEnd {
                return Err(self.make_err(ParseErrorKind::BlockMalformedArguments));
            }
            self.step()?;
        }

        let value = self.full_text().slice_partial(start, self.current());
        debug_assert!(!value.is_empty());
        Ok(Cow::Borrowed(value))
    }

    fn get_wikidot_quoted_block_argument(
        &mut self,
        allow_unclosed: bool,
    ) -> Result<Cow<'t, str>, ParseError> {
        let value_start = self.current();
        self.step()?;

        loop {
            if self.current().token == Token::DoubleQuote
                && quote_ends_block_argument(self)
            {
                let value_end = self.current();
                let value = self.full_text().slice(value_start, value_end);
                self.step()?;
                return Ok(parse_string(value));
            }

            if matches!(
                self.current().token,
                Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
            ) {
                return Err(self.make_err(ParseErrorKind::BlockMalformedArguments));
            }
            if allow_unclosed && self.current().token == Token::RightBlock {
                let value = self.full_text().slice(value_start, self.current());
                return Ok(Cow::Borrowed(value.strip_prefix('"').unwrap_or(value)));
            }
            self.step()?;
        }
    }
}

fn quote_ends_block_argument(parser: &Parser<'_, '_>) -> bool {
    let Some(next) = parser.look_ahead(0) else {
        return true;
    };

    match next.token {
        Token::RightBlock
        | Token::LineBreak
        | Token::ParagraphBreak
        | Token::InputEnd => true,
        Token::Whitespace => {
            let mut saw_key = false;
            for token in parser.remaining().iter().skip(1) {
                match token.token {
                    Token::Whitespace => continue,
                    Token::RightBlock => return true,
                    Token::Equals => return saw_key,
                    Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                        return false;
                    }
                    _ if token.slice.chars().all(|ch| {
                        ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')
                    }) =>
                    {
                        saw_key = true;
                    }
                    _ => return false,
                }
            }
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::Element;

    #[test]
    fn image_accepts_corpus_bare_anchor_argument() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let input = concat!(
            "[[image https://example.com/picture.png ",
            "style=\"width:70%;\" link=#]]",
        );
        let tokenization = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:#?}");
        let [
            Element::Image {
                link, attributes, ..
            },
        ] = tree.elements.as_slice()
        else {
            panic!("expected direct image, got {:?}", tree.elements);
        };

        assert_eq!(link, &None);
        assert_eq!(
            attributes.get().get("style").map(|value| value.as_ref()),
            Some("width:70%;"),
        );
    }

    #[test]
    fn wikidot_empty_bare_argument_is_ignored_without_losing_the_block() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[collapsible show= ]]body[[/collapsible]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("+&nbsp;show&nbsp;block"), "{html}");
        assert!(html.contains("body"), "{html}");
    }

    #[test]
    fn doubled_quotes_preserve_literal_quotes_in_block_arguments() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = concat!(
            "[[collapsible show=\"\"I am a doctor.\"\" ",
            "hide=\"\"I am a doctor.\"\"]]body[[/collapsible]]",
        );
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html.matches("&quot;I&nbsp;am&nbsp;a&nbsp;doctor.&quot;")
                .count(),
            2,
        );
        assert!(html.contains("body"), "{html}");
    }

    #[test]
    fn embedded_unescaped_quotes_remain_inside_block_argument() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let source = concat!(
            "[[collapsible show=\"the gun named \"Martha\" - dinner\" ",
            "hide=\"close\"]]body[[/collapsible]]",
        );
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(
            html.contains(
                "the&nbsp;gun&nbsp;named&nbsp;&quot;Martha&quot;&nbsp;-&nbsp;dinner"
            ),
            "{html}",
        );
    }

    #[test]
    fn ordinary_empty_quoted_argument_remains_empty() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(
            "[[collapsible show=\"\" hide=\"close\"]]body[[/collapsible]]",
        );
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(html.contains("collapsible-block"), "{html}");
    }
}
