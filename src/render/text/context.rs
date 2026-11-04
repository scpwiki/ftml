/*
 * render/text/context.rs
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
use crate::non_empty_vec::NonEmptyVec;
use crate::render::Handle;
use crate::settings::WikitextSettings;
use crate::tree::{Bibliography, BibliographyList, Element, VariableScopes};
use std::fmt::{self, Write};
use std::mem;
use std::num::NonZeroUsize;

fn advance_nonzero_index(index: &mut NonZeroUsize) -> NonZeroUsize {
    let next = NonZeroUsize::new(index.get() + 1).unwrap();
    mem::replace(index, next)
}

fn advance_list_index(index: &mut usize) -> usize {
    mem::replace(index, *index + 1)
}

#[derive(Debug)]
pub struct TextContext<'i, 'h, 'e, 't>
where
    'e: 't,
{
    output: String,
    info: &'i PageInfo<'i>,
    handle: &'h Handle,
    settings: &'e WikitextSettings,

    variables: VariableScopes,

    table_of_contents: &'e [Element<'t>],
    footnotes: &'e [Vec<Element<'t>>],
    bibliographies: &'e BibliographyList<'t>,

    prefixes: Vec<&'static str>,

    list_depths: NonEmptyVec<usize>,

    invisible: usize,

    equation_index: NonZeroUsize,

    footnote_index: NonZeroUsize,

    title_mode: bool,
}

impl<'i, 'h, 'e, 't> TextContext<'i, 'h, 'e, 't>
where
    'e: 't,
{
    #[inline]
    pub fn new(
        info: &'i PageInfo<'i>,
        handle: &'h Handle,
        settings: &'e WikitextSettings,
        table_of_contents: &'e [Element<'t>],
        footnotes: &'e [Vec<Element<'t>>],
        bibliographies: &'e BibliographyList<'t>,
        wikitext_len: usize,
    ) -> Self {
        TextContext {
            output: String::with_capacity(wikitext_len),
            info,
            handle,
            settings,
            variables: VariableScopes::new(),
            table_of_contents,
            footnotes,
            bibliographies,
            prefixes: Vec::new(),
            list_depths: NonEmptyVec::new(1),
            invisible: 0,
            equation_index: NonZeroUsize::new(1).unwrap(),
            footnote_index: NonZeroUsize::new(1).unwrap(),
            title_mode: false,
        }
    }

    #[inline]
    pub fn set_title_mode(&mut self, value: bool) {
        self.title_mode = value;
    }

    #[inline]
    pub fn title_mode(&self) -> bool {
        self.title_mode
    }

    pub fn buffer(&mut self) -> &mut String {
        std::convert::identity(&mut self.output)
    }

    #[inline]
    pub fn info(&self) -> &'i PageInfo<'i> {
        self.info
    }

    #[inline]
    pub fn settings(&self) -> &WikitextSettings {
        self.settings
    }

    #[inline]
    pub fn language(&self) -> &str {
        &self.info.language
    }

    #[inline]
    pub fn handle(&self) -> &'h Handle {
        self.handle
    }

    #[inline]
    pub fn variables(&self) -> &VariableScopes {
        &self.variables
    }

    #[inline]
    pub fn variables_mut(&mut self) -> &mut VariableScopes {
        &mut self.variables
    }

    #[inline]
    pub fn table_of_contents(&self) -> &'e [Element<'t>] {
        self.table_of_contents
    }

    #[inline]
    pub fn footnotes(&self) -> &'e [Vec<Element<'t>>] {
        self.footnotes
    }

    #[inline]
    pub fn get_bibliography(&self, index: usize) -> &'e Bibliography<'t> {
        self.bibliographies.get_bibliography(index)
    }

    pub fn get_bibliography_ref(
        &self,
        label: &str,
    ) -> Option<(usize, &'e [Element<'t>])> {
        self.bibliographies.get_reference(label)
    }

    pub fn next_equation_index(&mut self) -> NonZeroUsize {
        advance_nonzero_index(&mut self.equation_index)
    }

    pub fn next_footnote_index(&mut self) -> NonZeroUsize {
        advance_nonzero_index(&mut self.footnote_index)
    }

    #[inline]
    pub fn push_prefix(&mut self, prefix: &'static str) {
        self.prefixes.push(prefix);
    }

    #[inline]
    pub fn pop_prefix(&mut self) {
        self.prefixes.pop();
    }

    #[inline]
    pub fn list_depth(&self) -> usize {
        self.list_depths.len()
    }

    #[inline]
    pub fn incr_list_depth(&mut self) {
        self.list_depths.push(1);
    }

    #[inline]
    pub fn decr_list_depth(&mut self) {
        self.list_depths.pop();
    }

    pub fn next_list_index(&mut self) -> usize {
        advance_list_index(self.list_depths.last_mut())
    }

    #[inline]
    fn invisible(&self) -> bool {
        self.invisible > 0
    }

    #[inline]
    pub fn enable_invisible(&mut self) {
        self.invisible += 1;
    }

    #[inline]
    pub fn disable_invisible(&mut self) {
        self.invisible -= 1;
    }

    pub fn push(&mut self, ch: char) {
        if self.invisible() {
            self.output.push(' ');
        } else {
            self.output.push(ch);
        }
    }

    pub fn push_str(&mut self, s: &str) {
        if self.invisible() {
            let chars = s.chars().count();
            for _ in 0..chars {
                self.output.push(' ');
            }
        } else {
            self.output.push_str(s);
        }
    }

    pub fn add_newline(&mut self) {
        self.output.push('\n');

        for prefix in &self.prefixes {
            self.output.push_str(prefix);
        }
    }

    #[inline]
    pub fn ends_with_newline(&self) -> bool {
        self.output.ends_with('\n')
    }
}

impl<'i, 'h, 'e, 't> From<TextContext<'i, 'h, 'e, 't>> for String {
    #[inline]
    fn from(ctx: TextContext<'i, 'h, 'e, 't>) -> String {
        ctx.output
    }
}

impl<'e, 't> Write for TextContext<'_, '_, 'e, 't>
where
    'e: 't,
{
    #[inline]
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.buffer().write_str(s)
    }
}

#[test]
fn text_context_tracks_state_and_buffering() {
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};
    use crate::tree::{Bibliography, Element, VariableMap};

    let info = PageInfo::dummy();
    let handle = Handle;
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);
    let table_of_contents = vec![Element::Text(cow!("toc"))];
    let footnotes = vec![vec![Element::Text(cow!("footnote"))]];
    let reference = vec![Element::Text(cow!("reference"))];

    let mut bibliography = Bibliography::new();
    bibliography.add(cow!("alpha"), reference.clone());

    let mut bibliographies = BibliographyList::new();
    bibliographies.push(bibliography);

    let mut ctx = TextContext::new(
        &info,
        &handle,
        &settings,
        &table_of_contents,
        &footnotes,
        &bibliographies,
        64,
    );

    assert_eq!(ctx.info().page, "some-page");
    assert_eq!(ctx.settings(), &settings);
    assert_eq!(ctx.language(), "default");
    assert!(std::ptr::eq(ctx.handle(), &handle));
    assert_eq!(ctx.table_of_contents(), table_of_contents.as_slice());
    assert_eq!(ctx.footnotes(), footnotes.as_slice());
    assert_eq!(
        ctx.get_bibliography(0).get("alpha"),
        Some((1, reference.as_slice()))
    );
    assert_eq!(
        ctx.get_bibliography_ref("alpha"),
        Some((1, reference.as_slice()))
    );
    assert!(ctx.get_bibliography_ref("missing").is_none());

    let mut scope = VariableMap::new();
    scope.insert(cow!("name"), cow!("value"));
    ctx.variables_mut().push_scope(&scope);
    assert_eq!(ctx.variables().get("name"), Some("value"));
    ctx.variables_mut().pop_scope();
    assert_eq!(ctx.variables().get("name"), None);

    assert_eq!(ctx.next_equation_index().get(), 1);
    assert_eq!(ctx.next_equation_index().get(), 2);
    assert_eq!(ctx.next_footnote_index().get(), 1);
    assert_eq!(ctx.next_footnote_index().get(), 2);

    assert_eq!(ctx.list_depth(), 1);
    assert_eq!(ctx.next_list_index(), 1);
    assert_eq!(ctx.next_list_index(), 2);
    ctx.incr_list_depth();
    assert_eq!(ctx.list_depth(), 2);
    assert_eq!(ctx.next_list_index(), 1);
    ctx.decr_list_depth();
    assert_eq!(ctx.list_depth(), 1);
    assert_eq!(ctx.next_list_index(), 3);

    ctx.push_str("alpha");
    ctx.add_newline();
    assert!(ctx.ends_with_newline());
    ctx.push_prefix("> ");
    ctx.add_newline();
    ctx.pop_prefix();
    ctx.enable_invisible();
    ctx.push_str("βx");
    ctx.push('!');
    ctx.add_newline();
    ctx.disable_invisible();
    write!(&mut ctx, "done").unwrap();

    let output = String::from(ctx);
    assert_eq!(output, "alpha\n\n>    \ndone");
}
