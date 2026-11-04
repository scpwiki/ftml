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
use crate::parsing::collect::{
    CommentElidedText, consume_iftags_segment, consume_valid_comment,
};
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

        let mut elisions = Vec::new();
        while !ends_bare_value(self.current().token) {
            if self.current().token == Token::InputEnd {
                return Err(self.make_err(ParseErrorKind::BlockMalformedArguments));
            }
            if self.current().token == Token::LeftBlock
                && let Some(segment) = consume_iftags_segment(self)?
            {
                elisions.extend(segment.elisions());
                continue;
            }
            if self.current().token == Token::LeftComment {
                let mut candidate = self.clone();
                if let Ok(range) = consume_valid_comment(&mut candidate) {
                    elisions.push(range);
                    self.update(&candidate);
                    continue;
                }
            }
            self.step()?;
        }

        if elisions.is_empty() {
            let value = self.full_text().slice_partial(start, self.current());
            debug_assert!(!value.is_empty());
            return Ok(Cow::Borrowed(value));
        }
        let range = start.span.start..self.current().span.start;
        let value = CommentElidedText::new(self.full_text().inner(), range, elisions);
        Ok(value.into_cow())
    }

    fn get_wikidot_quoted_block_argument(
        &mut self,
        allow_unclosed: bool,
    ) -> Result<Cow<'t, str>, ParseError> {
        let value_start = self.current();
        self.step()?;

        let mut elisions = Vec::new();
        loop {
            if self.current().token == Token::LeftBlock
                && let Some(segment) = consume_iftags_segment(self)?
            {
                elisions.extend(segment.elisions());
                continue;
            }

            if self.current().token == Token::LeftComment {
                let mut candidate = self.clone();
                if let Ok(range) = consume_valid_comment(&mut candidate) {
                    elisions.push(range);
                    self.update(&candidate);
                    continue;
                }
            }

            if self.current().token == Token::DoubleQuote
                && quote_ends_block_argument(self)
            {
                let value_end = self.current();
                self.step()?;
                if elisions.is_empty() {
                    let value = self.full_text().slice(value_start, value_end);
                    return Ok(parse_string(value));
                }
                let range = value_start.span.end..value_end.span.start;
                let text =
                    CommentElidedText::new(self.full_text().inner(), range, elisions)
                        .into_cow();
                return Ok(Cow::Owned(parse_string(&text).into_owned()));
            }

            if matches!(
                self.current().token,
                Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
            ) {
                return Err(self.make_err(ParseErrorKind::BlockMalformedArguments));
            }
            if allow_unclosed && self.current().token == Token::RightBlock {
                if elisions.is_empty() {
                    let value = self.full_text().slice(value_start, self.current());
                    return Ok(Cow::Borrowed(value.strip_prefix('"').unwrap_or(value)));
                }
                let range = value_start.span.end..self.current().span.start;
                let text =
                    CommentElidedText::new(self.full_text().inner(), range, elisions)
                        .into_cow();
                return Ok(text);
            }
            self.step()?;
        }
    }
}

fn quote_ends_block_argument<'r, 't>(parser: &Parser<'r, 't>) -> bool
where
    'r: 't,
{
    let Some(next) = parser.look_ahead(0) else {
        return true;
    };

    match next.token {
        Token::RightBlock
        | Token::LineBreak
        | Token::ParagraphBreak
        | Token::InputEnd => true,
        Token::Whitespace | Token::LeftComment => {
            let mut probe = parser.clone();
            if probe.step().is_err() {
                return true;
            }
            let mut saw_key = false;
            loop {
                match probe.current().token {
                    Token::Whitespace => {
                        if probe.step().is_err() {
                            return false;
                        }
                    }
                    Token::LeftComment => {
                        if consume_valid_comment(&mut probe).is_err() {
                            return false;
                        }
                    }
                    Token::RightBlock => return true,
                    Token::Equals => return saw_key,
                    Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                        return false;
                    }
                    _ if probe.current().slice.chars().all(|ch| {
                        ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')
                    }) =>
                    {
                        saw_key = true;
                        if probe.step().is_err() {
                            return false;
                        }
                    }
                    _ => return false,
                }
            }
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
