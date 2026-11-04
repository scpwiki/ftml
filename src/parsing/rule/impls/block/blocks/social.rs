/*
 * parsing/rule/impls/block/blocks/social.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2026 Wikijump Team
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

use super::prelude::*;
use crate::delayed::DelayedElement;
use crate::parsing::collect::{
    collect_text_resolved_head, consume_valid_comment, iftags_segment_fused,
};
use crate::tree::SocialButtons;
use std::borrow::Cow;

pub const BLOCK_SOCIAL: BlockRule = BlockRule {
    name: "block-social",
    accepts_names: &["social"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: false,
    parse_fn,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing social block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Social doesn't allow star flag");
    assert!(!flag_score, "Social doesn't allow score flag");
    assert_block_name(&BLOCK_SOCIAL, name);
    parser.check_page_syntax()?;
    if !parser.settings().layout.legacy() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let source = parser.full_text().inner();
    let name_start = opener_start
        + 2
        + source[opener_start + 2..]
            .bytes()
            .take_while(|byte| matches!(byte, b' ' | b'\t'))
            .count();

    if in_head && matches!(parser.current().token, Token::RightBlock | Token::RightLink) {
        let name_end = name_start + name.len();
        if &source[name_end..parser.current().span.start] == " " {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
    }

    if !in_head {
        return success_elements(Element::SocialButtons(SocialButtons::parse("")));
    }

    let start = parser.current().span.start;
    let mut head_probe = parser.clone();
    let mut saw_seam = false;
    let mut fused_bracket_close = false;
    loop {
        let current = parser.current();
        if current.token == Token::LeftComment {
            let mut probe = parser.clone();
            if let Ok(range) = consume_valid_comment(&mut probe) {
                saw_seam = true;
                while parser.current().span.start < range.end
                    && parser.current().token != Token::InputEnd
                {
                    parser.step()?;
                }
                continue;
            }
        }
        if current.token == Token::LeftBlock
            && let Some(segment) = iftags_segment_fused(parser)
        {
            saw_seam = true;
            while parser.current().span.end <= segment.range.end
                && parser.current().token != Token::InputEnd
            {
                parser.step()?;
            }
            if segment.fused_closer {
                let completes = {
                    let current = parser.current();
                    current.span.start < segment.range.end
                        && current.span.end > segment.range.end
                        && matches!(parser.remaining().first(), Some(next) if next.token == Token::RightBracket && next.span.start == segment.range.end + 1)
                };
                if completes {
                    parser.step()?;
                    fused_bracket_close = true;
                }
            }
            continue;
        }
        let fused_bracket_close = std::mem::take(&mut fused_bracket_close)
            && current.token == Token::RightBracket;
        let trailing_bracket = if fused_bracket_close {
            false
        } else {
            match current.token {
                Token::RightBlock => false,
                Token::RightLink if current.slice == "]]]" => true,
                Token::LineBreak | Token::ParagraphBreak => {
                    return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
                }
                Token::InputEnd => {
                    return Err(parser.make_err(ParseErrorKind::EndOfInput));
                }
                _ => {
                    parser.step()?;
                    continue;
                }
            }
        };

        let head_range = start..current.span.start;
        if parser.has_runtime_scalar_in_range(head_range.clone()) {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
        if parser.has_generated_in_range(head_range) {
            let owner_end = current.span.end;
            let generated = parser.generated_in_range(opener_start..owner_end);
            parser.step()?;
            return success_elements(Element::Delayed(DelayedElement::shell(
                source,
                opener_start..owner_end,
                &generated,
            )));
        }
        let head = &parser.full_text().inner()[start..current.span.start];
        let head = if saw_seam {
            collect_text_resolved_head(&mut head_probe, parser.rule(), current.span.start)
                .unwrap_or(Cow::Borrowed(head))
        } else {
            Cow::Borrowed(head)
        };
        let social = Element::SocialButtons(SocialButtons::parse(&head));
        parser.step()?;
        return if trailing_bracket {
            ok!(Elements::Multiple(vec![social, text!("]")]))
        } else {
            success_elements(social)
        };
    }
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::ParseErrorKind;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::{Element, SocialSelection, SocialService};

    fn parse_social(input: &str) -> Element<'static> {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokens = crate::tokenize(input);
        let (tree, errors) = crate::parse(&tokens, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(tree.elements.len(), 1, "{:#?}", tree.elements);
        match &tree.elements[0] {
            Element::SocialButtons(_) => tree.elements[0].to_owned(),
            Element::Container(container) => {
                let [Element::SocialButtons(social)] = container.elements() else {
                    panic!(
                        "social paragraph did not contain exactly one typed request: {container:#?}"
                    );
                };
                Element::SocialButtons(social.clone())
            }
            element => panic!("social block did not stay typed: {element:#?}"),
        }
    }

    #[test]
    fn wikidot_social_selection_matches_live_token_boundaries() {
        let Element::SocialButtons(default) = parse_social("[[social]]") else {
            panic!("social block did not stay typed");
        };
        assert_eq!(default.selection(), None);

        let Element::SocialButtons(selected) =
            parse_social("[[social reddit, not-a-service,facebook]]")
        else {
            panic!("social block did not stay typed");
        };
        assert_eq!(
            selected.selection(),
            Some(
                &[
                    SocialSelection::Service(SocialService::Reddit),
                    SocialSelection::Service(SocialService::Facebook),
                ][..]
            )
        );

        let Element::SocialButtons(invalid) = parse_social("[[social Reddit,FACEBOOK]]")
        else {
            panic!("social block did not stay typed");
        };
        assert_eq!(
            invalid.selection(),
            Some(&[SocialSelection::Empty, SocialSelection::Empty][..])
        );
    }

    #[test]
    fn wikidot_social_space_only_head_stays_literal_but_tab_only_head_is_active() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        let tokens = crate::tokenize("[[social ]]");
        let (tree, errors) = crate::parse(&tokens, &page_info, &settings).into();
        assert!(
            errors.iter().any(|error| {
                error.rule() == "block-social"
                    && error.kind() == ParseErrorKind::RuleFailed
            }),
            "{errors:#?}",
        );
        assert_eq!(
            HtmlRender.render(&tree, &page_info, &settings).body,
            "<p>[[social ]]</p>"
        );

        let Element::SocialButtons(tab_only) = parse_social("[[social\t]]") else {
            panic!("tab-only social head did not stay typed");
        };
        assert_eq!(tab_only.selection(), None);
    }
}
