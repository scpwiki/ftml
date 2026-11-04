use super::elements_contain_delayed;
use crate::tree::{ContainerType, Element, ListItem, PartialElement, SyntaxTree};

#[cfg(feature = "html")]
use crate::data::PageInfo;
#[cfg(feature = "html")]
use crate::render::text::TextRender;
#[cfg(feature = "html")]
use crate::settings::WikitextSettings;
#[cfg(feature = "html")]
use crate::tree::{LinkLabel, LinkType};
#[cfg(feature = "html")]
use std::borrow::Cow;
#[cfg(feature = "html")]
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn entry_indices(tree: &SyntaxTree<'_>) -> Vec<usize> {
    let mut delayed = Vec::new();
    let mut entry_index = 0usize;
    visit_headings(&tree.elements, &tree.footnotes, &mut |elements| {
        if elements_contain_delayed(elements) {
            delayed.push(entry_index);
        }
        entry_index += 1;
    });
    delayed
}

fn visit_headings<'a, 't>(
    elements: &'a [Element<'t>],
    footnotes: &'a [Vec<Element<'t>>],
    visitor: &mut impl FnMut(&'a [Element<'t>]),
) {
    for element in elements {
        match element {
            Element::Container(container) => {
                if matches!(
                    container.ctype(),
                    ContainerType::Header(heading) if heading.has_toc
                ) {
                    visitor(container.elements());
                }
                visit_headings(container.elements(), footnotes, visitor);
            }
            Element::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        visit_headings(&cell.elements, footnotes, visitor);
                    }
                }
            }
            Element::TabView(tabs) => {
                for tab in tabs {
                    visit_headings(&tab.elements, footnotes, visitor);
                }
            }
            Element::List { items, .. } => {
                for item in items {
                    match item {
                        ListItem::Elements { elements, .. } => {
                            visit_headings(elements, footnotes, visitor);
                        }
                        ListItem::SubList { element } => {
                            visit_headings(
                                std::slice::from_ref(element.as_ref()),
                                footnotes,
                                visitor,
                            );
                        }
                    }
                }
            }
            Element::DefinitionList(items) => {
                for item in items {
                    visit_headings(&item.key_elements, footnotes, visitor);
                    visit_headings(&item.value_elements, footnotes, visitor);
                }
            }
            Element::Anchor { elements, .. }
            | Element::Collapsible { elements, .. }
            | Element::Color { elements, .. }
            | Element::Include { elements, .. } => {
                visit_headings(elements, footnotes, visitor);
            }
            Element::Partial(partial) => match partial {
                PartialElement::ListItem(ListItem::Elements { elements, .. }) => {
                    visit_headings(elements, footnotes, visitor);
                }
                PartialElement::ListItem(ListItem::SubList { element }) => {
                    visit_headings(
                        std::slice::from_ref(element.as_ref()),
                        footnotes,
                        visitor,
                    );
                }
                PartialElement::TableRow(row) => {
                    for cell in &row.cells {
                        visit_headings(&cell.elements, footnotes, visitor);
                    }
                }
                PartialElement::TableCell(cell) => {
                    visit_headings(&cell.elements, footnotes, visitor);
                }
                PartialElement::Tab(tab) => {
                    visit_headings(&tab.elements, footnotes, visitor);
                }
                PartialElement::RubyText(text) => {
                    visit_headings(&text.elements, footnotes, visitor);
                }
                PartialElement::WikidotEmptyInlineOwner
                | PartialElement::InlineSizeOpen(_)
                | PartialElement::InlineSizeClose(_)
                | PartialElement::InlineSpanOpen(_)
                | PartialElement::InlineSpanClose(_)
                | PartialElement::InlineFormatOpen(_)
                | PartialElement::InlineFormatClose(_)
                | PartialElement::InlineColorOpen(..)
                | PartialElement::InlineColorClose => {}
            },
            Element::Footnote(index) => {
                if let Some(contents) =
                    index.checked_sub(1).and_then(|index| footnotes.get(index))
                {
                    visit_headings(contents, footnotes, visitor);
                }
            }
            _ => {}
        }
    }
}

#[cfg(feature = "html")]
pub(super) fn bind_labels(
    tree: &mut SyntaxTree<'static>,
    delayed_entries: &[usize],
    page_info: &PageInfo,
    settings: &WikitextSettings,
) {
    if delayed_entries.is_empty() {
        return;
    }

    let targets = delayed_entries.iter().copied().collect::<BTreeSet<_>>();
    let mut labels = BTreeMap::new();
    let mut entry_index = 0usize;
    visit_headings(&tree.elements, &tree.footnotes, &mut |elements| {
        if targets.contains(&entry_index) {
            labels.insert(
                entry_index,
                TextRender.render_partial_title(elements, page_info, settings, 0),
            );
        }
        entry_index += 1;
    });
    debug_assert_eq!(labels.len(), delayed_entries.len());

    let mut current = 0usize;
    replace_labels(&mut tree.table_of_contents, &labels, &mut current);
}

#[cfg(feature = "html")]
fn replace_labels(
    elements: &mut [Element<'static>],
    labels: &BTreeMap<usize, String>,
    current: &mut usize,
) {
    for element in elements {
        let Element::List { items, .. } = element else {
            continue;
        };
        for item in items {
            match item {
                ListItem::Elements { elements, .. } => {
                    if let Some(label) = labels.get(current)
                        && let Some(LinkLabel::Text(text)) =
                            elements.iter_mut().find_map(|element| {
                                let Element::Link {
                                    ltype: LinkType::TableOfContents,
                                    label,
                                    ..
                                } = element
                                else {
                                    return None;
                                };
                                Some(label)
                            })
                    {
                        *text = Cow::Owned(label.clone());
                    }
                    *current += 1;
                }
                ListItem::SubList { element } => {
                    replace_labels(
                        std::slice::from_mut(element.as_mut()),
                        labels,
                        current,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delayed::{DelayedElement, GeneratedKind, SlotId};
    use crate::tree::{AttributeMap, Container, Heading, HeadingLevel};

    #[test]
    fn delayed_toc_uses_the_registered_footnote_index() {
        let delayed_heading = Element::Container(Container::new(
            ContainerType::Header(Heading {
                level: HeadingLevel::One,
                has_toc: true,
            }),
            vec![Element::Delayed(DelayedElement::active(
                SlotId::new(1),
                GeneratedKind::PageLink,
            ))],
            AttributeMap::new(),
        ));
        let tree = SyntaxTree {
            elements: vec![Element::Footnote(2)],
            footnotes: vec![vec![text!("first")], vec![delayed_heading]],
            ..SyntaxTree::default()
        };

        assert_eq!(entry_indices(&tree), vec![0]);
    }
}
