/*
 * parsing/rule/impls/italics.rs
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

use super::inline_delimiter::{
    assert_unpadded_open, block_name_at, is_block_name, link_name_at,
    validate_reachable_close,
};
use super::prelude::*;
use crate::parsing::ParseSuccess;
use crate::parsing::collect::consume_iftags_segment;
use crate::tree::Container;

pub const RULE_ITALICS: Rule = Rule {
    name: "italics",
    position: LineRequirement::Any,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Trying to create italics (emphasis) container");

    if parser.current().token != Token::Italics {
        return try_consume_split_delimiter(parser);
    }

    if has_ambiguous_crossed_bold_pair(parser) {
        return collect_ambiguous_crossed_bold_italics(parser);
    }

    // Wikidot canonicalizes the real-world crossed pair
    // `//**text//**` as italics containing bold. Handle only the unambiguous
    // single-pair shape here; ordinary nested formatting continues through
    // the standard collector below.
    if has_crossed_bold_pair(parser) {
        return collect_crossed_bold_italics(parser);
    }

    validate_reachable_close(parser, Token::Italics)?;
    assert_unpadded_open(parser, Token::Italics)?;
    let close = [ParseCondition::current(Token::Italics)];
    let invalid = [
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::token_pair(Token::Italics, Token::Whitespace),
        ParseCondition::token_pair(Token::Whitespace, Token::Italics),
    ];
    let ctype = ContainerType::Italics;
    let collected =
        collect_container(parser, RULE_ITALICS, ctype, &close, &invalid, None)?;
    finish_collected(parser, collected)
}

/// The two `/`s of an italics opener can be split by an iftags segment
/// (`/[[iftags -zzz]][[/iftags]]/word//`): live Wikidot resolves the
/// segment before the delimiter forms, so the pair still opens italics.
/// The split opener only forms when every segment between the markers
/// resolves to no text; a segment whose body would be kept puts real
/// text between the markers, which is not an italics opener on live
/// either.
fn try_consume_split_delimiter<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    if parser.current().token != Token::Other || parser.current().slice != "/" {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let mut probe = parser.clone();
    probe.step()?; // first `/`
    let mut segments = 0usize;
    while probe.current().token == Token::LeftBlock {
        let Some(segment) = consume_iftags_segment(&mut probe)? else {
            break;
        };
        if segment.include_body
            && !probe.full_text().inner()[segment.body.clone()].is_empty()
        {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
        segments += 1;
    }
    if segments == 0
        || probe.current().token != Token::Other
        || probe.current().slice != "/"
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    validate_reachable_close(parser, Token::Italics)?;
    parser.step()?; // first `/`
    while parser.current().token == Token::LeftBlock
        && consume_iftags_segment(parser)?.is_some()
    {}
    assert_step(parser, Token::Other)?;
    if parser.current().token == Token::Whitespace {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let close = [ParseCondition::current(Token::Italics)];
    let invalid = [
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::token_pair(Token::Italics, Token::Whitespace),
        ParseCondition::token_pair(Token::Whitespace, Token::Italics),
    ];
    let collected = collect_container(
        parser,
        RULE_ITALICS,
        ContainerType::Italics,
        &close,
        &invalid,
        None,
    )?;
    finish_collected(parser, collected)
}

fn finish_collected<'r, 't>(
    parser: &mut Parser<'r, 't>,
    collected: ParseSuccess<Elements<'t>>,
) -> ParseResult<'r, 't, Elements<'t>> {
    let (elements, errors, paragraph_safe) = collected.into();
    if parser.settings().layout.legacy()
        && matches!(
            &elements,
            Elements::Single(Element::Container(container)) if container.elements().is_empty()
        )
    {
        return ok!(paragraph_safe; Elements::None, errors);
    }
    ok!(paragraph_safe; elements, errors)
}

fn has_ambiguous_crossed_bold_pair(parser: &Parser<'_, '_>) -> bool {
    if !matches!(
        parser.next_two_tokens(),
        (Token::Italics, Some(Token::Bold))
    ) {
        return false;
    }

    // The first `//` decides: a crossed closer `//**` makes the pair
    // ambiguous when bold was seen on the way, while a plain `//` closes these
    // italics normally. Scanning past it would pair this opener with a later
    // span's `//**`, swallowing everything in between (in a list, the following
    // items too) and making every such span retry its neighbours.
    let tokens = parser.remaining();
    let mut saw_intermediate_bold = false;
    let mut open_blocks: Vec<String> = Vec::new();
    for (index, token) in tokens.iter().enumerate().skip(1) {
        match token.token {
            Token::Italics => {
                return saw_intermediate_bold
                    && tokens
                        .get(index + 1)
                        .is_some_and(|next| next.token == Token::Bold);
            }
            Token::Bold => saw_intermediate_bold = true,
            Token::LeftBlock | Token::LeftBlockStar => {
                let name = block_name_at(tokens, index);
                if is_block_name(&name) {
                    open_blocks.push(name);
                }
            }
            Token::LeftLink => {
                if let Some(name) = link_name_at(tokens, index) {
                    open_blocks.push(name);
                }
            }
            Token::LeftBlockEnd => {
                let name = block_name_at(tokens, index);
                if !is_block_name(&name) {
                    // Literal text, not a block end.
                } else if let Some(pos) = open_blocks.iter().rposition(|n| *n == name) {
                    open_blocks.truncate(pos);
                } else {
                    // The enclosing block ends before any crossed closer.
                    return false;
                }
            }
            Token::ParagraphBreak | Token::InputEnd => return false,
            _ => {}
        }
    }
    false
}

fn collect_ambiguous_crossed_bold_italics<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    assert_step(parser, Token::Italics)?;
    assert_step(parser, Token::Bold)?;
    let source = collect_text(
        parser,
        RULE_ITALICS,
        &[ParseCondition::token_pair(Token::Italics, Token::Bold)],
        &[ParseCondition::current(Token::ParagraphBreak)],
        None,
    )?;
    assert_step(parser, Token::Bold)?;

    let bold = Element::Container(Container::new(
        ContainerType::Bold,
        vec![text!(source)],
        AttributeMap::new(),
    ));
    let italics = Element::Container(Container::new(
        ContainerType::Italics,
        vec![bold],
        AttributeMap::new(),
    ));
    success_elements(italics)
}

fn has_crossed_bold_pair(parser: &Parser<'_, '_>) -> bool {
    if !matches!(
        parser.next_two_tokens(),
        (Token::Italics, Some(Token::Bold))
    ) {
        return false;
    }

    let tokens = parser.remaining();
    let mut open_blocks: Vec<String> = Vec::new();
    for (index, token) in tokens.iter().enumerate().skip(1) {
        match token.token {
            Token::Italics => {
                return tokens
                    .get(index + 1)
                    .is_some_and(|next| next.token == Token::Bold);
            }
            Token::LeftBlock | Token::LeftBlockStar => {
                let name = block_name_at(tokens, index);
                if is_block_name(&name) {
                    open_blocks.push(name);
                }
            }
            Token::LeftLink => {
                if let Some(name) = link_name_at(tokens, index) {
                    open_blocks.push(name);
                }
            }
            Token::LeftBlockEnd => {
                let name = block_name_at(tokens, index);
                if !is_block_name(&name) {
                    // Literal text, not a block end.
                } else if let Some(pos) = open_blocks.iter().rposition(|n| *n == name) {
                    open_blocks.truncate(pos);
                } else {
                    // The enclosing block ends before any crossed closer.
                    return false;
                }
            }
            Token::Bold | Token::ParagraphBreak | Token::InputEnd => return false,
            _ => {}
        }
    }

    false
}

fn collect_crossed_bold_italics<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    assert_step(parser, Token::Italics)?;
    assert_step(parser, Token::Bold)?;

    let close = [ParseCondition::token_pair(Token::Italics, Token::Bold)];
    let invalid = [ParseCondition::current(Token::ParagraphBreak)];
    let inner = collect_container(
        parser,
        RULE_ITALICS,
        ContainerType::Bold,
        &close,
        &invalid,
        None,
    )?;
    assert_step(parser, Token::Bold)?;

    let (inner, errors, paragraph_safe) = inner.into();
    let elements = inner.into_iter().collect();
    let outer = Element::Container(Container::new(
        ContainerType::Italics,
        elements,
        AttributeMap::new(),
    ));

    Ok(ParseSuccess::new(outer.into(), errors, paragraph_safe))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};
    use std::time::{Duration, Instant};

    #[test]
    fn split_italics_opener_with_kept_segment_body_stays_literal() {
        // The segment gate passes and its body is kept, so real text
        // sits between the two s: not an italics opener, matching
        // the text-level resolution live Wikidot performs.
        let (html, _errors) = render("/[[iftags +tale]]YES[[/iftags]]/word//");
        assert_eq!(html, "<p>/YES/word//</p>");
    }

    fn render(source: &str) -> (String, Vec<ParseError>) {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;
        (html, errors)
    }

    #[test]
    fn wikidot_crossed_bold_italics_pair_is_canonicalized() {
        let (html, errors) = render("//**You liked this//**");

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(
            html.contains("<em><strong>You liked this</strong></em>"),
            "{html}",
        );
    }

    #[test]
    fn ordinary_nested_bold_inside_italics_is_unchanged() {
        let (html, errors) = render("//before **bold** after//");

        assert!(errors.is_empty(), "{errors:#?}");
        assert!(
            html.contains("<em>before <strong>bold</strong> after</em>"),
            "{html}",
        );
    }

    #[test]
    fn each_italic_bold_span_closes_at_its_own_closer() {
        // The second case is EN from-120-s-archives-hub (source SHA-256
        // 4919a9bc9c2eef4434204a694bbd373c282315f0ab1fb671dc0d3ffa11dd4eda), which
        // Wikidot renders as `<em><strong>Warsaw Reconstruction Office</strong></em><br />
        // <strong>GENIUS LOCI <em>DEPARTMENT</em></strong>`.
        for (input, expected) in [
            (
                "//**A**// and //**B**//",
                "<p><em><strong>A</strong></em> and <em><strong>B</strong></em></p>",
            ),
            (
                "//**Warsaw Reconstruction Office**//\n**GENIUS LOCI //DEPARTMENT//**",
                concat!(
                    "<p><em><strong>Warsaw Reconstruction Office</strong></em><br>\n",
                    "<strong>GENIUS LOCI <em>DEPARTMENT</em></strong></p>",
                ),
            ),
        ] {
            let (html, errors) = render(input);
            assert!(errors.is_empty(), "{input:?}: {errors:#?}");
            assert_eq!(html, expected, "{input:?}");
        }
    }

    #[test]
    fn italic_bold_list_items_stay_separate_in_bounded_time() {
        // Reduced from EN vikander-kneed-technical-media-hub, a list of
        // `//**Title**//` items. Pairing each opener with the next item's
        // `//**` swallowed the items in between and took minutes to parse.
        const ITEM_COUNT: usize = 200;
        let input = "* //**Title**// by author\n".repeat(ITEM_COUNT);

        let started = Instant::now();
        let (html, errors) = render(&input);
        let elapsed = started.elapsed();

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches("<li>").count(), ITEM_COUNT, "{html}");
        assert_eq!(
            html.matches("<li><em><strong>Title</strong></em> by author</li>")
                .count(),
            ITEM_COUNT,
            "{html}",
        );
        assert!(
            elapsed < Duration::from_millis(500),
            "italic bold list parse took {elapsed:?}",
        );
    }

    #[test]
    fn ambiguous_crossed_delimiters_match_wikidot_nesting() {
        let (html, errors) = render("//**one **two//**");

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html, "<p><em><strong>one **two</strong></em></p>");
    }

    #[test]
    fn unmatched_italics_does_not_swallow_the_next_block() {
        // Real shape from en everhue-art-hub: a collapsible whose body
        // ends in a lone `//` line, followed by more collapsibles, one
        // of which contains a paired `//`. The lone delimiter must not
        // pair across the block boundary; Wikidot renders all three
        // collapsibles.
        let (html, _errors) = render(concat!(
            "[[collapsible show=\"A\" hide=\"A\"]]
",
            "[[image https://scp-wiki.wikidot.com/local--files/everhue-art-hub/TZRvldl.webp]]
",
            "//
",
            "[[/collapsible]]
",
            "[[collapsible show=\"B\" hide=\"B\"]]
",
            "[[image https://scp-wiki.wikidot.com/local--files/everhue-art-hub/Kuvbcu0.webp]]
",
            "[[/collapsible]]
",
            "[[collapsible show=\"C\" hide=\"C\"]]
",
            "//paired//
",
            "[[/collapsible]]
",
        ));
        assert_eq!(
            html.matches("collapsible-block-folded").count(),
            3,
            "{html}"
        );
        assert_eq!(html.matches("<em>").count(), 1, "{html}");
    }
}
