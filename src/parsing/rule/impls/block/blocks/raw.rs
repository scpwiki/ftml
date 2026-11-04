/*
 * parsing/rule/impls/block/blocks/raw.rs
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
use super::span::wikidot_literal_with_empty_scored_spans_elided;
use crate::delayed::DelayedElement;
use std::borrow::Cow;

pub const BLOCK_RAW: BlockRule = BlockRule {
    name: "block-raw",
    accepts_names: &["raw"],
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
    trace!("Parsing raw block (name '{name}', in-head {in_head}, score {flag_score})");
    assert!(!flag_star, "Raw doesn't allow star flag");
    assert!(!flag_score, "Raw doesn't allow score flag");

    assert_block_name(&BLOCK_RAW, name);

    if parser.native_blockquote_depth().is_some() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if parser.settings().layout.legacy() && !parser.discarding_hidden_body() {
        let source = parser.full_text().inner();
        let mut owner = parser.clone();
        if in_head {
            while !matches!(owner.current().token, Token::RightBlock | Token::InputEnd) {
                owner.step()?;
            }
            owner.get_token(
                Token::RightBlock,
                ParseErrorKind::BlockMissingCloseBrackets,
            )?;
        }
        let _ = owner.get_body_text(&BLOCK_RAW)?;
        let owner_end = owner.current().span.start;
        let generated = owner.generated_in_range(opener_start..owner_end);
        if generated.is_empty() {
            let has_runtime = std::iter::once(parser.current())
                .chain(parser.remaining())
                .take_while(|token| token.span.start < owner_end)
                .any(|token| token.token == Token::RuntimeText);
            if !has_runtime
                && let Some(elements) = wikidot_literal_with_empty_scored_spans_elided(
                    &source[opener_start..owner_end],
                )
            {
                parser.update(&owner);
                return success_elements(elements);
            }
            return Err(parser.make_err(ParseErrorKind::RuleFailed));
        }
        parser.update(&owner);
        return success_elements(Element::Delayed(DelayedElement::shell(
            source,
            opener_start..owner_end,
            &generated,
        )));
    }

    let mut content = parser.get_body_text(&BLOCK_RAW)?;

    if content == "\n" {
        content = Cow::Borrowed("");
    } else if content.starts_with('\n') && content.ends_with('\n') {
        content = match content {
            Cow::Borrowed(content) => {
                Cow::Borrowed(content.trim_start_matches('\n').trim_end_matches('\n'))
            }
            Cow::Owned(content) => Cow::Owned(
                content
                    .trim_start_matches('\n')
                    .trim_end_matches('\n')
                    .to_owned(),
            ),
        };
    }

    let element = Element::Raw(content);
    success_elements(element)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn raw_block_preserves_unparsed_multiline_text_without_outer_newlines_in_wikijump() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tokenization = crate::tokenize("[[raw]]\n**not bold**\n[[/raw]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:?}");
        let [Element::Container(paragraph)] = tree.elements.as_slice() else {
            panic!("expected one paragraph, got {:?}", tree.elements);
        };
        assert_eq!(paragraph.ctype(), ContainerType::Paragraph);

        let [Element::Raw(text)] = paragraph.elements() else {
            panic!("expected one raw element, got {:?}", paragraph.elements());
        };
        assert_eq!(text.as_ref(), "**not bold**");
    }

    #[test]
    fn raw_block_markers_remain_literal_in_wikidot() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[raw]]\n**bold**\n[[/raw]]");
        let (tree, _errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(!format!("{tree:?}").contains("Raw("), "{tree:#?}");
    }

    #[test]
    fn quoted_raw_markers_remain_literal_like_wikidot() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        for input in [
            ">> [[raw]]\n>> ALPHA_RAW_DEPTH_TWO\n>> [[/raw]]",
            "> > [[raw]]\n> > ALPHA_RAW_SPACED_LITERAL\n> > [[/raw]]",
            ">> [[raw]]\n>> OMEGA_RAW_SHALLOW_CLOSE\n> [[/raw]]\n> OMEGA_AFTER_SHALLOW",
            "> [[raw]]\n> OMEGA_RAW_DEEP_CLOSE\n>> [[/raw]]\n> OMEGA_AFTER_DEEP",
            concat!(
                "> [[collapsible]]\n",
                "> [[raw]]\n",
                "> raw body\n",
                "> [[/raw]]\n",
                "> [[/collapsible]]\n",
            ),
        ] {
            let tokenization = crate::tokenize(input);
            let (tree, _errors) =
                crate::parse(&tokenization, &page_info, &settings).into();

            let rendered = format!("{tree:?}");
            assert!(!rendered.contains("Raw("), "{input:?}: {rendered}");
            assert!(rendered.contains("Text(\"raw\")"), "{input:?}: {rendered}");
        }
    }
}
