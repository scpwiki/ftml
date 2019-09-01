/*
 * lib.rs
 *
 * ftml - Convert Wikidot code to HTML
 * Copyright (C) 2019 Ammon Smith
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

#![deny(missing_debug_implementations)]

//! A library to convert Wikidot text source into HTML.
//!
//! Essentially a rewrite of Wikidot's Text_Wiki module, with
//! the intention for better modular integration and standalone
//! servicing.
//!
//! The main goal of this project is backwards-compatibility: if
//! there is an article on the SCP Wiki which uses a piece of syntax,
//! we intend to support it (or convince the author to change it).
//! Thus, every parsing or rendering rule should have tests, and
//! a dedicated battery of test articles and their HTML outputs
//! are test for any new version.
//!
//! That said, deprecated tags or weird Wikidot behavior will not be
//! supported if there are no mainlist articles or pages using them.
//! Additionally, if Wikidot doesn't support something (such as nested
//! collapsibles), we will aim to allow them through the use of parser
//! rules. Additionally, features not found within Wikidot's Text_Wiki
//! will be added.
//!
//! This crate also provides an executable to convert files from
//! the command-line. See that file for usage documentation.

extern crate chrono;
extern crate color_backtrace;
extern crate either;
extern crate htmlescape;

#[macro_use]
extern crate lazy_static;

#[macro_use]
extern crate log;
extern crate percent_encoding;
extern crate pest;

#[macro_use]
extern crate pest_derive;
extern crate regex;

#[macro_use]
extern crate str_macro;

#[macro_use]
mod macros;

pub mod data;
mod enums;
mod error;
mod filter;
mod handle;
mod parse;
mod render;

#[cfg(test)]
mod test;

pub use self::error::Error;
pub use self::filter::{postfilter, prefilter, Includer};
pub use self::handle::RemoteHandle;
pub use self::parse::{parse, SyntaxTree};
pub use self::render::{HtmlRender, PageInfo, Render, TreeRender};

mod backtrace {
    use color_backtrace;
    use std::sync::Once;

    static BACKTRACE: Once = Once::new();

    pub fn init() {
        BACKTRACE.call_once(|| color_backtrace::install());
    }
}

pub mod prelude {
    pub use super::{data, parse, prefilter};
    pub use super::{
        Error, HtmlRender, PageInfo, RemoteHandle, Render, Result, StdResult, SyntaxTree, TreeRender,
    };
}

pub mod include {
    pub use super::filter::Includer;
    pub use super::filter::{NotFoundIncluder, NullIncluder};
}

pub type StdResult<T, E> = std::result::Result<T, E>;
pub type Result<T> = StdResult<T, Error>;
pub type RemoteResult<T> = StdResult<T, String>;
