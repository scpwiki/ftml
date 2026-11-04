/*
 * render/handle.rs
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

use crate::data::{KarmaLevel, PageInfo, UserInfo};
use crate::render::messages;
use crate::settings::WikitextSettings;
use crate::tree::{FileSource, ImageSize, LinkLabel, LinkLocation, Module};
use crate::url::BuildSiteUrl;
use std::borrow::Cow;
use std::num::NonZeroUsize;

#[derive(Debug)]
pub struct Handle;

/// Resolves whether an internal page reference names an existing page.
///
/// FTML performs this lookup synchronously while rendering. Callers backed by
/// an asynchronous data store should resolve all references in the syntax tree
/// in advance and provide a resolver over that snapshot.
pub trait PageExistenceResolver: Send + Sync {
    fn page_exists(&self, site: &str, page: &str) -> bool;

    /// The display title of an existing page, used for
    /// [`LinkLabel::Title`](crate::tree::LinkLabel::Title) labels.
    fn page_title(&self, _site: &str, _page: &str) -> Option<String> {
        None
    }
}

/// Resolves a parsed user name to its canonical account information.
///
/// Callers backed by an asynchronous data store should resolve all user names
/// in the syntax tree in advance and provide a resolver over that snapshot.
pub trait UserInfoResolver: Send + Sync {
    fn user_info(&self, name: &str) -> Option<UserInfo<'static>>;
}

impl PageExistenceResolver for Handle {
    fn page_exists(&self, site: &str, page: &str) -> bool {
        self.get_page_exists(site, page)
    }
}

impl UserInfoResolver for Handle {
    fn user_info(&self, name: &str) -> Option<UserInfo<'static>> {
        self.get_user_info(name)
    }
}

impl Handle {
    pub fn render_module(&self, buffer: &mut String, module: &Module) {
        debug!("Rendering module '{}'", module.name());
        str_write!(buffer, "<p>TODO: module {}</p>", module.name());
    }

    pub fn get_page_title(&self, _site: &str, _page: &str) -> Option<String> {
        debug!("Fetching page title");

        #[cfg(test)]
        if _page == "missing" {
            return None;
        }

        Some(format!("TODO: actual title ({_site} {_page})"))
    }

    pub fn get_page_exists(&self, _site: &str, _page: &str) -> bool {
        debug!("Checking page existence");

        if cfg!(test) && _page == "missing" {
            return false;
        }

        true
    }

    pub fn get_user_info(&self, name: &str) -> Option<UserInfo<'static>> {
        debug!("Fetching user info (name '{name}')");

        if cfg!(test) && name == "missing" {
            return None;
        }

        let mut info = UserInfo::fallback();
        let canonical = name.to_lowercase();
        info.user_name = Cow::Owned(canonical.clone());
        info.user_profile_url =
            Cow::Owned(format!("/user:info/{}", canonical.replace(' ', "-")));
        Some(info)
    }

    pub fn get_file_link<'a>(
        &self,
        source: &FileSource<'a>,
        info: &PageInfo,
        settings: &WikitextSettings,
    ) -> Option<Cow<'a, str>> {
        let (site, page, file): (&str, &str, &str) = match source {
            FileSource::Url(url)
                if url.starts_with("/local--files/") && !settings.allow_local_paths =>
            {
                warn!(
                    "Specified canonical local file source when local paths are disabled"
                );
                return None;
            }
            FileSource::Url(url) => return Some(Cow::clone(url)),
            FileSource::File1 { .. }
            | FileSource::File2 { .. }
            | FileSource::File3 { .. }
                if !settings.allow_local_paths =>
            {
                warn!("Specified path file source when local paths are disabled");
                return None;
            }
            FileSource::File1 { file }
                if settings.layout.legacy() && file.starts_with('/') =>
            {
                return Some(Cow::Owned(format!(
                    "https://{}.wjfiles.com/local--files/{}",
                    info.site,
                    file.trim_start_matches('/'),
                )));
            }
            FileSource::File1 { file } if settings.layout.legacy() => {
                let page = match info.category.as_deref() {
                    Some(category) if category != "_default" => {
                        format!("{category}:{}", info.page)
                    }
                    _ => info.page.to_string(),
                };
                return Some(Cow::Owned(format!(
                    "https://{}.wjfiles.com/local--files/{page}/{file}",
                    info.site,
                )));
            }
            FileSource::File1 { file } => (&info.site, &info.page, file),
            FileSource::File2 { page, file } => (&info.site, page, file),
            FileSource::File3 { site, page, file } => (site, page, file),
        };

        Some(Cow::Owned(format!(
            "https://{site}.wjfiles.com/local--files/{page}/{file}",
        )))
    }

    /// Resolves the display and original-file URLs for an attachment image.
    ///
    /// The parser retains the attachment filename. This handle materializes
    /// the URLs without asserting that the file exists or is authorized.
    pub fn get_implicit_attachment_image_links(
        &self,
        file: &str,
        page: Option<&str>,
        size: ImageSize,
        info: &PageInfo,
        settings: &WikitextSettings,
    ) -> Option<(String, String)> {
        if !settings.layout.legacy() {
            warn!("Specified a Wikidot attachment image in a non-legacy layout");
            return None;
        }
        if !settings.allow_local_paths {
            warn!("Specified an attachment image when local paths are disabled");
            return None;
        }

        let page = match page {
            Some(page) => page.to_owned(),
            None => match info.category.as_deref() {
                Some(category) if category != "_default" => {
                    format!("{category}:{}", info.page)
                }
                _ => info.page.to_string(),
            },
        };
        let file = file.replace('\\', "/");
        let source_owns_path = page.is_empty()
            && (file.starts_with("data:")
                || matches!(file.as_bytes().first(), Some(b'\'' | b'"'))
                    && file.contains('/'));
        let original = if source_owns_path {
            format!("https://{}.wjfiles.com/local--files/{file}", info.site)
        } else {
            format!(
                "https://{}.wjfiles.com/local--files/{page}/{file}",
                info.site
            )
        };
        let display = if source_owns_path {
            format!(
                "https://{}.wjfiles.com/local--resized-images/{file}/{}",
                info.site,
                size.file_name(),
            )
        } else {
            format!(
                "https://{}.wjfiles.com/local--resized-images/{page}/{file}/{}",
                info.site,
                size.file_name(),
            )
        };
        Some((display, original))
    }

    pub fn get_link_label<F>(
        &self,
        site: &str,
        link: &LinkLocation,
        label: &LinkLabel,
        f: F,
    ) where
        F: FnOnce(&str),
    {
        let page_title;
        let label_text = match label {
            LinkLabel::Text(text) | LinkLabel::Slug(text) | LinkLabel::Title(text) => {
                text
            }
            LinkLabel::Url => match link {
                LinkLocation::Url(url) => url.as_ref(),
                LinkLocation::Page(page_ref) => {
                    warn!("Link label requested raw URL text for page link");
                    page_title = page_ref.to_string();
                    &page_title
                }
            },
            LinkLabel::Page => match link {
                LinkLocation::Page(page_ref) => {
                    let (site, page, _) = page_ref.fields_or(site);
                    page_title = match self.get_page_title(site, page) {
                        Some(title) => title,
                        None => page_ref.to_string(),
                    };

                    &page_title
                }
                LinkLocation::Url(url) => {
                    warn!("Link label requested page title for URL link");
                    url.as_ref()
                }
            },
        };

        f(label_text);
    }

    pub fn get_karma_style(&self, karma: KarmaLevel) -> &'static str {
        match karma {
            KarmaLevel::Zero => {
                "background-image: url(https://www.wikidot.com/userkarma.php?u=8976177)"
            }
            KarmaLevel::One => {
                "background-image: url(https://www.wikidot.com/userkarma.php?u=172570)"
            }
            KarmaLevel::Two => {
                "background-image: url(https://www.wikidot.com/userkarma.php?u=172952)"
            }
            KarmaLevel::Three => {
                "background-image: url(https://www.wikidot.com/userkarma.php?u=172904)"
            }
            KarmaLevel::Four => {
                "background-image: url(https://www.wikidot.com/userkarma.php?u=6040770)"
            }
            KarmaLevel::Five => {
                "background-image: url(https://www.wikidot.com/userkarma.php?u=4598089)"
            }
        }
    }

    pub fn get_message(&self, language: &str, message: &str) -> &'static str {
        debug!("Fetching message (language {language}, key {message})");

        messages::get(language, message).unwrap_or_else(|| {
            error!("Unknown message requested (key {message})");
            "?"
        })
    }

    pub fn post_html(&self, info: &PageInfo, html: &str) -> String {
        debug!("Submitting HTML to create iframe-able snippet");

        let _ = info;
        let _ = html;

        str!("https://example.com/")
    }

    pub fn post_code(&self, index: NonZeroUsize, code: &str) {
        debug!("Submitting code snippet (index {})", index.get());

        let _ = index;
        let _ = code;
    }
}

impl BuildSiteUrl for Handle {
    fn build_url(&self, site: &str, path: &str, extra: Option<&str>) -> String {
        let extra = extra.unwrap_or("");
        format!("https://{site}.wikijump.com/{path}{extra}")
    }
}

#[test]
fn handle_fallbacks_cover_rendering_helpers() {
    use crate::layout::Layout;
    use crate::settings::WikitextMode;

    let handle = Handle;
    let info = PageInfo::dummy();
    let mut settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);

    let mut module_output = String::new();
    let module = Module::Rate;
    handle.render_module(&mut module_output, &module);
    assert_eq!(
        module_output,
        format!("<p>TODO: module {}</p>", module.name()),
    );

    assert_eq!(
        handle.get_page_title("sandbox", "page"),
        Some(str!("TODO: actual title (sandbox page)")),
    );
    assert!(handle.get_page_exists("sandbox", "page"));
    assert!(!handle.get_page_exists("sandbox", "missing"));

    let user = handle.get_user_info("ExampleUser").unwrap();
    assert_eq!(user.user_name, "exampleuser");
    assert_eq!(user.user_profile_url, "/user:info/exampleuser");
    assert!(handle.get_user_info("missing").is_none());

    assert_eq!(
        handle
            .get_file_link(
                &FileSource::Url(cow!("https://example.com/file.png")),
                &info,
                &settings,
            )
            .as_deref(),
        Some("https://example.com/file.png"),
    );
    assert_eq!(
        handle
            .get_file_link(
                &FileSource::File1 {
                    file: cow!("local.png"),
                },
                &info,
                &settings,
            )
            .as_deref(),
        Some("https://sandbox.wjfiles.com/local--files/some-page/local.png"),
    );
    assert_eq!(
        handle
            .get_file_link(
                &FileSource::File2 {
                    page: cow!("other-page"),
                    file: cow!("other.png"),
                },
                &info,
                &settings,
            )
            .as_deref(),
        Some("https://sandbox.wjfiles.com/local--files/other-page/other.png"),
    );
    assert_eq!(
        handle
            .get_file_link(
                &FileSource::File3 {
                    site: cow!("other-site"),
                    page: cow!("third-page"),
                    file: cow!("third.png"),
                },
                &info,
                &settings,
            )
            .as_deref(),
        Some("https://other-site.wjfiles.com/local--files/third-page/third.png"),
    );

    let mut legacy_info = PageInfo::dummy();
    legacy_info.category = Some(cow!("run-owned"));
    let legacy_settings =
        WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    assert_eq!(
        handle
            .get_file_link(
                &FileSource::File1 {
                    file: cow!("local.png"),
                },
                &legacy_info,
                &legacy_settings,
            )
            .as_deref(),
        Some("https://sandbox.wjfiles.com/local--files/run-owned:some-page/local.png"),
    );
    assert_eq!(
        handle.get_implicit_attachment_image_links(
            r#"folder\image.png"#,
            None,
            ImageSize::Medium,
            &legacy_info,
            &legacy_settings,
        ),
        Some((
            "https://sandbox.wjfiles.com/local--resized-images/run-owned:some-page/folder/image.png/medium.jpg".to_owned(),
            "https://sandbox.wjfiles.com/local--files/run-owned:some-page/folder/image.png".to_owned(),
        )),
    );
    assert_eq!(
        handle.get_implicit_attachment_image_links(
            "photo.jpeg",
            Some("other-page"),
            ImageSize::Medium,
            &legacy_info,
            &legacy_settings,
        ),
        Some((
            "https://sandbox.wjfiles.com/local--resized-images/other-page/photo.jpeg/medium.jpg".to_owned(),
            "https://sandbox.wjfiles.com/local--files/other-page/photo.jpeg".to_owned(),
        )),
    );

    settings.allow_local_paths = false;
    assert!(
        handle
            .get_file_link(
                &FileSource::File1 {
                    file: cow!("blocked.png"),
                },
                &info,
                &settings,
            )
            .is_none(),
    );
    assert!(
        handle
            .get_file_link(
                &FileSource::Url(cow!("/local--files/private-page/secret.png")),
                &info,
                &settings,
            )
            .is_none(),
    );
    assert_eq!(
        handle
            .get_file_link(
                &FileSource::Url(cow!("/ordinary-absolute/image.png")),
                &info,
                &settings,
            )
            .as_deref(),
        Some("/ordinary-absolute/image.png"),
    );

    let mut label = String::new();
    handle.get_link_label(
        "sandbox",
        &LinkLocation::Url(cow!("https://example.com")),
        &LinkLabel::Url,
        |text| label.push_str(text),
    );
    assert_eq!(label, "https://example.com");

    label.clear();
    handle.get_link_label(
        "sandbox",
        &LinkLocation::Url(cow!("https://example.com")),
        &LinkLabel::Text(cow!("custom")),
        |text| label.push_str(text),
    );
    assert_eq!(label, "custom");

    label.clear();
    handle.get_link_label(
        "sandbox",
        &LinkLocation::Url(cow!("https://example.com")),
        &LinkLabel::Slug(cow!("slug-label")),
        |text| label.push_str(text),
    );
    assert_eq!(label, "slug-label");

    label.clear();
    let page_ref = crate::data::PageRef::parse("target-page").unwrap();
    handle.get_link_label(
        "sandbox",
        &LinkLocation::Page(page_ref),
        &LinkLabel::Page,
        |text| label.push_str(text),
    );
    assert_eq!(label, "TODO: actual title (sandbox target-page)");

    label.clear();
    let page_ref = crate::data::PageRef::parse("missing").unwrap();
    handle.get_link_label(
        "sandbox",
        &LinkLocation::Page(page_ref),
        &LinkLabel::Page,
        |text| label.push_str(text),
    );
    assert_eq!(label, "missing");

    let karma_cases = [
        (KarmaLevel::Zero, "8976177"),
        (KarmaLevel::One, "172570"),
        (KarmaLevel::Two, "172952"),
        (KarmaLevel::Three, "172904"),
        (KarmaLevel::Four, "6040770"),
        (KarmaLevel::Five, "4598089"),
    ];
    for (karma, user_id) in karma_cases {
        assert!(handle.get_karma_style(karma).contains(user_id));
    }

    let message_cases = [
        ("button-copy-clipboard", "Copy to Clipboard"),
        ("collapsible-open", "+ open block"),
        ("collapsible-hide", "- hide block"),
        ("table-of-contents", "Table of Contents"),
        ("footnote", "Footnote"),
        ("footnote-cite-not-found", "Footnote item not found"),
        ("footnote-block-title", "Footnotes"),
        ("bibliography-reference", "Reference"),
        ("bibliography-block-title", "Bibliography"),
        ("bibliography-cite-not-found", "Bibliography item not found"),
        (
            "bibliography-block-not-found",
            "Bibliography block not found",
        ),
        ("image-context-bad", "No images in this context"),
        ("audio-context-bad", "No audio in this context"),
        ("video-context-bad", "No videos in this context"),
        ("user-missing-pre", ""),
        (
            "user-missing-post",
            " does not match any existing user name",
        ),
        ("unknown-key", "?"),
    ];
    for (key, expected) in message_cases {
        assert_eq!(handle.get_message("default", key), expected);
    }

    assert_eq!(
        handle.post_html(&info, "<p>text</p>"),
        "https://example.com/"
    );
    handle.post_code(NonZeroUsize::new(7).unwrap(), "code");
    assert_eq!(
        BuildSiteUrl::build_url(&handle, "sandbox", "page/edit", Some("/true")),
        "https://sandbox.wikijump.com/page/edit/true",
    );
    assert_eq!(
        BuildSiteUrl::build_url(&handle, "sandbox", "page", None),
        "https://sandbox.wikijump.com/page",
    );
}

#[test]
fn handle_falls_back_for_mismatched_link_labels() {
    let handle = Handle;
    let mut label = String::new();

    let page_ref = crate::data::PageRef::parse("target-page").unwrap();
    handle.get_link_label(
        "sandbox",
        &LinkLocation::Page(page_ref),
        &LinkLabel::Url,
        |text| label.push_str(text),
    );
    assert_eq!(label, "target-page");

    label.clear();
    handle.get_link_label(
        "sandbox",
        &LinkLocation::Url(cow!("https://example.com")),
        &LinkLabel::Page,
        |text| label.push_str(text),
    );
    assert_eq!(label, "https://example.com");
}

#[test]
fn handle_localizes_all_render_messages_to_japanese() {
    let handle = Handle;
    let message_cases = [
        ("button-copy-clipboard", "クリップボードにコピー"),
        ("collapsible-open", "+ ブロックを開く"),
        ("collapsible-hide", "- ブロックを隠す"),
        ("table-of-contents", "目次"),
        ("footnote", "脚注"),
        ("footnote-cite-not-found", "脚注項目が見つかりません"),
        ("footnote-block-title", "脚注"),
        ("bibliography-reference", "参照"),
        ("bibliography-block-title", "参考文献"),
        (
            "bibliography-cite-not-found",
            "参考文献項目が見つかりません",
        ),
        (
            "bibliography-block-not-found",
            "参考文献ブロックが見つかりません",
        ),
        ("image-context-bad", "このコンテキストには画像がありません"),
        ("audio-context-bad", "このコンテキストには音声がありません"),
        ("video-context-bad", "このコンテキストには動画がありません"),
        ("user-missing-pre", ""),
        ("user-missing-post", " に一致するユーザー名は存在しません"),
    ];

    for (key, expected) in message_cases {
        assert_eq!(handle.get_message("ja-JP", key), expected);
    }
    assert_eq!(handle.get_message("ja-JP", "unknown-key"), "?");
}
