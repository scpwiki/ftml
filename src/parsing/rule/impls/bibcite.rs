/*
 * parsing/rule/impls/bibcite.rs
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
use std::borrow::Cow;

pub const RULE_BIBCITE: Rule = Rule {
    name: "bibcite",
    position: LineRequirement::Any,
    try_consume_fn,
};

fn bibliography_cite(label: Cow<'_, str>) -> Element<'_> {
    Element::BibliographyCite {
        label,
        brackets: false,
        inline: true,
    }
}

fn try_consume_fn<'r, 't>(
    parser: &mut Parser<'r, 't>,
) -> ParseResult<'r, 't, Elements<'t>> {
    debug!("Trying to create bibcite element");
    if parser.current().token != Token::LeftParentheses {
        warn!("Current token is not '((', failing rule");
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    parser.step()?;

    let current = parser.current();
    if current.token != Token::Identifier
        || !current.slice.eq_ignore_ascii_case("bibcite")
    {
        warn!("'((' not followed by 'bibcite', failing rule");
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    parser.step()?;

    if !matches!(parser.current().token, Token::Whitespace) {
        warn!("'((bibcite' not followed by a space, failing rule");
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }
    parser.step()?;

    let close = [ParseCondition::current(Token::RightParentheses)];
    let invalid = [
        ParseCondition::current(Token::Whitespace),
        ParseCondition::current(Token::ParagraphBreak),
        ParseCondition::current(Token::LineBreak),
    ];
    let (label, _) = collect_text_resolved(parser, RULE_BIBCITE, &close, &invalid, None)?;
    if parser.settings().layout.legacy() && !valid_wikidot_inline_label(&label) {
        return Err(parser.make_err(ParseErrorKind::RuleFailed));
    }

    ok!(bibliography_cite(label))
}

fn valid_wikidot_inline_label(label: &str) -> bool {
    !label.is_empty()
        && label
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn inline_bibcite_rejects_wrong_marker() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("(( label))");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("left parentheses should follow input start");
        parser.set_rule(RULE_BIBCITE);

        let error = RULE_BIBCITE
            .try_consume(&mut parser)
            .expect_err("non-bibcite marker should fail");
        assert_eq!(error.kind(), ParseErrorKind::RuleFailed);
    }

    #[test]
    fn inline_bibcite_rejects_wrong_opening_token() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("))");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("right parentheses should follow input start");
        parser.set_rule(RULE_BIBCITE);

        let error = RULE_BIBCITE
            .try_consume(&mut parser)
            .expect_err("wrong opening token should fail");
        assert_eq!(error.kind(), ParseErrorKind::RuleFailed);
    }

    #[test]
    fn inline_bibcite_rejects_wrong_identifier() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("((notbibcite label))");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("left parentheses should follow input start");
        parser.set_rule(RULE_BIBCITE);

        let error = RULE_BIBCITE
            .try_consume(&mut parser)
            .expect_err("wrong marker identifier should fail");
        assert_eq!(error.kind(), ParseErrorKind::RuleFailed);
    }

    #[test]
    fn inline_bibcite_requires_label_separator() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("((bibcite))");
        let mut parser = Parser::new(&tokenization, &page_info, &settings);
        parser
            .step()
            .expect("left parentheses should follow input start");
        parser.set_rule(RULE_BIBCITE);

        let error = RULE_BIBCITE
            .try_consume(&mut parser)
            .expect_err("missing separator should fail");
        assert_eq!(error.kind(), ParseErrorKind::RuleFailed);
    }

    #[test]
    fn inline_bibcite_parses_label_as_bare_citation() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("((bibcite alpha))");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty());
        match tree.elements.as_slice() {
            [Element::Container(container)] => match container.elements() {
                [
                    Element::BibliographyCite {
                        label,
                        brackets,
                        inline,
                    },
                ] => {
                    assert_eq!(label, "alpha");
                    assert!(!brackets);
                    assert!(inline);
                }
                other => panic!("expected bare bibliography cite, got {other:?}"),
            },
            other => panic!("expected citation paragraph, got {other:?}"),
        }
    }

    #[test]
    fn wikidot_inline_bibcite_preserves_an_empty_label_literally() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("((bibcite ))");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:#?}");
        match tree.elements.as_slice() {
            [Element::Container(container)] => {
                let literal = container
                    .elements()
                    .iter()
                    .map(|element| match element {
                        Element::Text(text) => text.as_ref(),
                        other => panic!("expected only literal text, got {other:?}"),
                    })
                    .collect::<String>();
                assert_eq!(literal, "((bibcite ))");
            }
            other => panic!("expected a literal citation paragraph, got {other:?}"),
        }
    }

    #[test]
    fn wikijump_inline_bibcite_keeps_its_existing_empty_label_behavior() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
        let tokenization = crate::tokenize("((bibcite ))");
        let (tree, errors) = crate::parse(&tokenization, &page_info, &settings).into();

        assert!(errors.is_empty(), "{errors:#?}");
        match tree.elements.as_slice() {
            [Element::Container(container)] => match container.elements() {
                [Element::BibliographyCite { label, inline, .. }] => {
                    assert!(label.is_empty());
                    assert!(*inline);
                }
                other => panic!("expected the existing Wikijump citation, got {other:?}"),
            },
            other => panic!("expected a Wikijump citation paragraph, got {other:?}"),
        }
    }
}
