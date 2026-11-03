/*
 * parsing/rule/impls/block/blocks/gallery.rs
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
use crate::tree::{
    Gallery, GallerySelection, gallery_head_end, parse_explicit_gallery_entries,
    parse_gallery_head_arguments,
};
use std::borrow::Cow;

pub const BLOCK_GALLERY: BlockRule = BlockRule {
    name: "block-gallery",
    accepts_names: &["gallery"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn,
};

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    assert!(!flag_star, "Gallery doesn't allow star flag");
    assert!(!flag_score, "Gallery doesn't allow score flag");
    assert_block_name(&BLOCK_GALLERY, name);

    let source = parser.full_text().inner();
    let name_start = (name.as_ptr() as usize)
        .checked_sub(source.as_ptr() as usize)
        .expect("parsed gallery name belongs to the source");
    let opener_start = source[..name_start]
        .rfind("[[")
        .expect("parsed gallery name follows its opener");
    if !gallery_owns_physical_line(
        source,
        opener_start,
        parser.in_native_blockquote_line(),
        parser.settings().layout.legacy(),
    ) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let relative_head_end = gallery_head_end(&source[opener_start..])
        .ok_or_else(|| parser.make_err(ParseErrorKind::BlockMissingCloseBrackets))?;
    let head_end = opener_start + relative_head_end;
    if range_has_non_authored(parser, opener_start..head_end) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    let wikidot = parser.settings().layout.legacy();
    let arguments =
        parse_gallery_head_arguments(&source[opener_start..head_end], wikidot)
            .ok_or_else(|| parser.make_err(ParseErrorKind::BlockMalformedArguments))?;
    let _ = parser.get_head_map_with_body_start_wikidot(&BLOCK_GALLERY, in_head)?;

    let residual_opener_bracket = source[head_end..].starts_with(']');
    if let Some((entries, owner_end, residual_close_bracket)) =
        parse_explicit_gallery_entries(source, head_end)
    {
        let non_authored = range_has_non_authored(parser, head_end..owner_end);
        let generated =
            non_authored.then(|| parser.generated_in_range(opener_start..owner_end));
        let mut after_owner = parser.clone();
        while after_owner.current().span.start < owner_end {
            if after_owner.current().token == Token::InputEnd {
                return Err(parser.make_err(ParseErrorKind::EndOfInput));
            }
            after_owner.step()?;
        }
        parser.update(&after_owner);
        let element = if let Some(generated) = generated {
            Element::Delayed(DelayedElement::shell(
                source,
                opener_start..owner_end,
                &generated,
            ))
        } else {
            Element::Gallery(Gallery::new(
                &source[opener_start..owner_end],
                arguments,
                GallerySelection::Explicit(entries),
            ))
        };
        let residual_count =
            usize::from(residual_opener_bracket) + usize::from(residual_close_bracket);
        return if residual_count == 0 {
            success_elements(element)
        } else {
            ok!(Elements::Multiple(vec![
                element,
                Element::Text(Cow::Owned("]".repeat(residual_count))),
            ]))
        };
    }

    let gallery = Element::Gallery(Gallery::new(
        &source[opener_start..head_end],
        arguments,
        GallerySelection::CurrentPageFiles,
    ));
    if residual_opener_bracket {
        let mut elements = vec![gallery, text!("]")];
        if matches!(source.as_bytes().get(head_end + 1), Some(b'\r' | b'\n')) {
            elements.push(Element::LineBreak);
            parser.mark_wikidot_mixed_paragraph_ownership();
        }
        ok!(Elements::Multiple(elements))
    } else {
        success_elements(gallery)
    }
}

fn range_has_non_authored(
    parser: &Parser<'_, '_>,
    range: std::ops::Range<usize>,
) -> bool {
    parser.has_generated_in_range(range.clone())
        || std::iter::once(parser.current())
            .chain(parser.remaining())
            .take_while(|token| token.span.start < range.end)
            .any(|token| {
                token.span.end > range.start
                    && matches!(
                        token.token,
                        Token::RuntimeText
                            | Token::GeneratedPageLink
                            | Token::GeneratedTagLinks
                    )
            })
}

fn gallery_owns_physical_line(
    source: &str,
    opener_start: usize,
    quoted: bool,
    wikidot: bool,
) -> bool {
    let line_start = source[..opener_start]
        .rfind(['\n', '\r'])
        .map_or(0, |newline| newline + 1);
    let prefix = &source[line_start..opener_start];
    if prefix.is_empty()
        || !wikidot
            && prefix
                .bytes()
                .all(|byte| matches!(byte, b' ' | b'\t' | b'\0'))
    {
        return true;
    }
    if !quoted {
        return false;
    }
    prefix
        .trim_matches([' ', '\t'])
        .bytes()
        .all(|byte| byte == b'>')
}

pub(super) fn authored_gallery_line_offset(source: &str, wikidot: bool) -> Option<usize> {
    let mut line_start = 0;
    loop {
        let line_end = source[line_start..]
            .find('\n')
            .map_or(source.len(), |offset| line_start + offset);
        let line = source[line_start..line_end]
            .strip_suffix('\r')
            .unwrap_or(&source[line_start..line_end]);
        let leading = line.len() - line.trim_start_matches([' ', '\t']).len();
        let candidate = &line[leading..];
        if candidate.get(..2).is_some_and(|prefix| prefix == "[[")
            && gallery_head_end(candidate)
                .and_then(|end| parse_gallery_head_arguments(&candidate[..end], wikidot))
                .is_some()
        {
            return Some(line_start + leading);
        }
        if line_end == source.len() {
            return None;
        }
        line_start = line_end + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::parsing::rule::impls::block::RULE_BLOCK;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn gallery_line_detection_requires_a_complete_authored_head() {
        assert_eq!(
            authored_gallery_line_offset("x\n[[Gallery size=\"small\"]]", false),
            Some(2)
        );
        assert_eq!(authored_gallery_line_offset("A[[gallery]]B", false), None);
        assert_eq!(authored_gallery_line_offset("+ [[gallery]]", false), None);
        assert_eq!(authored_gallery_line_offset("[[gallery", false), None);
    }

    #[test]
    fn dense_non_line_gallery_candidates_stay_bounded() {
        let source = "A[[gallery]]B\n".repeat(2_048);
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
    }
}
