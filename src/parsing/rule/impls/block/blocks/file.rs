/*
 * parsing/rule/impls/block/blocks/file.rs
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
use crate::parsing::collect::{CommentElidedText, consume_iftags_segment};
use crate::parsing::trim_wikidot_ascii;
use std::borrow::Cow;
pub const BLOCK_FILE: BlockRule = BlockRule {
    name: "block-file",
    accepts_names: &["file"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines_outside_body: false,
    parse_fn,
};

struct WikidotFileHead<'t> {
    file: Cow<'t, str>,
    label: Cow<'t, str>,
    trailing_bracket: bool,
}

fn parse_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &str,
    _opener_start: usize,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Parsing file link block (name '{name}', in-head {in_head})");
    assert!(!flag_star, "File doesn't allow star flag");
    assert!(!flag_score, "File doesn't allow score flag");
    assert_block_name(&BLOCK_FILE, name);

    let (file, label, trailing_bracket) = if parser.settings().layout.legacy() {
        let head = parse_wikidot_file_head(parser, in_head)?;
        (head.file, head.label, head.trailing_bracket)
    } else {
        let (file, label) = parser.get_head_value_resolved(
            &BLOCK_FILE,
            in_head,
            parse_evidenced_file_link,
        )?;
        (file, label, false)
    };

    let link = Element::FileLink { file, label };
    if trailing_bracket {
        ok!(Elements::Multiple(vec![link, text!("]")]))
    } else {
        success_elements(link)
    }
}

fn parse_wikidot_file_head<'r, 't>(
    parser: &mut Parser<'r, 't>,
    in_head: bool,
) -> Result<WikidotFileHead<'t>, ParseError>
where
    'r: 't,
{
    if !parser.settings().allow_local_paths {
        return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
    }
    if !in_head {
        return Err(parser.make_err(ParseErrorKind::BlockMissingArguments));
    }

    let start = parser.current().span.start;
    let mut elisions = Vec::new();
    loop {
        if parser.current().token == Token::LeftBlock
            && let Some(segment) = consume_iftags_segment(parser)?
        {
            elisions.extend(segment.elisions());
            continue;
        }

        let current = parser.current();
        let trailing_bracket = match current.token {
            Token::RightBlock => false,
            Token::RightLink if current.slice == "]]]" => true,
            Token::LineBreak | Token::ParagraphBreak => {
                return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
            }
            Token::InputEnd => return Err(parser.make_err(ParseErrorKind::EndOfInput)),
            _ => {
                parser.step()?;
                continue;
            }
        };

        let span = start..current.span.start;
        let value =
            CommentElidedText::new(parser.full_text().inner(), span, elisions).into_cow();
        let (file, label) = parse_evidenced_file_link(parser, Some(value))?;
        parser.step()?;
        return Ok(WikidotFileHead {
            file,
            label,
            trailing_bracket,
        });
    }
}

fn parse_evidenced_file_link<'t>(
    parser: &Parser<'_, 't>,
    value: Option<Cow<'t, str>>,
) -> Result<(Cow<'t, str>, Cow<'t, str>), ParseError> {
    if !parser.settings().allow_local_paths {
        return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
    }

    match value {
        Some(Cow::Borrowed(value)) => {
            let (file, label) = parse_evidenced_file_link_str(parser, Some(value))?;
            Ok((Cow::Borrowed(file), Cow::Borrowed(label)))
        }
        Some(Cow::Owned(value)) => {
            let (file, label) = parse_evidenced_file_link_str(parser, Some(&value))?;
            Ok((Cow::Owned(file.to_owned()), Cow::Owned(label.to_owned())))
        }
        None => {
            let (file, label) = parse_evidenced_file_link_str(parser, None)?;
            Ok((Cow::Borrowed(file), Cow::Borrowed(label)))
        }
    }
}

fn parse_evidenced_file_link_str<'t, 'v>(
    parser: &Parser<'_, 't>,
    value: Option<&'v str>,
) -> Result<(&'v str, &'v str), ParseError> {
    let value = match value {
        Some(value) => value,
        None => return Err(parser.make_err(ParseErrorKind::BlockMissingArguments)),
    };
    let value = if parser.settings().layout.legacy() {
        trim_wikidot_ascii(value)
    } else {
        value.trim()
    };
    if !parser.settings().layout.legacy() {
        let Some((file, label)) = value.split_once(" | ") else {
            return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
        };
        if !is_evidenced_file_name(file) || label.is_empty() || label.contains('|') {
            return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
        }
        return Ok((file, label));
    }

    let (file, label) = match value.split_once('|') {
        Some((file, label)) => {
            let file = file.trim_matches([' ', '\t', '\r', '\n']);
            let label = label.trim_matches([' ', '\t', '\r', '\n']);
            (
                file,
                if label.is_empty() {
                    file_name(file)
                } else {
                    label
                },
            )
        }
        None => (
            value,
            if value.bytes().any(|byte| matches!(byte, b' ' | b'\t')) {
                value
            } else {
                file_name(value)
            },
        ),
    };
    if !is_evidenced_wikidot_file_source(file) {
        return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
    }

    Ok((file, label))
}

fn file_name(source: &str) -> &str {
    source.rsplit('/').next().unwrap_or(source)
}

fn is_evidenced_wikidot_file_source(source: &str) -> bool {
    !source.is_empty()
}

fn is_evidenced_file_name(source: &str) -> bool {
    !matches!(source, "" | "." | "..") && is_evidenced_file_path_part(source)
}

fn is_evidenced_file_path_part(part: &str) -> bool {
    part.chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::render::Render;
    use crate::render::html::HtmlRender;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn wikidot_file_link_accepts_live_evidenced_forms_without_file_state() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        // Saved-page cases record--a31ae3233a367367f65eb4cb, record--95406cc12956c54f1dd849bf, record--5a933922d44be7b45247533e, and record--c99e6dd95cff0355c4ac6408 produced anchors on Wikidot without uploaded-file state.
        for (source, file, label) in [
            ("[[file elements.tsv]]", "elements.tsv", "elements.tsv"),
            (
                "[[file other-page/elements.tsv | Download Catalog]]",
                "other-page/elements.tsv",
                "Download Catalog",
            ),
            (
                "[[file elements.tsv|Download Catalog]]",
                "elements.tsv",
                "Download Catalog",
            ),
            ("[[file elements.tsv | ]]", "elements.tsv", "elements.tsv"),
        ] {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();

            assert!(errors.is_empty(), "{source} should parse: {errors:#?}");
            let [Element::Container(paragraph)] = tree.elements.as_slice() else {
                panic!(
                    "{source} should produce one paragraph: {:#?}",
                    tree.elements
                );
            };
            assert_eq!(
                paragraph.elements(),
                [Element::FileLink {
                    file: cow!(file),
                    label: cow!(label),
                }],
            );
        }
    }

    #[test]
    fn wikidot_file_link_preserves_opaque_live_path_and_label_data() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for (source, file, label, trailing) in [
            ("[[file .]]", ".", ".", false),
            ("[[file ..]]", "..", "..", false),
            (
                "[[file ../elements.tsv | Download Catalog]]",
                "../elements.tsv",
                "Download Catalog",
                false,
            ),
            (
                "[[file elements.tsv | label | extra]]",
                "elements.tsv",
                "label | extra",
                false,
            ),
            (
                "[[file path with spaces/elements.tsv]]",
                "path with spaces/elements.tsv",
                "path with spaces/elements.tsv",
                false,
            ),
            ("[[file a.txt#x]]", "a.txt#x", "a.txt#x", false),
            ("[[file a.txt?x=1]]", "a.txt?x=1", "a.txt?x=1", false),
            ("[[file 日本語.txt]]", "日本語.txt", "日本語.txt", false),
            (
                "[[file elements.tsv]]]",
                "elements.tsv",
                "elements.tsv",
                true,
            ),
        ] {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();

            assert!(errors.is_empty(), "{source} should parse: {errors:#?}");
            let [Element::Container(paragraph)] = tree.elements.as_slice() else {
                panic!(
                    "{source} should produce one paragraph: {:#?}",
                    tree.elements
                );
            };
            let [
                Element::FileLink {
                    file: actual_file,
                    label: actual_label,
                },
                rest @ ..,
            ] = paragraph.elements()
            else {
                panic!(
                    "{source} should produce a file link: {:#?}",
                    paragraph.elements()
                );
            };
            assert_eq!(actual_file, file, "{source}");
            assert_eq!(actual_label, label, "{source}");
            assert_eq!(rest == [text!("]")], trailing, "{source}");
        }
    }

    #[test]
    fn wikidot_file_link_renders_live_evidenced_target_and_label_shapes() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);

        for (source, expected) in [
            (
                "[[file elements.tsv]]",
                r#"<p><a href="https://sandbox.wjfiles.com/local--files/some-page/elements.tsv">elements.tsv</a></p>"#,
            ),
            (
                "[[file other-page/elements.tsv | Download Catalog]]",
                r#"<p><a href="https://sandbox.wjfiles.com/local--files/other-page/elements.tsv">Download Catalog</a></p>"#,
            ),
            (
                "[[file elements.tsv|Download Catalog]]",
                r#"<p><a href="https://sandbox.wjfiles.com/local--files/some-page/elements.tsv">Download Catalog</a></p>"#,
            ),
            (
                "[[file elements.tsv | ]]",
                r#"<p><a href="https://sandbox.wjfiles.com/local--files/some-page/elements.tsv">elements.tsv</a></p>"#,
            ),
        ] {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();

            assert!(errors.is_empty(), "{source} should parse: {errors:#?}");
            assert_eq!(
                HtmlRender.render(&tree, &page_info, &settings).body,
                expected,
            );
        }
    }

    #[test]
    fn wikijump_layout_keeps_the_native_file_link_grammar() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);

        for source in [
            "[[file elements.tsv]]",
            "[[file other-page/elements.tsv | Download Catalog]]",
            "[[file elements.tsv|Download Catalog]]",
            "[[file elements.tsv | ]]",
        ] {
            let tokenization = crate::tokenize(source);
            let (tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();

            assert!(
                errors
                    .iter()
                    .any(|error| error.kind() == ParseErrorKind::BlockMalformedArguments),
                "{source} should retain the native grammar: {errors:#?}",
            );
            assert!(
                !tree
                    .elements
                    .iter()
                    .any(|element| matches!(element, Element::FileLink { .. })),
            );
        }
    }

    #[test]
    fn file_link_is_disabled_for_forum_posts() {
        let page_info = PageInfo::dummy();
        let settings =
            WikitextSettings::from_mode(WikitextMode::ForumPost, Layout::Wikidot);
        let tokenization = crate::tokenize("[[file elements.tsv | Download Catalog]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(
            errors
                .iter()
                .any(|error| error.kind() == ParseErrorKind::BlockMalformedArguments),
            "{errors:#?}",
        );
        assert!(
            !tree
                .elements
                .iter()
                .any(|element| matches!(element, Element::FileLink { .. })),
        );
    }
}
