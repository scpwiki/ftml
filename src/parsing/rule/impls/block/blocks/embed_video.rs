/*
 * parsing/rule/impls/block/blocks/embed_video.rs
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
use crate::parsing::parser::QuoteScanOutcome;
use crate::tree::EmbedVideo;

pub const BLOCK_EMBED_VIDEO: BlockRule = BlockRule {
    name: "block-embedvideo",
    accepts_names: &["embedvideo"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: false,
    parse_fn,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    assert!(!flag_star, "EmbedVideo doesn't allow star flag");
    assert!(!flag_score, "EmbedVideo doesn't allow score flag");
    assert_block_name(&BLOCK_EMBED_VIDEO, name);

    if in_head {
        return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
    }

    let source = parser.full_text().inner();
    let body_cursor = parser.current().span.start;
    let normal_length = "[[embedvideo]]".len();
    let extra_length = "[[embedvideo]]]".len();
    // Wikidot ignores spaces and tabs between the embedvideo head's closing
    // brackets and the end of its physical line (the same convention as the
    // div head): `[[embedvideo]] \n` opens the same block as
    // `[[embedvideo]]\n`. The head check therefore runs against the end of
    // the head itself, before any such trailing whitespace. Whitespace
    // followed by body text on the same line is not a head ending, so
    // `[[embedvideo]] X[[/embedvideo]]` stays literal.
    let head_end = source[..body_cursor].trim_end_matches([' ', '\t']).len();
    if head_end < body_cursor {
        let line_rest = source[head_end..].lines().next().unwrap_or("");
        if !line_rest
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
        {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
    }
    let opener_start = if head_end >= normal_length
        && source[head_end - normal_length..head_end]
            .eq_ignore_ascii_case("[[embedvideo]]")
    {
        head_end - normal_length
    } else if head_end >= extra_length
        && source[head_end - extra_length..head_end]
            .eq_ignore_ascii_case("[[embedvideo]]]")
    {
        head_end - extra_length
    } else {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    };
    if parser.has_runtime_literal_in_range(opener_start..body_cursor)
        || parser.has_runtime_scalar_in_range(opener_start..body_cursor)
        || parser.has_generated_in_range(opener_start..body_cursor)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let preceding_backslashes = source[..opener_start]
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'\\')
        .count();
    if preceding_backslashes % 2 == 1 {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let payload_start = opener_start + normal_length;

    let opener_quote_depth = quote_depth(source, opener_start);
    let mut close = parser.clone();
    let initial_start = close.current().span.start;
    if parser.quote_scan_outcome((
        BLOCK_EMBED_VIDEO.name,
        opener_quote_depth,
        true,
        initial_start,
    )) == Some(QuoteScanOutcome::Missing)
    {
        return Err(parser.make_err(ParseErrorKind::BlockExpectedEnd));
    }
    let mut traversed_token_starts = Vec::new();
    loop {
        #[cfg(test)]
        parser.increment_quote_scan_token_visits();
        let current = close.current();
        traversed_token_starts.push(current.span.start);

        if matches!(
            current.token,
            Token::RuntimeText | Token::GeneratedPageLink | Token::GeneratedTagLinks
        ) {
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }

        if current.token == Token::LeftBlockEnd
            && quote_depth(source, current.span.start) == opener_quote_depth
        {
            let body_end = current.span.start;
            let mut after_close = close.clone();
            if let Ok((_close_name, residual_close_bracket)) =
                after_close.get_wikidot_end_block_with_residual()
            {
                let owner_end = after_close
                    .current()
                    .span
                    .start
                    .saturating_sub(usize::from(residual_close_bracket));
                let owner = &source[opener_start..owner_end];
                let payload = &source[payload_start..body_end];
                parser.update(&after_close);

                let embed_video = Element::EmbedVideo(EmbedVideo::new(owner, payload));
                return if residual_close_bracket {
                    ok!(Elements::Multiple(vec![embed_video, text!("]")]))
                } else {
                    success_elements(embed_video)
                };
            }
        }

        if current.token == Token::InputEnd {
            parser.cache_quote_scan_outcomes(
                BLOCK_EMBED_VIDEO.name,
                opener_quote_depth,
                true,
                &traversed_token_starts,
                QuoteScanOutcome::Missing,
            );
            return Err(parser.make_err(ParseErrorKind::BlockExpectedEnd));
        }
        close.step().expect("input always ends with InputEnd");
    }
}

fn quote_depth(source: &str, position: usize) -> usize {
    let line_start = source[..position]
        .rfind(['\n', '\r'])
        .map_or(0, |newline| newline + 1);
    source[line_start..position]
        .trim_start_matches([' ', '\t'])
        .bytes()
        .take_while(|byte| *byte == b'>')
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::rule::impls::block::RULE_BLOCK;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn missing_closer_scan_visits_each_token_at_most_once() {
        let source = "[[embedvideo]]".repeat(2_048);
        let tokenization = crate::tokenize(&source);
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let mut parser = Parser::new(&tokenization, &page_info, &settings);

        while parser.current().token != Token::InputEnd {
            if parser.current().token == Token::LeftBlock {
                let _ = RULE_BLOCK.try_consume(&mut parser);
            }
            parser.step().expect("input end remains available");
        }

        assert!(
            parser.quote_scan_token_visits() <= tokenization.tokens().len(),
            "{} visits for {} tokens",
            parser.quote_scan_token_visits(),
            tokenization.tokens().len(),
        );
    }
}
