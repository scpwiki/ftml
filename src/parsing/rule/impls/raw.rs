/*
 * parsing/rule/impls/raw.rs
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

use super::entity::decode_semicolon_entities;
use super::prelude::*;
use crate::delayed::DelayedElement;
use crate::tree::PartialElement;
use std::borrow::Cow;

macro_rules! raw {
    ($value:expr) => {
        Element::Raw(cow!($value))
    };
}

pub const RULE_RAW: Rule = Rule {
    name: "raw",
    position: LineRequirement::Any,
    try_consume_fn,
};

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Consuming tokens until end of raw");

    let ending_token = match parser.current().token {
        Token::Raw => Token::Raw,
        Token::LeftRaw => Token::RightRaw,
        _ => panic!("Current token is not a starting raw"),
    };

    if ending_token == Token::Raw {
        trace!("First token is '@@', checking for special cases");

        let next_1 = parser.look_ahead_err(0)?;
        let next_2 = parser.look_ahead_err(1)?;

        let special_case = if next_2.token == Token::Raw
            && let Some(generated) = parser.generated_for(next_1).cloned()
        {
            parser.step_n(3)?;
            Some(Element::Delayed(DelayedElement::raw(
                parser.full_text().inner(),
                generated.source_range.clone(),
                &[generated],
            )))
        } else {
            match (next_1.token, next_2.token) {
                (Token::Raw, Token::Raw) => {
                    trace!("Found meta-raw (\"@@@@@@\"), returning");
                    parser.step_n(3)?;
                    Some(text!("@@"))
                }

                (Token::Raw, Token::Other) => {
                    if next_2.slice == "@" {
                        trace!("Found single-raw (\"@@@@@\"), returning");
                        parser.step_n(3)?;
                        Some(text!("@"))
                    } else {
                        trace!("Found empty raw (\"@@@@\"), followed by other text");
                        parser.step_n(2)?;
                        return ok!(Elements::None);
                    }
                }

                (Token::Raw, _) => {
                    trace!("Found empty raw (\"@@@@\"), returning");
                    parser.step_n(2)?;
                    return if parser.settings().layout.legacy()
                        && super::footnote_first::starts_footnote(parser)
                    {
                        ok!(Element::Partial(PartialElement::WikidotEmptyInlineOwner,))
                    } else {
                        ok!(Elements::None)
                    };
                }

                (Token::LineBreak, Token::Raw) => {
                    parser.step_n(3)?;
                    return finish_double_at_raw(parser, text!(" "));
                }

                (Token::ParagraphBreak, Token::Raw) => {
                    if parser.settings().layout.legacy() {
                        parser.step_n(3)?;
                        return ok!(Elements::None);
                    }
                    trace!("Found interrupted raw, aborting");
                    return Err(parser.make_err(ParseErrorKind::RuleFailed));
                }

                (Token::RightRaw, Token::Raw) if parser.settings().layout.legacy() => {
                    parser.step_n(3)?;
                    let raw = next_1
                        .slice
                        .strip_suffix('@')
                        .expect("right-raw token ends with an at sign");
                    return ok!(Elements::Multiple(vec![raw!(raw), text!("@")]));
                }

                (_, Token::Raw) => {
                    trace!("Found single-element raw, returning");
                    parser.step_n(3)?;
                    return finish_double_at_raw(parser, raw!(next_1.slice));
                }

                _ => None,
            }
        };

        if let Some(element) = special_case {
            return success_elements(element);
        }
    }

    let current = parser.step()?;
    let (start, mut end) = (current, current);
    let mut saw_left_raw = false;
    let mut saw_nested_raw_pair = false;
    let mut generated = Vec::new();
    let mut saw_non_authored = false;
    let mut crossed_break = false;

    loop {
        let token = parser.current().token;

        trace!("Received token '{}' inside raw", token.name());

        // `@@<...>@@` overlaps an angle-delimited raw span by one `@` on
        // each side in Wikidot. The ordinary scanner therefore exposes the
        // closing `>@` before it can expose a second `@@` token. Preserve the
        // two residual at signs and give the authored angle span ownership.
        if parser.settings().layout.legacy()
            && ending_token == Token::Raw
            && token == Token::RightRaw
            && generated.is_empty()
            && !saw_non_authored
            && parser.current_generated().is_none()
            && parser.look_ahead(0).is_some_and(|trailing| {
                trailing.token == Token::Other
                    && trailing.slice == "@"
                    && parser.generated_for(trailing).is_none()
            })
        {
            let slice = parser.full_text().slice_partial(start, end);
            if let Some(inner) = slice.strip_prefix('<') {
                parser.step_n(2)?;
                return ok!(Elements::Multiple(vec![
                    text!("@"),
                    raw!(inner),
                    text!("@"),
                ]));
            }
        }

        if matches!(token, Token::RightRaw | Token::Raw) {
            if token == ending_token {
                trace!("Reached end of raw, returning");

                let content_range = start.span.start..parser.current().span.start;
                let slice = parser.full_text().slice_partial(start, end);
                parser.step()?;

                if parser.settings().layout.legacy()
                    && ending_token == Token::Raw
                    && saw_nested_raw_pair
                {
                    return ok!(Elements::None);
                }

                if parser.settings().layout.legacy()
                    && ending_token == Token::Raw
                    && slice.ends_with(">@")
                {
                    let raw = slice
                        .strip_suffix('@')
                        .expect("right-raw token ends with an at sign");
                    return ok!(Elements::Multiple(vec![raw!(raw), text!("@")]));
                }

                if parser.settings().layout.legacy()
                    && ending_token == Token::RightRaw
                    && slice.is_empty()
                {
                    return ok!(Elements::None);
                }

                if !generated.is_empty() {
                    return success_elements(Element::Delayed(DelayedElement::raw(
                        parser.full_text().inner(),
                        content_range,
                        &generated,
                    )));
                }

                let raw = match ending_token {
                    Token::RightRaw => decode_semicolon_entities(slice),
                    Token::Raw => cow!(slice),
                    _ => unreachable!(),
                };
                if parser.settings().layout.legacy()
                    && ending_token == Token::Raw
                    && let Some(inner) = slice
                        .strip_prefix("<<")
                        .and_then(|slice| slice.strip_suffix(">>"))
                {
                    let raw = format!("<{inner}>");
                    return ok!(Elements::Multiple(vec![
                        text!("@"),
                        Element::Raw(Cow::Owned(raw)),
                        text!("@"),
                    ]));
                }
                let element = Element::Raw(raw);
                return if ending_token == Token::Raw {
                    let result = finish_double_at_raw(parser, element);
                    if crossed_break {
                        result.map(|mut success| {
                            success.item = match success.item {
                                Elements::None => Elements::None,
                                Elements::Single(element) => {
                                    Elements::Multiple(vec![element, text!(" ")])
                                }
                                Elements::Multiple(mut elements) => {
                                    elements.insert(1, text!(" "));
                                    Elements::Multiple(elements)
                                }
                            };
                            success
                        })
                    } else {
                        result
                    }
                } else {
                    success_elements(element)
                };
            }

            trace!("Wasn't end of raw, continuing");
            if token == Token::RightRaw && saw_left_raw {
                saw_nested_raw_pair = true;
            }
        } else if token == Token::LeftRaw && ending_token == Token::Raw {
            saw_left_raw = true;
        } else if token == Token::ParagraphBreak {
            trace!("Reached paragraph break, aborting");
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        } else if token == Token::LineBreak {
            let crosses = ending_token == Token::Raw
                && start.span.start < end.span.start
                && parser.current_generated().is_none()
                && parser.look_ahead(0).is_some_and(|next| {
                    next.token == Token::Raw && parser.generated_for(next).is_none()
                });
            if !crosses {
                trace!("Reached newline, aborting");
                return Err(parser.make_err(ParseErrorKind::RuleFailed));
            }

            crossed_break = true;
            parser.step()?;
            continue;
        } else if token == Token::InputEnd {
            trace!("Reached end of input, aborting");
            return Err(parser.make_err(ParseErrorKind::EndOfInput));
        }

        if let Some(slot) = parser.current_generated() {
            generated.push(slot.clone());
        }
        if matches!(
            token,
            Token::RuntimeText | Token::GeneratedPageLink | Token::GeneratedTagLinks
        ) {
            saw_non_authored = true;
        }

        trace!("Appending present token to raw");

        end = parser.step()?;
    }
}

fn finish_double_at_raw<'r, 't>(
    parser: &mut Parser<'r, 't>,
    element: Element<'t>,
) -> ParseResult<'r, 't, Elements<'t>>
where
    'r: 't,
{
    fn generated_ahead<'r, 't>(parser: &Parser<'r, 't>) -> bool {
        parser.current_generated().is_some()
            || matches!(
                parser.current().token,
                Token::RuntimeText | Token::GeneratedPageLink | Token::GeneratedTagLinks
            )
    }

    let source = parser.full_text().inner();
    let start = parser.current().span.start;
    let mut candidate = parser.clone();
    let mut run_end = None;

    while !candidate.current().slice.is_empty()
        && candidate.current().slice.bytes().all(|byte| byte == b'@')
    {
        if generated_ahead(&candidate) {
            return success_elements(element);
        }

        run_end = Some(candidate.current().span.end);
        if candidate.step().is_err() {
            break;
        }
    }

    let Some(end) = run_end else {
        return success_elements(element);
    };

    let commit_run = |parser: &mut Parser<'r, 't>| {
        parser.update(&candidate);
        ok!(Elements::Multiple(vec![
            element.clone(),
            text!(&source[start..end]),
        ]))
    };

    let mut probe = candidate.clone();
    let at_close = loop {
        if generated_ahead(&probe) {
            return success_elements(element);
        }

        match probe.current().token {
            Token::Raw => break probe.clone(),
            Token::LineBreak | Token::ParagraphBreak | Token::InputEnd => {
                return commit_run(parser);
            }
            _ => {
                if probe.step().is_err() {
                    return commit_run(parser);
                }
            }
        }
    };

    let close_start = at_close.current().span.start;
    let close_end = at_close.current().span.end;

    let mut after_close = at_close.clone();
    let _ = after_close.step();
    let mut scan = after_close.clone();
    let mut later_raw = false;
    loop {
        if generated_ahead(&scan) {
            break;
        }

        match scan.current().token {
            Token::Raw => {
                later_raw = true;
                break;
            }
            Token::LineBreak => {
                let mut next = scan.clone();
                if next.step().is_ok()
                    && !generated_ahead(&next)
                    && next.current().token == Token::Raw
                {
                    later_raw = true;
                }
                break;
            }
            Token::ParagraphBreak | Token::InputEnd => break,
            _ => {
                if scan.step().is_err() {
                    break;
                }
            }
        }
    }

    if later_raw {
        parser.update(&at_close);
        ok!(Elements::Multiple(vec![
            element,
            text!(&source[start..close_start]),
        ]))
    } else {
        parser.update(&after_close);
        ok!(Elements::Multiple(vec![
            element,
            text!(&source[start..close_end]),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::Parser;
    use crate::settings::{WikitextMode, WikitextSettings};

    fn with_raw_elements<R>(
        input: &str,
        assert_result: impl FnOnce(Result<Elements<'_>, ParseError>) -> R,
    ) -> R {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize(input);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        let result = parser
            .step()
            .and_then(|_| try_consume_fn(&mut parser))
            .map(|success| success.item);

        assert_result(result)
    }

    #[test]
    fn raw_rule_handles_short_raw_special_cases() {
        with_raw_elements("@@@@@@", |result| {
            assert_eq!(result.unwrap(), Elements::Single(text!("@@")));
        });
        with_raw_elements("@@@@@", |result| {
            assert_eq!(result.unwrap(), Elements::Single(text!("@")));
        });
        with_raw_elements("@@@@", |result| {
            assert_eq!(result.unwrap(), Elements::None);
        });
        with_raw_elements("@@token@@", |result| {
            assert_eq!(result.unwrap(), Elements::Single(raw!("token")));
        });
        with_raw_elements("@@\n@@", |result| {
            assert_eq!(result.unwrap(), Elements::Single(text!(" ")));
        });
    }

    #[test]
    fn raw_rule_collects_long_raw_and_left_raw_forms() {
        with_raw_elements("@@a >@ b@@", |result| {
            assert_eq!(result.unwrap(), Elements::Single(raw!("a >@ b")));
        });
        with_raw_elements("@<a @@ b>@", |result| {
            assert_eq!(result.unwrap(), Elements::Single(raw!("a @@ b")));
        });
    }

    #[test]
    fn wikidot_discards_double_at_raw_containing_a_complete_angle_raw() {
        with_raw_elements("@@foo @< >@ bar@@", |result| {
            assert_eq!(result.unwrap(), Elements::None);
        });
        with_raw_elements("@@a >@ b@@", |result| {
            assert_eq!(result.unwrap(), Elements::Single(raw!("a >@ b")));
        });
    }

    #[test]
    fn wikidot_resolves_overlapping_raw_closers_without_stealing_the_last_at() {
        with_raw_elements("@@>@@@", |result| {
            assert_eq!(
                result.unwrap(),
                Elements::Multiple(vec![raw!(">"), text!("@")]),
            );
        });
        with_raw_elements("@@alpha>@@@", |result| {
            assert_eq!(
                result.unwrap(),
                Elements::Multiple(vec![raw!("alpha>"), text!("@")]),
            );
        });
    }

    #[test]
    fn wikidot_angle_raw_uses_the_overlapping_at_angle_owner() {
        with_raw_elements("@@<<A>>@@", |result| {
            assert_eq!(
                result.unwrap(),
                Elements::Multiple(vec![text!("@"), raw!("<A>"), text!("@")]),
            );
        });
    }

    #[test]
    fn wikidot_discards_empty_angle_raw() {
        with_raw_elements("@<>@", |result| {
            assert_eq!(result.unwrap(), Elements::None);
        });
    }

    #[test]
    fn left_raw_decodes_wikidot_universal_escaping_entities() {
        with_raw_elements("@<&copy; &#252; &#8212; &#x2014;>@", |result| {
            assert_eq!(
                result.unwrap(),
                Elements::Single(raw!("\u{a9} \u{fc} \u{2014} \u{2014}")),
            );
        });
    }

    #[test]
    fn left_raw_preserves_unknown_and_incomplete_entities() {
        with_raw_elements("@<&copy &not-an-entity; &copy>@", |result| {
            assert_eq!(
                result.unwrap(),
                Elements::Single(raw!("&copy &not-an-entity; &copy")),
            );
        });
    }

    #[test]
    fn double_at_raw_does_not_decode_entities() {
        with_raw_elements("@@&copy; &#252; &#8212;@@", |result| {
            assert_eq!(
                result.unwrap(),
                Elements::Single(raw!("&copy; &#252; &#8212;")),
            );
        });
    }

    #[test]
    #[should_panic(expected = "Current token is not a starting raw")]
    fn raw_rule_panics_if_called_on_non_raw_token() {
        with_raw_elements("plain text", |_| {});
    }
}
