use crate::tree::{ContainerType, Element, ListItem, PartialElement};

pub(crate) fn normalize_wikidot_headings<'t>(elements: &mut Vec<Element<'t>>) {
    for element in elements.iter_mut() {
        visit_children_mut(element, normalize_wikidot_headings);
    }
    elements.retain(|element| {
        !matches!(
            element,
            Element::Container(container)
                if matches!(container.ctype(), ContainerType::Header(heading) if !heading.has_toc)
                    && heading_elements_are_empty(container.elements())
        )
    });
}

fn heading_elements_are_empty(elements: &[Element<'_>]) -> bool {
    elements.iter().all(|element| match element {
        Element::Text(text) | Element::Raw(text) => text.chars().all(char::is_whitespace),
        Element::Container(container) => heading_elements_are_empty(container.elements()),
        Element::LineBreak => true,
        Element::Partial(partial) if partial.is_inline_format_control() => true,
        _ => false,
    })
}

fn visit_children_mut<'t>(element: &mut Element<'t>, visit: fn(&mut Vec<Element<'t>>)) {
    match element {
        Element::Container(container) => visit(container.elements_mut()),
        Element::Table(table) => {
            for row in &mut table.rows {
                for cell in &mut row.cells {
                    visit(&mut cell.elements);
                }
            }
        }
        Element::TabView(tabs) => {
            for item in tabs {
                visit(item.elements_mut());
            }
        }
        Element::Anchor { elements, .. }
        | Element::Collapsible { elements, .. }
        | Element::Color { elements, .. }
        | Element::Include { elements, .. } => visit(elements),
        Element::WikidotBlockFrame { children, .. } => visit(children),
        Element::List { items, .. } => {
            for item in items {
                match item {
                    ListItem::Elements { elements, .. } => visit(elements),
                    ListItem::SubList { element } => visit_children_mut(element, visit),
                }
            }
        }
        Element::DefinitionList(items) => {
            for item in items {
                visit(&mut item.key_elements);
                visit(&mut item.value_elements);
            }
        }
        Element::Partial(partial) => match partial {
            PartialElement::ListItem(ListItem::Elements { elements, .. }) => {
                visit(elements)
            }
            PartialElement::ListItem(ListItem::SubList { element }) => {
                visit_children_mut(element, visit)
            }
            PartialElement::TableRow(row) => {
                for cell in &mut row.cells {
                    visit(&mut cell.elements);
                }
            }
            PartialElement::TableCell(cell) => visit(&mut cell.elements),
            PartialElement::Tab(tab) => visit(&mut tab.elements),
            PartialElement::RubyText(ruby_text) => visit(&mut ruby_text.elements),
            PartialElement::WikidotEmptyInlineOwner
            | PartialElement::WikidotBlockOpen(_)
            | PartialElement::WikidotBlockClose(_)
            | PartialElement::InlineSizeOpen(..)
            | PartialElement::InlineSizeClose(_)
            | PartialElement::InlineSpanOpen(..)
            | PartialElement::InlineSpanClose(_)
            | PartialElement::InlineFormatOpen(_)
            | PartialElement::InlineFormatClose(_)
            | PartialElement::InlineColorOpen(..)
            | PartialElement::InlineColorClose
            | PartialElement::InlineAnchorOpen(..)
            | PartialElement::InlineAnchorClose(_) => {}
        },
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{AttributeMap, Container, Heading, HeadingLevel};

    #[test]
    fn empty_wikidot_no_toc_headings_are_removed_after_inline_normalization() {
        let mut elements = vec![
            Element::Container(Container::new(
                ContainerType::Header(Heading {
                    level: HeadingLevel::One,
                    has_toc: false,
                }),
                vec![Element::Container(Container::new(
                    ContainerType::Span,
                    Vec::new(),
                    AttributeMap::new(),
                ))],
                AttributeMap::new(),
            )),
            Element::Container(Container::new(
                ContainerType::Header(Heading {
                    level: HeadingLevel::Two,
                    has_toc: true,
                }),
                Vec::new(),
                AttributeMap::new(),
            )),
        ];

        normalize_wikidot_headings(&mut elements);

        assert_eq!(elements.len(), 1);
        let Element::Container(container) = &elements[0] else {
            panic!("expected an ordinary empty heading: {elements:#?}");
        };
        assert!(matches!(
            container.ctype(),
            ContainerType::Header(Heading { has_toc: true, .. })
        ));
    }
}
