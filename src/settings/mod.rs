/*
 * settings/mod.rs
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

mod interwiki;

use crate::layout::Layout;
use crate::next_index::Incrementer;
use std::str::FromStr;

pub use self::interwiki::{
    DEFAULT_INTERWIKI, EMPTY_INTERWIKI, InterwikiSettings, WIKIDOT_INTERWIKI,
};

const DEFAULT_MINIFY_CSS: bool = true;
const DEFAULT_ENABLE_HTML_BLOCKS: bool = true;

/// Settings to tweak behavior in the ftml parser and renderer.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct WikitextSettings {
    pub mode: WikitextMode,

    /// Whether top-level delayed ListPages values should remain outside a
    /// paragraph when the caller is composing an inline combined wrapper.
    ///
    /// This is disabled by default; Wikijump enables it only for the
    /// evidenced `separate="no"` wrapper path.
    #[serde(default)]
    pub list_pages_inline: bool,

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

    /// Whether `[[html]]` blocks are interpreted as hosted HTML blocks.
    ///
    /// Callers disable this in lifecycle contexts, such as an unsaved Wikidot page
    /// preview, where the same source is intentionally rendered as literal text.
    #[serde(default = "default_enable_html_blocks")]
    pub enable_html_blocks: bool,

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
    pub fn from_mode(mode: WikitextMode, layout: Layout) -> Self {
        let interwiki = match layout {
            Layout::Wikidot => WIKIDOT_INTERWIKI.clone(),
            Layout::Wikijump => DEFAULT_INTERWIKI.clone(),
        };

        match mode {
            WikitextMode::Page => WikitextSettings {
                mode,
                list_pages_inline: false,
                layout,
                enable_page_syntax: true,
                enable_html_blocks: DEFAULT_ENABLE_HTML_BLOCKS,
                use_true_ids: true,
                isolate_user_ids: layout == Layout::Wikidot,
                minify_css: DEFAULT_MINIFY_CSS,
                allow_local_paths: true,
                interwiki,
            },
            WikitextMode::PageNav => WikitextSettings {
                mode,
                list_pages_inline: false,
                layout,
                enable_page_syntax: true,
                enable_html_blocks: DEFAULT_ENABLE_HTML_BLOCKS,
                use_true_ids: false,
                isolate_user_ids: false,
                minify_css: DEFAULT_MINIFY_CSS,
                allow_local_paths: true,
                interwiki,
            },
            WikitextMode::Draft => WikitextSettings {
                mode,
                list_pages_inline: false,
                layout,
                enable_page_syntax: true,
                enable_html_blocks: DEFAULT_ENABLE_HTML_BLOCKS,
                use_true_ids: false,
                isolate_user_ids: layout == Layout::Wikidot,
                minify_css: DEFAULT_MINIFY_CSS,
                allow_local_paths: true,
                interwiki,
            },
            WikitextMode::ForumPost | WikitextMode::DirectMessage => WikitextSettings {
                mode,
                list_pages_inline: false,
                layout,
                enable_page_syntax: false,
                enable_html_blocks: DEFAULT_ENABLE_HTML_BLOCKS,
                use_true_ids: false,
                isolate_user_ids: false,
                minify_css: DEFAULT_MINIFY_CSS,
                allow_local_paths: false,
                interwiki,
            },
            WikitextMode::List => WikitextSettings {
                mode,
                list_pages_inline: false,
                layout,
                enable_page_syntax: true,
                enable_html_blocks: DEFAULT_ENABLE_HTML_BLOCKS,
                use_true_ids: false,
                isolate_user_ids: false,
                minify_css: DEFAULT_MINIFY_CSS,
                allow_local_paths: true,
                interwiki,
            },
        }
    }

    pub fn id_indexer(&self) -> Incrementer {
        if self.use_true_ids {
            Incrementer::default()
        } else {
            Incrementer::disabled()
        }
    }
}

const fn default_enable_html_blocks() -> bool {
    DEFAULT_ENABLE_HTML_BLOCKS
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

    /// Processing for the navigation panels for a page on a site.
    PageNav,

    /// Processing for a draft of a page.
    Draft,

    /// Processing for the contents of a forum post, of which there may be many.
    ForumPost,

    /// Processing for the contents of a direct message, sent to a user.
    DirectMessage,

    /// Processing for modules or other contexts such as `ListPages`.
    List,
}

impl WikitextMode {
    pub const fn external_name(self) -> &'static str {
        match self {
            WikitextMode::Page => "page",
            WikitextMode::PageNav => "page-nav",
            WikitextMode::Draft => "draft",
            WikitextMode::ForumPost => "forum-post",
            WikitextMode::DirectMessage => "direct-message",
            WikitextMode::List => "list",
        }
    }

    pub fn is_nav_content(self) -> bool {
        matches!(self, WikitextMode::PageNav)
    }
}

impl FromStr for WikitextMode {
    type Err = ();

    fn from_str(mode: &str) -> Result<Self, Self::Err> {
        match mode {
            "page" => Ok(WikitextMode::Page),
            "page-nav" => Ok(WikitextMode::PageNav),
            "draft" => Ok(WikitextMode::Draft),
            "forum-post" => Ok(WikitextMode::ForumPost),
            "direct-message" => Ok(WikitextMode::DirectMessage),
            "list" => Ok(WikitextMode::List),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn external_mode_names_round_trip_through_from_str() {
        let cases = [
            (WikitextMode::Page, "page"),
            (WikitextMode::PageNav, "page-nav"),
            (WikitextMode::Draft, "draft"),
            (WikitextMode::ForumPost, "forum-post"),
            (WikitextMode::DirectMessage, "direct-message"),
            (WikitextMode::List, "list"),
        ];

        for (mode, name) in cases {
            assert_eq!(mode.external_name(), name);
            assert_eq!(name.parse::<WikitextMode>(), Ok(mode));
        }

        assert!("unknown".parse::<WikitextMode>().is_err());
    }

    #[test]
    fn only_page_nav_is_nav_content() {
        let cases = [
            (WikitextMode::Page, false),
            (WikitextMode::PageNav, true),
            (WikitextMode::Draft, false),
            (WikitextMode::ForumPost, false),
            (WikitextMode::DirectMessage, false),
            (WikitextMode::List, false),
        ];

        for (mode, expected) in cases {
            assert_eq!(
                mode.is_nav_content(),
                expected,
                "unexpected nav content result for {mode:?}",
            );
        }
    }

    #[test]
    fn page_nav_settings_keep_page_features_without_true_ids() {
        let settings =
            WikitextSettings::from_mode(WikitextMode::PageNav, Layout::Wikijump);

        assert_eq!(settings.mode, WikitextMode::PageNav);
        assert_eq!(settings.layout, Layout::Wikijump);
        assert!(settings.enable_page_syntax);
        assert!(!settings.use_true_ids);
        assert!(!settings.isolate_user_ids);
        assert!(settings.minify_css);
        assert!(settings.allow_local_paths);
        assert_eq!(settings.interwiki, *DEFAULT_INTERWIKI);
    }

    #[test]
    fn wikidot_layout_settings_use_live_interwiki_table() {
        let wikidot = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let wikijump = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);

        assert_eq!(wikidot.interwiki, *WIKIDOT_INTERWIKI);
        assert_eq!(wikijump.interwiki, *DEFAULT_INTERWIKI);

        assert_eq!(
            wikidot
                .interwiki
                .build("wikipedia:Albert_Einstein")
                .as_deref(),
            Some("https://en.wikipedia.org/wiki/Albert_Einstein"),
        );
        assert_eq!(wikidot.interwiki.build("wp:SCP_Foundation"), None);
        assert_eq!(
            wikijump
                .interwiki
                .build("wikipedia:Albert_Einstein")
                .as_deref(),
            Some("https://wikipedia.org/wiki/Albert_Einstein"),
        );
    }

    #[test]
    fn wikidot_page_settings_isolate_user_ids() {
        let wikidot = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let wikijump = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);

        assert!(wikidot.use_true_ids);
        assert!(wikidot.isolate_user_ids);
        assert!(wikijump.use_true_ids);
        assert!(!wikijump.isolate_user_ids);
    }

    #[test]
    fn wikidot_draft_settings_isolate_user_ids() {
        let wikidot = WikitextSettings::from_mode(WikitextMode::Draft, Layout::Wikidot);
        let wikijump = WikitextSettings::from_mode(WikitextMode::Draft, Layout::Wikijump);

        assert!(!wikidot.use_true_ids);
        assert!(wikidot.isolate_user_ids);
        assert!(!wikijump.use_true_ids);
        assert!(!wikijump.isolate_user_ids);
    }
}
