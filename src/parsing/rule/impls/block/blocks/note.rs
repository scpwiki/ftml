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
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing highlight block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "Note doesn't allow star flag");
    assert!(!flag_score, "Note doesn't allow score flag");
    assert_block_name(&BLOCK_NOTE, name);

    // Wikidot does not nest notes: an inner `[[note]]` stays literal text.
    if parser.in_note() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    let arguments = parser.get_head_map(&BLOCK_NOTE, in_head)?;

    // Wikidot's note takes no arguments: `[[note class=\"x\"]]` stays literal
    // (references-20260815-10), while a bare `[[note]]` renders
    // (references-20261025-01).
    if parser.settings().layout.legacy() && !arguments.is_empty() {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    // `[[note]]` is a block element and must start on a new line.
    // Legacy `Text_Wiki` parser also rejected inline note blocks, so preserve
    // that behavior for compatibility.
    if !parser.start_of_line() {
        return Err(parser.make_err(ParseErrorKind::NotSupportedInline));
    }

    let legacy = parser.settings().layout.legacy();
    parser.set_note_flag(legacy);
    let body = parser.get_body_elements(&BLOCK_NOTE, legacy);
    parser.set_note_flag(false);
    let (elements, errors, paragraph_safe) = body?.into();

    let element = Element::Container(Container::new(
        ContainerType::Note,
        elements,
        arguments.to_attribute_map(parser.settings()),
    ));

    // Wikidot's note is always a block, even when its body is empty.
    ok!(paragraph_safe && !legacy; element, errors)
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
