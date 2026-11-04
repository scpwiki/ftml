/*
 * parsing/rule/impls/link_anchor.rs
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

use super::RULE_COMMENT;
use super::prelude::*;
use crate::id_prefix::isolate_ids;
use crate::parsing::ParseSuccess;
use crate::parsing::collect::{
    collect_text_resolved_fused, consume_iftags_segment, consume_iftags_segment_fused,
};
use crate::parsing::{discard_wikidot_controls, trim_wikidot_ascii_cow};
use crate::tree::{AnchorTarget, LinkLabel, LinkLocation, LinkType};
use std::borrow::Cow;
use wikidot_normalize::normalize;

#[derive(Debug)]
struct WikidotAnchorLabel<'t> {
    text: Cow<'t, str>,
    delayed: Vec<Element<'t>>,
    residual_brackets: usize,
}

pub const RULE_LINK_ANCHOR: Rule = Rule {
    name: "link-anchor",
    position: LineRequirement::Any,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    assert_step(parser, Token::LeftBracketAnchor)?;
    try_consume_anchor(parser, None)
}

pub(super) fn try_consume_wikidot_star_anchor<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>>
where
    'r: 't,
{
    if parser.current().token != Token::NumberedItem || parser.current().slice != "#" {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    parser.step()?;
    try_consume_anchor(parser, Some(AnchorTarget::NewTab))
}

pub(super) fn try_consume_wikidot_bracket_anchor<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>>
where
    'r: 't,
{
    if parser.current().token != Token::NumberedItem || parser.current().slice != "#" {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    parser.step()?;
    try_consume_anchor(parser, None)
}

fn try_consume_anchor<'r, 't>(
    parser: &mut Parser<'r, 't>,
    target: Option<AnchorTarget>,
) -> ParseResult<'r, 't, Elements<'t>>
where
    'r: 't,
{
    let url_close = [ParseCondition::current(Token::Whitespace)];
    let url_invalid = [
        ParseCondition::current(Token::RightBracket),
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::current(Token::LineBreak),
    ];
    let url = if parser.settings().layout.legacy() {
        collect_wikidot_anchor_name(parser)?
    } else {
        collect_text_resolved_fused(
            parser,
            RULE_LINK_ANCHOR,
            &url_close,
            &url_invalid,
            None,
        )?
        .0
    };

    if parser.settings().layout.legacy()
        && !url.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'%')
        })
    {
        return Err(parser.make_err(ParseErrorKind::InvalidUrl));
    }

    let url = if url.is_empty() {
        Cow::Borrowed("javascript:;")
    } else {
        // Wikidot preserves the authored fragment spelling. Wikijump keeps
        // its normalized, optionally isolated identifier behavior.
        let mut url = str!(url);
        if !parser.settings().layout.legacy() {
            normalize(&mut url);
            if parser.settings().isolate_user_ids {
                url = isolate_ids(&url);
            }
        }
        url.insert(0, '#');

        Cow::Owned(url)
    };

    let label_close = [ParseCondition::current(Token::RightBracket)];
    let label_invalid = [
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::current(Token::LineBreak),
    ];
    let WikidotAnchorLabel {
        text: label,
        delayed,
        residual_brackets,
    } = if parser.settings().layout.legacy() {
        collect_wikidot_anchor_label(parser)?
    } else {
        WikidotAnchorLabel {
            text: collect_anchor_text(parser, &label_close, &label_invalid)?,
            delayed: Vec::new(),
            residual_brackets: 0,
        }
    };

    let label = if parser.settings().layout.legacy() {
        trim_wikidot_ascii_cow(discard_wikidot_controls(label))
    } else {
        trim_cow(label)
    };
    if parser.settings().layout.legacy() && label.is_empty() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let element = Element::Link {
        ltype: LinkType::Anchor,
        link: LinkLocation::Url(url),
        label: LinkLabel::Text(label),
        target,
    };
    let mut output = Vec::with_capacity(2 + delayed.len());
    output.push(element);
    if residual_brackets > 0 {
        output.push(Element::Text(Cow::Owned("]".repeat(residual_brackets))));
    }
    output.extend(delayed);
    let elements: Elements = output.into();
    let paragraph_safe = elements.paragraph_safe();
    Ok(ParseSuccess::new(elements, Vec::new(), paragraph_safe))
}

fn collect_wikidot_anchor_name<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<Cow<'t, str>, ParseError>
where
    'r: 't,
{
    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let mut copied_until = start;
    let mut without_segments = None::<String>;

    // Materializes the collected name, resolving any iftags segments
    // that were consumed along the way.
    macro_rules! finish {
        ($end:expr) => {
            match without_segments {
                Some(mut output) => {
                    output.push_str(&source[copied_until..$end]);
                    Cow::Owned(output)
                }
                None => Cow::Borrowed(&source[start..$end]),
            }
        };
    }

    loop {
        match parser.current().token {
            Token::Whitespace => {
                let end = parser.current().span.start;
                parser.step()?;
                return Ok(finish!(end));
            }
            // Wikidot typography turns `1 A` into a non-breaking space. It
            // must retain separator ownership when a comment-elided label
            // exposes that sequence, as in `[#toc1 A[!--x--]B]`.
            Token::Other if parser.current().slice == "\u{a0}" => {
                let end = parser.current().span.start;
                parser.step()?;
                return Ok(finish!(end));
            }
            Token::RightBracket | Token::LineBreak | Token::ParagraphBreak => {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            Token::GeneratedPageLink | Token::GeneratedTagLinks => {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            Token::InputEnd => return Err(parser.make_err(ParseErrorKind::EndOfInput)),
            Token::LeftBlock => match consume_iftags_segment(parser)? {
                Some(segment) => {
                    let output = without_segments.get_or_insert_with(String::new);
                    output.push_str(&source[copied_until..segment.range.start]);
                    if segment.include_body {
                        output.push_str(&source[segment.body.clone()]);
                    }
                    copied_until = segment.range.end;
                }
                None => {
                    parser.step()?;
                }
            },
            _ => {
                parser.step()?;
            }
        }
    }
}

fn collect_wikidot_anchor_label<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> Result<WikidotAnchorLabel<'t>, ParseError>
where
    'r: 't,
{
    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let mut copied_until = start;
    let mut without_comments = None::<String>;
    let mut delayed = Vec::<Element<'t>>::new();

    loop {
        match parser.current().token {
            Token::RightBracket | Token::RightBlock | Token::RightLink => {
                let end = parser.current().span.start;
                let residual_brackets = parser.current().slice.len() - 1;
                if let Some(output) = without_comments.as_mut() {
                    output.push_str(&source[copied_until..end]);
                }
                parser.step()?;
                return Ok(WikidotAnchorLabel {
                    text: match without_comments {
                        Some(output) => Cow::Owned(output),
                        None => Cow::Borrowed(&source[start..end]),
                    },
                    delayed,
                    residual_brackets,
                });
            }
            Token::LeftComment => {
                let comment_start = parser.current().span.start;
                let output = without_comments
                    .get_or_insert_with(|| String::with_capacity(comment_start - start));
                output.push_str(&source[copied_until..comment_start]);

                let mut comment_parser = parser.clone_with_rule(RULE_COMMENT);
                let comment = RULE_COMMENT.try_consume(&mut comment_parser)?;
                delayed.extend(comment.item);
                parser.update(&comment_parser);
                copied_until = parser.current().span.start;
            }
            Token::LeftBlock => match consume_iftags_segment_fused(parser)? {
                Some(segment) => {
                    {
                        let output = without_comments.get_or_insert_with(|| {
                            String::with_capacity(segment.range.start - start)
                        });
                        output.push_str(&source[copied_until..segment.range.start]);
                        if segment.include_body {
                            output.push_str(&source[segment.body.clone()]);
                        }
                    }
                    copied_until = segment.range.end;

                    if segment.fused_closer {
                        // The segment ended inside a fused bracket
                        // token whose leftover bracket is the link's
                        // own closing bracket; the token is already
                        // consumed.
                        return Ok(WikidotAnchorLabel {
                            text: Cow::Owned(without_comments.take().unwrap()),
                            delayed,
                            residual_brackets: 0,
                        });
                    }
                }
                None => {
                    parser.step()?;
                }
            },
            Token::LineBreak | Token::ParagraphBreak => {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            Token::GeneratedPageLink | Token::GeneratedTagLinks => {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            Token::InputEnd => return Err(parser.make_err(ParseErrorKind::EndOfInput)),
            _ => {
                parser.step()?;
            }
        }
    }
}

fn trim_cow(value: Cow<'_, str>) -> Cow<'_, str> {
    match value {
        Cow::Borrowed(value) => Cow::Borrowed(value.trim()),
        Cow::Owned(value) => Cow::Owned(value.trim().to_owned()),
    }
}

fn collect_anchor_text<'r, 't>(
    parser: &mut Parser<'r, 't>,
    close: &[ParseCondition],
    invalid: &[ParseCondition],
) -> Result<Cow<'t, str>, ParseError>
where
    'r: 't,
{
    Ok(collect_text_resolved_fused(parser, RULE_LINK_ANCHOR, close, invalid, None)?.0)
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn wikidot_anchor_links_keep_the_live_fragment_name() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[#Patrick Jump]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html, r##"<p><a href="#Patrick">Jump</a></p>"##);
    }

    #[test]
    fn wikidot_anchor_links_accept_the_live_name_characters() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let render = |source: &str| {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            assert!(errors.is_empty(), "{errors:#?}");
            HtmlRender.render(&tree, &page_info, &settings).body
        };

        assert_eq!(
            render("[#my-anchor Label]"),
            "<p><a href=\"#my-anchor\">Label</a></p>",
        );
        assert_eq!(
            render("[#my_anchor Label]"),
            "<p><a href=\"#my_anchor\">Label</a></p>",
        );
        assert_eq!(render("[#a%b Label]"), "<p><a href=\"#a%b\">Label</a></p>",);
        assert_eq!(
            render("[#my-anchor Label text]"),
            "<p><a href=\"#my-anchor\">Label text</a></p>",
        );
    }

    #[test]
    fn wikidot_anchor_links_reject_names_outside_the_live_characters() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let render = |source: &str| {
            let tokenization = crate::tokenize(source);
            let (tree, _) = crate::parse(&tokenization, &page_info, &settings).into();
            HtmlRender.render(&tree, &page_info, &settings).body
        };

        assert_eq!(render("[#my.anchor Label]"), "<p>[#my.anchor Label]</p>");
        assert_eq!(render("[#a:b Label]"), "<p>[#a:b Label]</p>");
        assert_eq!(render("[#a+b Label]"), "<p>[#a+b Label]</p>");
        assert_eq!(render("[#my-anchor]"), "<p>[#my-anchor]</p>");
    }
}
