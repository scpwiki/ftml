/*
 * parsing/rule/impls/block/blocks/note.rs
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
use super::{
    consume_wikidot_flat_head_break, drop_wikidot_trailing_vanish_breaks,
    trim_wikidot_trailing_explicit_list_break,
    unwrap_wikidot_paragraph_after_explicit_list,
    wikidot_eject_after_lost_owner_collapsible, wikidot_flat_pair_for_head,
};
use crate::parsing::parser::block_pairs::WikidotBlockKind;
use crate::tree::{PartialElement, WikidotBlockShell};

pub const BLOCK_NOTE: BlockRule = BlockRule {
    name: "block-note",
    accepts_names: &["note"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: true,
    parse_fn: parse_note_block,
};

fn parse_note_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing highlight block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Note doesn't allow star flag");
    assert!(!flag_score, "Note doesn't allow score flag");
    assert_block_name(&BLOCK_NOTE, name);
    if parser.settings().layout.legacy()
        && !parser.discarding_hidden_body()
        && parser.wikidot_byte_on_heading_line(opener_start)
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if parser.in_note() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let (arguments, body_start) =
        parser.get_head_map_with_body_start(&BLOCK_NOTE, in_head)?;

    if parser.settings().layout.legacy() && !arguments.is_empty() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    if let Some(pair) = wikidot_flat_pair_for_head(parser, opener_start)
        && pair.kind == WikidotBlockKind::Note
    {
        consume_wikidot_flat_head_break(parser, false)?;
        let shell = WikidotBlockShell::Note { head: opener_start };
        return ok!(Element::Partial(PartialElement::WikidotBlockOpen(shell)));
    }

    let head_ended_at_break = matches!(
        parser.current().token,
        Token::LineBreak | Token::ParagraphBreak | Token::InputEnd
    );
    if !parser.start_of_line() && !head_ended_at_break {
        return Err(parser.make_err(ParseErrorKind::NotSupportedInline));
    }

    // First-fit binding: a scan-seen head with a visible same-kind closer
    // after it lost that closer to an outer opener and must stay literal.
    // Scan-blind heads (zoned closers, dropped arguments) and truly unclosed
    // heads fall through to the legacy path, which already fails unclosed.
    if parser.settings().layout.legacy()
        && parser
            .wikidot_block_pair_at_head_byte(opener_start)
            .is_none()
        && (parser.discarding_hidden_body()
            || parser
                .wikidot_scan_shows_stolen_head(WikidotBlockKind::Note, opener_start)
            // d446-neighbors: a note head inside a list item body with no
            // `[[/note]]` anywhere after it stays literal on live (a6/a7/a9);
            // the legacy body path would otherwise form a div ending at the
            // item boundary. Scan-blind zoned closers still fall through.
            || (parser.wikidot_li_collect_top().is_some()
                && !parser.has_body_end_block(&BLOCK_NOTE)))
    {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let legacy = parser.settings().layout.legacy();
    parser.set_note_flag(legacy);
    let body_start_byte = parser.current().span.start;
    let body = if legacy {
        parser.get_body_elements_with_context(&BLOCK_NOTE, legacy, body_start, None)
    } else {
        parser.get_body_elements(&BLOCK_NOTE, legacy)
    };
    parser.set_note_flag(false);
    let (mut elements, errors, paragraph_safe) = body?.into();
    let body_end_byte = parser.current().span.start;
    let mut ejected = Vec::new();
    if legacy {
        trim_wikidot_trailing_explicit_list_break(&mut elements);
        drop_wikidot_trailing_vanish_breaks(&mut elements);
        unwrap_wikidot_paragraph_after_explicit_list(&mut elements);
        if arguments.is_empty()
            && let Some(after) = wikidot_eject_after_lost_owner_collapsible(
                parser,
                &mut elements,
                body_start_byte,
                body_end_byte,
            )
        {
            ejected = after;
        }
    }
    let row_suffix = if legacy {
        crate::parsing::rule::impls::eject_wikidot_row_suffix(parser, &mut elements)
    } else {
        Vec::new()
    };

    let element = Element::Container(Container::new(
        ContainerType::Note,
        elements,
        arguments.to_attribute_map(parser.settings()),
    ));

    // In the Wikidot layout the note stays paragraph-unsafe: text following
    // the closer is a bare continuation with no fresh paragraph, exactly like
    // the generic div block.
    if row_suffix.is_empty() && ejected.is_empty() {
        ok!(paragraph_safe && !legacy; element, errors)
    } else {
        let mut output = vec![element];
        output.extend(ejected);
        output.extend(row_suffix);
        ok!(false; output, errors)
    }
}

#[cfg(test)]
mod tests {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::{Render, html::HtmlRender};
    use crate::settings::{WikitextMode, WikitextSettings};

    fn render(source: &str, layout: Layout) -> (String, Vec<crate::parsing::ParseError>) {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
        let mut source = source.to_owned();
        crate::preprocess_for_layout(&mut source, settings.layout);
        let tokenization = crate::tokenize(&source);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        let html = HtmlRender.render(&tree, &page_info, &settings).body;

        (html, errors)
    }

    #[test]
    fn wikidot_note_matches_live_pagepreview() {
        let (html, errors) = render("[[note]]\nBody text.\n[[/note]]", Layout::Wikidot);
        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html, "<div class=\"wiki-note\"><p>Body text.</p></div>");

        let (html, _) = render(
            "[[note]]\n[[note]]\nB\n[[/note]]\n[[/note]]",
            Layout::Wikidot,
        );
        assert_eq!(html.matches("wiki-note").count(), 1, "{html}");
        assert!(html.contains("[[note]]"), "{html}");

        let (html, _) = render("[[note class=\"x\"]]\nB\n[[/note]]", Layout::Wikidot);
        assert!(!html.contains("wiki-note"), "{html}");

        let (html, _) = render("[[note]]\nNo close.", Layout::Wikidot);
        assert!(!html.contains("wiki-note"), "{html}");
        assert!(html.contains("[[note]]"), "{html}");
    }

    #[test]
    fn wikijump_note_uses_native_dom_and_preserves_attributes() {
        let (html, errors) = render(
            "[[note class=\"custom\" data-kind=\"example\"]]\nBody\n[[/note]]",
            Layout::Wikijump,
        );

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(
            html,
            r#"<p><div class="wj-note custom" data-kind="example">Body</div></p>"#,
        );
    }

    #[test]
    fn wikijump_note_rejects_inline_openers() {
        let (html, errors) = render("prefix [[note]]Body[[/note]]", Layout::Wikijump);

        assert!(
            errors.iter().any(|error| error.kind()
                == crate::parsing::ParseErrorKind::NotSupportedInline),
            "html={html}; errors={errors:#?}",
        );
        assert!(!html.contains("class=\"wj-note"), "{html}");
    }

    #[test]
    fn wikijump_nested_notes_keep_native_ownership() {
        let source = "[[note]]\nA\n[[note]]\nB\n[[/note]]\nC\n[[/note]]";
        let (html, errors) = render(source, Layout::Wikijump);

        assert!(errors.is_empty(), "{errors:#?}");
        assert_eq!(html.matches(r#"class="wj-note""#).count(), 2, "{html}");
        assert!(!html.contains("[[note]]"), "{html}");
        assert!(!html.contains("[[/note]]"), "{html}");
    }
}
