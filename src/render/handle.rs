/*
 * render/handle.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2021 Wikijump Team
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
use crate::tree::LinkLabel;
use crate::tree::Module;
use std::num::NonZeroUsize;
use strum_macros::IntoStaticStr;

#[derive(Debug)]
pub struct Handle;

impl Handle {
    pub fn get_url(&self, log: &slog::Logger, site_slug: &str) -> String {
        debug!(
            log,
            "Getting URL of this Wikijump instance";
            "site" => site_slug,
        );

        // TODO
        format!("https://{}.wikijump.com/", site_slug)
    }

    pub fn render_module(
        &self,
        log: &slog::Logger,
        buffer: &mut String,
        module: &Module,
        mode: ModuleRenderMode,
    ) {
        debug!(
            log,
            "Rendering module";
            "module" => module.name(),
            "mode" => mode.name(),
        );

        match mode {
            ModuleRenderMode::Html => {
                str_write!(buffer, "<p>TODO: module {}</p>", module.name());
            }
            ModuleRenderMode::Text => {
                str_write!(buffer, "TODO: module {}", module.name());
            }
        }
    }

    pub fn get_page_title(&self, log: &slog::Logger, page_slug: &str) -> String {
        debug!(log, "Fetching page title"; "page" => page_slug);

        // TODO
        format!("TODO: actual title ({})", page_slug)
    }

    pub fn get_link_label<F>(
        &self,
        log: &slog::Logger,
        url: &str,
        label: &LinkLabel,
        f: F,
    ) where
        F: FnOnce(&str),
    {
        let page_title;
        let label_text = match *label {
            LinkLabel::Text(ref text) => text,
            LinkLabel::Url(Some(ref text)) => text,
            LinkLabel::Url(None) => url,
            LinkLabel::Page => {
                page_title = self.get_page_title(log, url);
                &page_title
            }
        };

        f(label_text);
    }

    pub fn get_message(
        &self,
        log: &slog::Logger,
        locale: &str,
        message: &str,
    ) -> &'static str {
        debug!(
            log,
            "Fetching message";
            "locale" => locale,
            "message" => message,
        );

        // TODO
        match message {
            "collapsible-open" => "+ open block",
            "collapsible-hide" => "- hide block",
            _ => {
                error!(
                    log,
                    "Unknown message requested";
                    "message" => message,
                );

                ""
            }
        }
    }

    pub fn post_html(&self, log: &slog::Logger, _info: &PageInfo, _html: &str) -> String {
        debug!(log, "Submitting HTML to create iframe-able snippet");

        // TODO
        str!("https://example.com/")
    }

    pub fn post_code(&self, log: &slog::Logger, index: NonZeroUsize, code: &str) {
        debug!(
            log,
            "Submitting code snippet";
            "index" => index.get(),
            "code" => code,
        );

        // TODO
    }
}

#[derive(
    IntoStaticStr, Serialize, Deserialize, Debug, Hash, Copy, Clone, PartialEq, Eq,
)]
#[serde(rename_all = "kebab-case")]
pub enum ModuleRenderMode {
    Html,
    Text,
}

impl ModuleRenderMode {
    #[inline]
    pub fn name(self) -> &'static str {
        self.into()
    }
}