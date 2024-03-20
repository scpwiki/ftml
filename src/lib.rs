/*
 * lib.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2024 Wikijump Team
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

//! A library to parse Wikidot text and produce an abstract syntax tree (AST).
//!
//! This library aims to be a replacement of Wikidot's Text_Wiki
//! parser, which is presently a loose group of regular expressions
//! (with irregular Perl extensions). The aim is to provide an AST
//! while also maintaining the flexibility and lax parsing that
//! Wikidot permits.
//!
//! The overall flow is the following:
//!
//! * Run messy includer
//! * Run preprocessor
//! * Run tokenizer
//! * Run parser
//! * Run renderer
//!
//! Each step of the flow makes extensive use of Rust's
//! borrowing capabilities, ensuring that as few allocations
//! are performed as possible. Any strings which are unmodified
//! are passed by reference. Despite this, all of the exported
//! structures are both serializable and deserializable via
//! [`serde`].
//!
//! Rendering is performed by the trait [`Render`].
//! There are two main implementations of note,
//! [`TextRender`] and [`HtmlRender`], which render to
//! plain text and full HTML respectively.
//!
//! # Features
//! This crate has one feature of note:
//!
//! The `mathml` feature pulls in the `latex2mathml` library,
//! which renders LaTeX blocks using MathML. It is enabled
//! by default.
//!
//! # Examples
// TODO do something with the links in these comments
//! ```
//!// Get an `Includer`.
//!//
//!// See trait documentation for what this requires, but
//!// essentially it is some abstract handle that gets the
//!// contents of a page to be included.
//!//
//!// Two sample includers you could try are `NullIncluder`
//!// and `DebugIncluder`.
//!let includer = MyIncluderImpl::new();
//!
//!// Get our source text
//!let mut input = "**some** test <<string?>>";
//!
//!// Substitute page inclusions
//!let (mut text, included_pages) = ftml::include(input, includer, &settings);
//!
//!// Perform preprocess substitutions
//!ftml::preprocess(&log, &mut text);
//!
//!// Generate token from input text
//!let tokens = ftml::tokenize(&text);
//!
//!// Parse the token list to produce an AST.
//!//
//!// Note that this produces a `ParseResult<SyntaxTree>`, which records the
//!// parsing warnings in addition to the final result.
//!let result = ftml::parse(&tokens, &page_info, &settings);
//!
//!// Here we extract the tree separately from the warning list.
//!//
//!// Now we have the final AST, as well as all the issues that
//!// occurred during the parsing process.
//!let (tree, warnings) = result.into();
//!// Finally, we render with our renderer. Generally this is `HtmlRender`,
//!// but you could have a custom implementation here too.
//!//
//!// You must provide a `PageInfo` struct, which describes the page being rendered.
//!// You must also provide a handle to provide various remote sources, such as
//!// module content, but this is not stabilized yet.
//!let html_output = HtmlRender.render(&tree, &page_info, &settings);
//! ````
//! # Targets
//! The library supports being compiled into WebAssembly.
//! (target `wasm32-unknown-unknown`, see [`wasm-pack`] for more information)
//!
//! Compiling to wasm also disables all FFI integration,
//! since these are inherently incompatible.
//!
//! # Bugs
//! If you discover any bugs or have any feature requests,
//! you can submit them via our Atlassian helpdesk [here](https://scuttle.atlassian.net/servicedesk/customer/portal/2).
//!
//! Alternatively, you can [get in touch with Wikijump developers directly](https://github.com/scpwiki/wikijump#readme).
//!
//! [`Render`]: ./render/trait.Render.html
//! [`TextRender`]: ./render/html/struct.HtmlRender.html
//! [`HtmlRender`]: ./render/text/struct.TextRender.html
//! [`serde`]: https://docs.rs/serde
//! [`wasm-pack`]: https://rustwasm.github.io/docs/wasm-pack/

// Only list crates which we want global macro imports.
// Rest are implicit based on Cargo.toml

#[macro_use]
extern crate cfg_if;

#[macro_use]
extern crate enum_map;

#[macro_use]
extern crate log;

#[macro_use]
extern crate maplit;

#[macro_use]
extern crate pest_derive;

#[macro_use]
extern crate serde;

#[macro_use]
extern crate serde_repr;

#[macro_use]
extern crate str_macro;

// Library top-level modules

#[cfg(test)]
mod test;

#[macro_use]
mod macros;

mod id_prefix;
mod next_index;
mod non_empty_vec;
mod preproc;
mod text;
mod url;
mod utf16;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub mod data;
pub mod includes;
pub mod info;
pub mod parsing;
pub mod render;
pub mod settings;
pub mod tokenizer;
pub mod tree;

pub use self::includes::include;
pub use self::parsing::parse;
pub use self::preproc::preprocess;
pub use self::tokenizer::{tokenize, Tokenization};
pub use self::utf16::Utf16IndexMap;

/// This module collects commonly used traits from this crate.
pub mod prelude {
    pub use super::data::{PageInfo, ScoreValue};
    pub use super::includes::{include, Includer};
    pub use super::parsing::{parse, ParseError, ParseResult};
    pub use super::preprocess;
    pub use super::render::Render;
    pub use super::settings::{
        InterwikiSettings, WikitextMode, WikitextSettings, DEFAULT_INTERWIKI,
        EMPTY_INTERWIKI,
    };
    pub use super::tokenizer::{tokenize, Tokenization};
    pub use super::tree::{Element, SyntaxTree};
}
