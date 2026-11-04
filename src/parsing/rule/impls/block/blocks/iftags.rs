/*
 * parsing/rule/impls/block/blocks/iftags.rs
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
use crate::parsing::ElementCondition;
use std::borrow::Cow;

pub const BLOCK_IFTAGS: BlockRule = BlockRule {
    name: "block-iftags",
    accepts_names: &["iftags"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn,
};

fn parse_conditions<'r, 't>(
    parser: &Parser<'r, 't>,
    spec: Option<&'t str>,
) -> Result<Vec<ElementCondition<'t>>, ParseError> {
    match spec {
        Some("-") if parser.settings().layout.legacy() => Ok(Vec::new()),
        Some(spec) if !parser.settings().layout.legacy() && spec.trim().is_empty() => {
            Err(parser.make_err(ParseErrorKind::BlockMissingArguments))
        }
        Some(spec) => Ok(ElementCondition::parse(spec)),
        None if parser.settings().layout.legacy() => Ok(Vec::new()),
        None => Err(parser.make_err(ParseErrorKind::BlockMissingArguments)),
    }
}

/// Consume an iftags gate inside a matched Wikidot span as literal text
/// (#792): its head, plus its body through its own paired closer when it
/// has one before the enclosing span's designated end. The condition is
/// never evaluated and the consumed range renders verbatim.
fn literal_span_gate<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
) -> ParseResult<'r, 't, Elements<'t>> {
    // The block machinery has already consumed `[[` and the name, so the
    // head's start is recovered by scanning the source back from the
    // current token: optional whitespace, the name, optional whitespace,
    // then `[[`.
    let text = parser.full_text().inner();
    let mut start = parser.current().span.start;
    let bytes = text.as_bytes();
    while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
        start -= 1;
    }
    if start < name.len() || !text[start - name.len()..start].eq_ignore_ascii_case(name) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    start -= name.len();
    while start > 0 && matches!(bytes[start - 1], b' ' | b'\t') {
        start -= 1;
    }
    if start < 2 || &text[start - 2..start] != "[[" {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    start -= 2;

    // Consume the rest of the head.
    loop {
        match parser.current().token {
            Token::RightBlock => {
                parser.step()?;
                break;
            }
            Token::InputEnd
            | Token::ParagraphBreak
            | Token::LeftBlock
            | Token::LeftBlockEnd => {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
            _ => parser.step()?,
        };
    }

    let (pairing_end, _) = parser.wikidot_iftags_pairing_end();
    let designated = parser.wikidot_iftags_body_end();
    match (pairing_end, designated) {
        // This gate's pairing reaches or passes the enclosing span's end:
        // consume up to (not including) the designated closer.
        (Some(pe), Some(d)) if pe >= d => {
            while parser.current().span.start < d
                && parser.current().token != Token::InputEnd
            {
                parser.step()?;
            }
        }
        // Consume through this gate's own paired closer.
        (Some(pe), _) => loop {
            let token = parser.current();
            if token.token == Token::LeftBlockEnd && token.span.start == pe {
                while parser.current().token != Token::RightBlock
                    && parser.current().token != Token::InputEnd
                {
                    parser.step()?;
                }
                if parser.current().token == Token::RightBlock {
                    parser.step()?;
                }
                break;
            }
            if token.span.start > pe || token.token == Token::InputEnd {
                break;
            }
            parser.step()?;
        },
        // Unpairable here: the head alone is the literal unit; the rest of
        // the enclosing body is gathered normally.
        _ => {}
    }

    let end = parser.current().span.start;
    ok!(true; Element::Text(Cow::Borrowed(&text[start..end])), Vec::new())
}

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing iftags block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "IfTags doesn't allow star flag");
    assert!(!flag_score, "IfTags doesn't allow score flag");
    assert_block_name(&BLOCK_IFTAGS, name);

    // Inside a matched Wikidot iftags span, iftags tags are literal text:
    // the enclosing span's pairing was decided at the text level and its
    // body is never rescanned (ftml #792). Consume this gate's own paired
    // unit as literal text, without evaluating its condition.
    if parser.settings().layout.legacy() && parser.in_wikidot_iftags_span() {
        return literal_span_gate(parser, name);
    }

    let (conditions, body_start) = parser.get_head_value_with_body_start(
        &BLOCK_IFTAGS,
        in_head,
        parse_conditions,
    )?;

    // A closed no-argument gate on Wikidot passes exactly when the page
    // carries at least one tag. The frozen PagePreview parity bindings are
    // tag-less observations and bind the gate as false; on the saved-page
    // route every real bare-gate page on record is tagged and renders its
    // body. Keeping this explicit avoids changing ElementCondition's
    // general empty-set semantics.
    let include_body = if conditions.is_empty() && parser.settings().layout.legacy() {
        !parser.page_info().tags.is_empty()
    } else {
        check_iftags(parser.page_info(), &conditions)
    };

    // Wikidot pairing (#792): in a visible context, the opener pairs with
    // the closer the pairing search chooses, not the closer proper nesting
    // would pick; the gather below is restricted to that designated closer
    // and nested iftags gates are literal inside the span. An unpairable opener
    // renders literally, as before. Gates parsed while a hidden body is
    // discarded, gates inside blockquote bodies, and false gates whose only
    // closers sit inside regions the search cannot see into all keep the
    // legacy guards and gathers (hidden bodies discard one gate at a time,
    // fail-closed).
    //
    // The wikijump layout takes the missing-closer guard as well (below):
    // a gate with no closer is not a gate at all, so the rule fails and
    // the opener renders literally, matching live Wikidot, which renders
    // both unclosed forms as plain text. Without the guard, the wikijump
    // false-condition path discarded elements to the end of the input
    // and reported success, silently swallowing the rest of the document.
    let legacy = parser.settings().layout.legacy();
    let mut designated_end = None;
    if legacy && !parser.discarding_hidden_body() && !parser.in_quote_body() {
        let (pairing, _) = parser.wikidot_iftags_pairing_end();
        match pairing {
            Some(end) => {
                designated_end = Some(end);
            }
            None => {
                if include_body || !parser.has_body_end_block(&BLOCK_IFTAGS) {
                    return Err(parser.make_err(ParseErrorKind::RuleFailed));
                }
            }
        }
    } else if legacy {
        if parser.discarding_hidden_body() {
            if !parser.has_two_body_end_blocks(&BLOCK_IFTAGS) {
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }
        } else if !parser.has_body_end_block(&BLOCK_IFTAGS) {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
    } else if !parser.discarding_hidden_body()
        && !parser.has_body_end_block(&BLOCK_IFTAGS)
    {
        // A gate parsed while a hidden body is discarded needs no guard:
        // the discard machinery consumes fail-closed (an unterminated
        // discard runs to the end of the input), so nothing can leak
        // through it, and imposing the guard there would change which
        // closer the enclosing discard treats as its end.
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let previous_body_end = parser.wikidot_iftags_body_end();
    let previous_span = parser.in_wikidot_iftags_span();
    if designated_end.is_some() {
        parser.set_wikidot_iftags_body_end(designated_end);
        parser.set_in_wikidot_iftags_span(true);
    }

    let result = if !include_body {
        trace!("Conditions failed, skipping hidden body");
        let hidden_start = parser.current().span.start;
        match parser
            .discard_body_elements_with_literal_quote_context(&BLOCK_IFTAGS, body_start)
        {
            Ok(()) => {
                let hidden_end = parser.current().span.start;
                let generated = parser.generated_in_range(hidden_start..hidden_end);
                ok!(true; Element::Delayed(DelayedElement::suppressed(&generated)), Vec::new())
            }
            Err(error) => Err(error),
        }
    } else {
        match parser.get_body_elements_with_literal_quote_context(
            &BLOCK_IFTAGS,
            false,
            body_start,
        ) {
            Ok(body) => {
                let (elements, errors, paragraph_safe) = body.into();

                trace!(
                    "IfTags parsed {} conditions and {} elements",
                    conditions.len(),
                    elements.len(),
                );

                trace!("Conditions passed, including elements");
                let mut bounded = Vec::with_capacity(elements.len() + 2);
                bounded.push(Element::Delayed(DelayedElement::typography_boundary()));
                bounded.extend(elements);
                bounded.push(Element::Delayed(DelayedElement::typography_boundary()));
                ok!(paragraph_safe; Elements::Multiple(bounded), errors)
            }
            Err(error) => Err(error),
        }
    };

    if designated_end.is_some() {
        parser.set_wikidot_iftags_body_end(previous_body_end);
        parser.set_in_wikidot_iftags_span(previous_span);
    }
    result
}

pub fn check_iftags(info: &PageInfo, conditions: &[ElementCondition]) -> bool {
    trace!("Checking iftags");
    ElementCondition::check(conditions, &info.tags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};
    use std::borrow::Cow;

    #[test]
    fn iftags_no_arguments_follows_page_tags_and_conditions_check_page_tags() {
        let legacy = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tagless = PageInfo {
            tags: Vec::new(),
            ..PageInfo::dummy()
        };
        let tagged = PageInfo::dummy();
        assert!(!tagged.tags.is_empty());

        fn body_rendered(
            source: &str,
            info: &PageInfo,
            settings: &WikitextSettings,
        ) -> bool {
            let tokenization = crate::tokenize(source);
            let (tree, errors) = crate::parse(&tokenization, info, settings).into();
            assert!(errors.is_empty(), "{errors:?}");
            format!("{tree:?}").contains("body")
        }

        // A closed no-argument gate hides its body on a tag-less page (the
        // frozen PagePreview binding context) and renders it on a tagged
        // page (the saved-route evidence).
        for source in ["[[iftags]]body[[/iftags]]", "[[iftags   ]]body[[/iftags]]"] {
            assert!(!body_rendered(source, &tagless, &legacy), "{source}");
            assert!(body_rendered(source, &tagged, &legacy), "{source}");
        }

        // Sign-only specs: "+" parses to a required condition on an empty
        // tag name, which no page satisfies, so the body stays hidden with
        // or without tags. "-" normalizes to no conditions in the legacy
        // layout and follows the no-argument rule above. Both stay hidden
        // on a tag-less page, as the frozen bindings record.
        assert!(!body_rendered(
            "[[iftags +]]body[[/iftags]]",
            &tagless,
            &legacy
        ));
        assert!(!body_rendered(
            "[[iftags +]]body[[/iftags]]",
            &tagged,
            &legacy
        ));
        assert!(!body_rendered(
            "[[iftags -]]body[[/iftags]]",
            &tagless,
            &legacy
        ));
        assert!(body_rendered(
            "[[iftags -]]body[[/iftags]]",
            &tagged,
            &legacy
        ));

        let modern = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        for source in ["[[iftags]]body[[/iftags]]", "[[iftags   ]]body[[/iftags]]"] {
            let tokenization = crate::tokenize(source);
            let (_tree, errors) = crate::parse(&tokenization, &tagged, &modern).into();
            assert!(
                errors
                    .iter()
                    .any(|error| error.kind() == ParseErrorKind::BlockMissingArguments),
                "{source}: {errors:?}",
            );
        }

        let info = PageInfo {
            tags: vec![Cow::Borrowed("alpha"), Cow::Borrowed("gamma")],
            ..PageInfo::dummy()
        };
        let included = ElementCondition::parse("+alpha -beta");
        let excluded = ElementCondition::parse("+alpha -gamma");

        assert!(check_iftags(&info, &included));
        assert!(!check_iftags(&info, &excluded));
    }
}
