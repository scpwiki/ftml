/*
 * settings/mod.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2025 Wikijump Team
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

mod interwiki;

use crate::layout::Layout;
use crate::next_index::Incrementer;

pub use self::interwiki::{DEFAULT_INTERWIKI, EMPTY_INTERWIKI, InterwikiSettings};

const DEFAULT_MINIFY_CSS: bool = true;

/// Settings to tweak behavior in the ftml parser and renderer.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct WikitextSettings {
    /// What mode we're running in.
    pub mode: WikitextMode,

    /// What layout we're targeting.
    ///
    /// For instance, generating Wikidot's legacy HTML structure.
    pub layout: Layout,

    /// Whether page-contextual syntax is permitted.
    ///
    /// This currently refers to:
    /// * Include
    /// * Module
    /// * Table of Contents
    /// * Button
    pub enable_page_syntax: bool,

    /// Whether a literal `[[include]]` is permitted.
    ///
    /// If this is true, then `[[include]]` is treated as an alias
    /// for `[[include]]`, which is necessary for Wikidot compatibility.
    ///
    /// It is off by default.
    pub use_include_compatibility: bool,

    /// Whether IDs should have true values, or be excluded or randomly generated.
    ///
    /// In the latter case, IDs can be used for navigation, for instance
    /// the table of contents, but setting this to `true` is needed in any
    /// context where more than one instance of rendered wikitext could be emitted.
    pub use_true_ids: bool,

    /// Whether to prefix user IDs with `u-`.
    ///
    /// This is a behavior found in Wikidot (although implemented incompletely)
    /// which prefixes IDs in HTML elements provided by the user with `u-` to ensure
    /// isolation.
    pub isolate_user_ids: bool,

    /// Whether to minify CSS in `<style>` blocks.
    pub minify_css: bool,

    /// Whether local paths are permitted.
    ///
    /// This should be disabled in contexts where there is no "local context"
    /// to which these paths could be interpreted. For instance, on pages
    /// you can reference an attached file, but on an arbitrary forum thread
    /// no such file can exist.
    ///
    /// This applies to:
    /// * Files
    /// * Images
    pub allow_local_paths: bool,

    /// What interwiki prefixes are supported.
    ///
    /// All instances of `$$` in the destination URL are replaced with the link provided
    /// in the interwiki link. For instance, `[wikipedia:SCP_Foundation SCP Wiki]`, then
    /// `$$` will be replaced with `SCP_Foundation`.
    ///
    /// # Notes
    ///
    /// * These are matched case-sensitively.
    /// * Prefixes may not contain colons, they are matched up to the first colon, and
    ///   any beyond that are considered part of the link.
    /// * By convention, prefixes should be all-lowercase.
    pub interwiki: InterwikiSettings,
}

impl WikitextSettings {
    /// Returns the default settings for the given [`WikitextMode`].
    pub fn from_mode(mode: WikitextMode, layout: Layout) -> Self {
        let interwiki = DEFAULT_INTERWIKI.clone();

        match mode {
            WikitextMode::Page => WikitextSettings {
                mode,
                layout,
                enable_page_syntax: true,
                use_include_compatibility: false,
                use_true_ids: true,
                isolate_user_ids: false,
                minify_css: DEFAULT_MINIFY_CSS,
                allow_local_paths: true,
                interwiki,
            },
            WikitextMode::Draft => WikitextSettings {
                mode,
                layout,
                enable_page_syntax: true,
                use_include_compatibility: false,
                use_true_ids: false,
                isolate_user_ids: false,
                minify_css: DEFAULT_MINIFY_CSS,
                allow_local_paths: true,
                interwiki,
            },
            WikitextMode::ForumPost | WikitextMode::DirectMessage => WikitextSettings {
                mode,
                layout,
                enable_page_syntax: false,
                use_include_compatibility: false,
                use_true_ids: false,
                isolate_user_ids: false,
                minify_css: DEFAULT_MINIFY_CSS,
                allow_local_paths: false,
                interwiki,
            },
            WikitextMode::List => WikitextSettings {
                mode,
                layout,
                enable_page_syntax: true,
                use_include_compatibility: false,
                use_true_ids: false,
                isolate_user_ids: false,
                minify_css: DEFAULT_MINIFY_CSS,
                allow_local_paths: true,
                interwiki,
            },
        }
    }

    /// Construct a new `Indexer` based on the setting of `use_true_ids`.
    pub fn id_indexer(&self) -> Incrementer {
        if self.use_true_ids {
            Incrementer::default()
        } else {
            Incrementer::disabled()
        }
    }
}

/// What mode parsing and rendering is done in.
///
/// Each variant has slightly different behavior associated
/// with them, beyond the typical flags for the rest of `WikitextSettings`.
///
/// The exact details of each are still being decided as this is implemented.
#[derive(Serialize, Deserialize, Debug, Copy, Clone, Hash, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WikitextMode {
    /// Processing for the contents of a page on a site.
    Page,

    /// Processing for a draft of a page.
    Draft,

    /// Processing for the contents of a forum post, of which there may be many.
    ForumPost,

    /// Processing for the contents of a direct message, sent to a user.
    DirectMessage,

    /// Processing for modules or other contexts such as `ListPages`.
    List,
}
