/*
 * parsing/rule/impls/block/blocks/media.rs
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
use crate::tree::{Alignment, FileSource, FloatAlignment};

pub const BLOCK_AUDIO: BlockRule = BlockRule {
    name: "block-audio",
    accepts_names: &["audio"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines: false,
    parse_fn: parse_audio,
};

pub const BLOCK_VIDEO: BlockRule = BlockRule {
    name: "block-video",
    accepts_names: &["video"],
    accepts_star: false,
    accepts_score: false,
    accepts_newlines: false,
    parse_fn: parse_video,
};

fn parse_audio<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    parse_audio_block(parser, name, (flag_star, flag_score, in_head))
}

fn parse_video<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flag_star: bool,
    flag_score: bool,
    in_head: bool,
) -> ParseResult<'r, 't, Elements<'t>> {
    parse_video_block(parser, name, (flag_star, flag_score, in_head))
}

fn parse_audio_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flags: (bool, bool, bool),
) -> ParseResult<'r, 't, Elements<'t>> {
    parse_media_block(parser, &BLOCK_AUDIO, name, flags, build_audio)
}

fn parse_video_block<'r, 't>(
    parser: &mut Parser<'r, 't>,
    name: &'t str,
    flags: (bool, bool, bool),
) -> ParseResult<'r, 't, Elements<'t>> {
    parse_media_block(parser, &BLOCK_VIDEO, name, flags, build_video)
}

fn build_audio<'t>(
    source: FileSource<'t>,
    alignment: Option<FloatAlignment>,
    attributes: crate::tree::AttributeMap<'t>,
) -> Element<'t> {
    Element::Audio {
        source,
        alignment,
        attributes,
    }
}

fn build_video<'t>(
    source: FileSource<'t>,
    alignment: Option<FloatAlignment>,
    attributes: crate::tree::AttributeMap<'t>,
) -> Element<'t> {
    Element::Video {
        source,
        alignment,
        attributes,
    }
}

fn parse_media_block<'r, 't, F>(
    parser: &mut Parser<'r, 't>,
    block_rule: &BlockRule,
    name: &'t str,
    flags: (bool, bool, bool),
    build_element: F,
) -> ParseResult<'r, 't, Elements<'t>>
where
    F: FnOnce(
        FileSource<'t>,
        Option<FloatAlignment>,
        crate::tree::AttributeMap<'t>,
    ) -> Element<'t>,
{
    let (flag_star, flag_score, in_head) = flags;
    let block_name = block_rule.name;
    debug!("Parsing media block (rule {block_name}, name {name}, in-head {in_head})");
    assert!(!flag_star, "Media blocks don't allow star flag");
    assert!(!flag_score, "Media blocks don't allow score flag");
    assert_block_name(block_rule, name);

    let (source, mut arguments) = parser.get_head_name_map(block_rule, in_head)?;
    let alignment = parse_media_alignment(parser, &mut arguments)?;

    let source = match FileSource::parse(source) {
        Some(source) => source,
        None => return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments)),
    };

    if arguments.get("src").is_some() {
        return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments));
    }

    let _autoplay = arguments.get("autoplay");

    let attributes = arguments.to_attribute_map(parser.settings());
    let element = build_element(source, alignment, attributes);
    success_elements(element)
}

fn parse_media_alignment<'r, 't>(
    parser: &mut Parser<'r, 't>,
    arguments: &mut Arguments<'t>,
) -> Result<Option<FloatAlignment>, ParseError> {
    let Some(value) = arguments.get("align") else {
        return Ok(None);
    };

    let align = match value.as_ref() {
        "left" => Alignment::Left,
        "right" => Alignment::Right,
        "center" => Alignment::Center,
        _ => return Err(parser.make_err(ParseErrorKind::BlockMalformedArguments)),
    };

    let float = false;
    Ok(Some(FloatAlignment { align, float }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn media_blocks_parse_audio_and_video_sources_alignment_and_attributes() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);

        let tokenization =
            crate::tokenize(r#"[[audio page/song.mp3 align="right" class="player"]]"#);
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:?}");

        let [Element::Container(paragraph)] = tree.elements.as_slice() else {
            panic!("expected paragraph, got {:?}", tree.elements);
        };
        let [
            Element::Audio {
                source,
                alignment,
                attributes,
            },
        ] = paragraph.elements()
        else {
            panic!("expected audio element, got {:?}", paragraph.elements());
        };

        assert_eq!(
            source,
            &FileSource::File2 {
                page: cow!("page"),
                file: cow!("song.mp3"),
            },
        );
        assert_eq!(
            *alignment,
            Some(FloatAlignment {
                align: Alignment::Right,
                float: false,
            }),
        );
        assert_eq!(
            attributes.get().get("class").map(|value| value.as_ref()),
            Some("player")
        );
        assert!(!attributes.get().contains_key("align"));

        let tokenization = crate::tokenize(
            r#"[[video https://example.com/video.mp4 align="center" title="Demo"]]"#,
        );
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:?}");

        let [Element::Container(paragraph)] = tree.elements.as_slice() else {
            panic!("expected paragraph, got {:?}", tree.elements);
        };
        let [
            Element::Video {
                source,
                alignment,
                attributes,
            },
        ] = paragraph.elements()
        else {
            panic!("expected video element, got {:?}", paragraph.elements());
        };

        assert_eq!(
            source,
            &FileSource::Url(cow!("https://example.com/video.mp4"))
        );
        assert_eq!(
            *alignment,
            Some(FloatAlignment {
                align: Alignment::Center,
                float: false,
            }),
        );
        assert_eq!(
            attributes.get().get("title").map(|value| value.as_ref()),
            Some("Demo")
        );
        assert!(!attributes.get().contains_key("align"));
    }

    #[test]
    fn media_blocks_reject_malformed_arguments_and_allow_missing_alignment() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);

        let tokenization = crate::tokenize("[[audio song.mp3]]");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(errors.is_empty(), "{errors:?}");
        let [Element::Container(paragraph)] = tree.elements.as_slice() else {
            panic!("expected paragraph, got {:?}", tree.elements);
        };
        let [Element::Audio { alignment, .. }] = paragraph.elements() else {
            panic!("expected audio element, got {:?}", paragraph.elements());
        };
        assert_eq!(*alignment, None);

        let tokenization = crate::tokenize("[[audio]]");
        let (_tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();
        assert!(
            errors
                .iter()
                .any(|error| error.kind() == ParseErrorKind::BlockMissingName),
            "[[audio]] should report missing name: {errors:?}",
        );

        for input in [
            "[[audio one/two/three/four.mp3]]",
            r#"[[audio song.mp3 src="duplicate"]]"#,
            r#"[[video clip.mp4 align="top"]]"#,
        ] {
            let tokenization = crate::tokenize(input);
            let (_tree, errors) =
                crate::parse(&tokenization, &page_info, &settings).into();
            assert!(
                errors
                    .iter()
                    .any(|error| error.kind() == ParseErrorKind::BlockMalformedArguments),
                "{input} should report malformed arguments: {errors:?}",
            );
        }
    }
}
