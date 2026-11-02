/*
 * test/image_paragraphs.rs
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

use crate::data::PageInfo;
use crate::layout::Layout;
use crate::settings::{WikitextMode, WikitextSettings};
use crate::tree::{Alignment, ContainerType, Element, FloatAlignment, SyntaxTree};

#[derive(Debug)]
struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, _record: &log::Record<'_>) {}

    fn flush(&self) {}
}

static TEST_LOGGER: TestLogger = TestLogger;
static TEST_LOGGER_INIT: std::sync::Once = std::sync::Once::new();

fn enable_test_logging() {
    TEST_LOGGER_INIT.call_once(|| {
        let _ = log::set_logger(&TEST_LOGGER);
        log::set_max_level(log::LevelFilter::Trace);
    });
}

fn with_wikidot_tree(input: &str, check: impl FnOnce(&SyntaxTree<'_>)) {
    enable_test_logging();

    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = crate::tokenize(input);
    let result = crate::parse(&tokenization, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(errors.is_empty(), "unexpected parse errors: {errors:?}");
    check(&tree);
}

fn image_alignment(element: &Element<'_>) -> Option<FloatAlignment> {
    match element {
        Element::Image { alignment, .. } => *alignment,
        _ => panic!("expected an image element, got {element:?}"),
    }
}

fn assert_single_text(elements: &[Element<'_>], expected: &str) {
    match elements {
        [Element::Text(actual)] => assert_eq!(actual, expected),
        _ => panic!("expected one text element, got {elements:?}"),
    }
}

#[test]
fn aligned_image_line_closes_paragraph_without_trailing_break() {
    let cases = [
        (
            "f<",
            FloatAlignment {
                align: Alignment::Left,
                float: true,
            },
        ),
        (
            "f>",
            FloatAlignment {
                align: Alignment::Right,
                float: true,
            },
        ),
        (
            "<",
            FloatAlignment {
                align: Alignment::Left,
                float: false,
            },
        ),
        (
            ">",
            FloatAlignment {
                align: Alignment::Right,
                float: false,
            },
        ),
        (
            "=",
            FloatAlignment {
                align: Alignment::Center,
                float: false,
            },
        ),
        (
            "f=",
            FloatAlignment {
                align: Alignment::Center,
                float: true,
            },
        ),
    ];

    for (prefix, expected_alignment) in cases {
        let input = format!("text\n[[{prefix}image https://example.com/a.png]]");
        with_wikidot_tree(&input, |tree| {
            let [Element::Container(paragraph), image] = tree.elements.as_slice() else {
                panic!("expected paragraph then image, got {:?}", tree.elements);
            };

            assert_eq!(paragraph.ctype(), ContainerType::Paragraph);
            assert_single_text(paragraph.elements(), "text");
            assert_eq!(image_alignment(image), Some(expected_alignment));
        });
    }
}

#[test]
fn aligned_image_after_unaligned_image_drops_separator_break() {
    with_wikidot_tree(
        "[[image https://example.com/a.png]]\n[[f<image https://example.com/a.png]]",
        |tree| {
            assert_eq!(tree.elements.len(), 2, "got {:?}", tree.elements);
            assert_eq!(image_alignment(&tree.elements[0]), None);
            assert_eq!(
                image_alignment(&tree.elements[1]),
                Some(FloatAlignment {
                    align: Alignment::Left,
                    float: true
                }),
            );
        },
    );
}

#[test]
fn break_before_unaligned_image_is_kept_but_not_before_aligned_image() {
    with_wikidot_tree(
        "text\n[[image https://example.com/a.png]]\n[[f<image https://example.com/a.png]]",
        |tree| {
            let breaks = tree
                .elements
                .iter()
                .filter(|element| **element == Element::LineBreak)
                .count();
            assert_eq!(breaks, 1, "got {:?}", tree.elements);

            let images: Vec<usize> = tree
                .elements
                .iter()
                .enumerate()
                .filter_map(|(index, element)| {
                    matches!(element, Element::Image { .. }).then_some(index)
                })
                .collect();
            assert_eq!(images.len(), 2, "got {:?}", tree.elements);
            assert_eq!(images[1], images[0] + 1, "got {:?}", tree.elements);
            assert_eq!(tree.elements[images[0] - 1], Element::LineBreak);
        },
    );
}

#[test]
fn earlier_breaks_in_paragraph_before_aligned_image_are_kept() {
    with_wikidot_tree("aaa\nbbb\n[[f<image https://example.com/a.png]]", |tree| {
        let [Element::Container(paragraph), image] = tree.elements.as_slice() else {
            panic!("expected paragraph then image, got {:?}", tree.elements);
        };

        assert_eq!(paragraph.ctype(), ContainerType::Paragraph);
        match paragraph.elements() {
            [
                Element::Text(first),
                Element::LineBreak,
                Element::Text(second),
            ] => {
                assert_eq!(first, "aaa");
                assert_eq!(second, "bbb");
            }
            _ => panic!("unexpected paragraph elements: {:?}", paragraph.elements()),
        }
        assert_eq!(
            image_alignment(image),
            Some(FloatAlignment {
                align: Alignment::Left,
                float: true
            }),
        );
    });
}

#[test]
fn image_paragraphs_fixture_paragraphs_have_no_trailing_breaks() {
    let input = "[[image https://example.com/a.png]]\n\nx [[image https://example.com/a.png]] y\n\ntext\n[[image https://example.com/a.png]]\n\n[[image https://example.com/a.png]]\ntext\n\nx [[<image https://example.com/a.png]] y\n\n[[f<image https://example.com/a.png]]\ntext\n\ntext\n[[f<image https://example.com/a.png]]\n";

    with_wikidot_tree(input, |tree| {
        let mut paragraph_texts = Vec::new();
        let mut alignments = Vec::new();

        for element in &tree.elements {
            match element {
                Element::Container(container)
                    if container.ctype() == ContainerType::Paragraph =>
                {
                    assert!(
                        !matches!(container.elements().last(), Some(Element::LineBreak)),
                        "paragraph ends with a line break: {:?}",
                        container.elements(),
                    );
                    let mut text = String::new();
                    for child in container.elements() {
                        match child {
                            Element::Text(value) => text.push_str(value),
                            _ => panic!("unexpected paragraph child: {child:?}"),
                        }
                    }
                    paragraph_texts.push(text);
                }
                Element::Image { .. } => alignments.push(image_alignment(element)),
                _ => {}
            }
        }

        assert_eq!(paragraph_texts, ["x", "y", "text", "text"]);
        assert_eq!(
            alignments,
            [
                None,
                None,
                None,
                None,
                Some(FloatAlignment {
                    align: Alignment::Left,
                    float: false
                }),
                Some(FloatAlignment {
                    align: Alignment::Left,
                    float: true
                }),
                Some(FloatAlignment {
                    align: Alignment::Left,
                    float: true
                }),
            ],
        );
    });
}
