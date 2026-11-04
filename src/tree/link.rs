/*
 * tree/link.rs
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

use super::clone::string_to_owned;
use crate::data::PageRef;
use crate::settings::WikitextSettings;
use crate::url::is_url;
use std::borrow::Cow;
use strum_macros::EnumIter;

#[derive(Serialize, Deserialize, Debug, Hash, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum LinkLocation<'a> {
    Page(PageRef),

    Url(Cow<'a, str>),
}

impl<'a> LinkLocation<'a> {
    pub fn parse_with_interwiki(
        link: Cow<'a, str>,
        settings: &WikitextSettings,
    ) -> Option<(Self, LinkType)> {
        match link.as_ref().strip_prefix('!') {
            None => {
                let interwiki = Self::parse(link);
                let ltype = interwiki.link_type();
                Some((interwiki, ltype))
            }

            Some(link) => {
                if let Some(url) = settings.interwiki.build(link) {
                    Some((LinkLocation::Url(Cow::Owned(url)), LinkType::Interwiki))
                } else if settings.layout.legacy() {
                    let location = Self::parse(Cow::Owned(link.to_owned()));
                    let ltype = location.link_type();
                    Some((location, ltype))
                } else {
                    None
                }
            }
        }
    }

    pub fn parse(link: Cow<'a, str>) -> Self {
        if Self::is_direct_target(&link) {
            return LinkLocation::Url(link);
        }

        match PageRef::parse(&link) {
            Err(_) => LinkLocation::Url(link),
            Ok(page_ref) => LinkLocation::Page(page_ref),
        }
    }

    fn is_direct_target(link: &str) -> bool {
        is_url(link) || link.starts_with('#') || Self::is_absolute_path(link)
    }

    fn is_absolute_path(link: &str) -> bool {
        link.starts_with('/')
    }

    pub fn to_owned(&self) -> LinkLocation<'static> {
        match self {
            LinkLocation::Page(page) => LinkLocation::Page(page.to_owned()),
            LinkLocation::Url(url) => LinkLocation::Url(string_to_owned(url)),
        }
    }

    pub fn link_type(&self) -> LinkType {
        match self {
            LinkLocation::Page(_) => LinkType::Page,
            LinkLocation::Url(_) => LinkType::Direct,
        }
    }
}

#[test]
fn test_link_location() {
    #[inline]
    fn convert_opt(s: Option<&str>) -> Option<String> {
        s.map(String::from)
    }

    macro_rules! test {
        ($input:expr => $site:expr, $page:expr, $extra:expr $(,)?) => {{
            let expected = LinkLocation::Page(PageRef {
                site: convert_opt($site),
                page: str!($page),
                extra: convert_opt($extra),
            });
            test!($input; expected);
        }};

        ($input:expr => $url:expr $(,)?) => {
            let url = cow!($url);
            let expected = LinkLocation::Url(url);
            test!($input; expected);
        };

        ($input:expr; $expected:expr $(,)?) => {{
            let actual = LinkLocation::parse(cow!($input));
            assert_eq!(
                actual,
                $expected,
                "Actual link location result doesn't match expected",
            );
        }};
    }

    test!("" => "");
    test!("/" => "/");
    test!("#" => "#");
    test!("#anchor" => "#anchor");

    test!("page" => None, "page", None);
    test!("page/edit" => None, "page", Some("/edit"));
    test!("page#toc0" => None, "page", Some("#toc0"));
    test!("page/comments#main" => None, "page", Some("/comments#main"));

    test!("/page" => "/page");
    test!("/page/edit" => "/page/edit");
    test!("/page#toc0" => "/page#toc0");

    test!("component:theme" => None, "component:theme", None);
    test!(":scp-wiki:scp-1000" => Some("scp-wiki"), "scp-1000", None);
    test!(
        ":scp-wiki:scp-1000#page-options-bottom" =>
            Some("scp-wiki"), "scp-1000", Some("#page-options-bottom"),
    );
    test!(
        ":scp-wiki:component:theme" =>
            Some("scp-wiki"), "component:theme", None,
    );
    test!(
        ":scp-wiki:component:theme/edit/true" =>
            Some("scp-wiki"), "component:theme", Some("/edit/true"),
    );
    // Dotted spellings stay local: Wikidot has no cross-site triple-bracket
    // link, so the link path must not canonicalize the host form.
    test!(
        ":scp-jp.wikidot.com:component:jstyles" =>
            Some("scp-jp-wikidot-com"), "component:jstyles", None,
    );

    test!("http://blog.wikidot.com/" => "http://blog.wikidot.com/");
    test!("https://example.com" => "https://example.com");
    test!("mailto:test@example.net" => "mailto:test@example.net");

    test!("::page" => "::page");
    test!("::component:theme" => "::component:theme");
    test!("multiple:category:page" => None, "multiple-category:page", None);
}

#[test]
fn link_location_parse_with_interwiki_expands_known_prefixes() {
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikijump);

    let (location, ltype) =
        LinkLocation::parse_with_interwiki(cow!("!wp:SCP Foundation"), &settings)
            .expect("known interwiki prefix should expand");
    assert_eq!(ltype, LinkType::Interwiki);
    assert_eq!(
        location,
        LinkLocation::Url(cow!("https://wikipedia.org/wiki/SCP%20Foundation")),
    );

    let (location, ltype) =
        LinkLocation::parse_with_interwiki(cow!("normal-page"), &settings)
            .expect("normal page links should still parse");
    assert_eq!(ltype, LinkType::Page);
    assert!(matches!(location, LinkLocation::Page(_)));

    assert!(
        LinkLocation::parse_with_interwiki(cow!("!missing:target"), &settings).is_none()
    );
}

#[derive(Serialize, Deserialize, Debug, Hash, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LinkLabel<'a> {
    Text(Cow<'a, str>),

    /// Page slug-based link label.
    ///
    /// This is set when the link is also the label.
    /// The link is pre-normalization but post-category stripping.
    ///
    /// For instance:
    /// * `[[[SCP-001]]]`
    /// * `[[[Ethics Committee Orientation]]]`
    /// * `[[[system: Recent Pages]]]`
    Slug(Cow<'a, str>),

    Url,

    Page,

    /// Wikidot's empty link label (`[[[target|]]]`).
    ///
    /// Wikidot shows the target page's title when the page exists
    /// (`[[[scp-series|]]]` renders "SCP Series 1") and this text, the
    /// normalized target, when it does not.
    Title(Cow<'a, str>),
}

impl LinkLabel<'_> {
    pub fn to_owned(&self) -> LinkLabel<'static> {
        match self {
            LinkLabel::Text(text) => LinkLabel::Text(string_to_owned(text)),
            LinkLabel::Slug(text) => LinkLabel::Slug(string_to_owned(text)),
            LinkLabel::Url => LinkLabel::Url,
            LinkLabel::Page => LinkLabel::Page,
            LinkLabel::Title(text) => LinkLabel::Title(string_to_owned(text)),
        }
    }
}

#[derive(EnumIter, Serialize, Deserialize, Debug, Hash, Copy, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LinkType {
    Direct,

    Page,

    Interwiki,

    Anchor,

    TableOfContents,
}

impl LinkType {
    pub fn name(self) -> &'static str {
        match self {
            LinkType::Direct => "direct",
            LinkType::Page => "page",
            LinkType::Interwiki => "interwiki",
            LinkType::Anchor => "anchor",
            LinkType::TableOfContents => "table-of-contents",
        }
    }
}

impl<'a> TryFrom<&'a str> for LinkType {
    type Error = &'a str;

    fn try_from(value: &'a str) -> Result<LinkType, &'a str> {
        if value == "direct" {
            Ok(LinkType::Direct)
        } else if value == "page" {
            Ok(LinkType::Page)
        } else if value == "interwiki" {
            Ok(LinkType::Interwiki)
        } else if value == "anchor" {
            Ok(LinkType::Anchor)
        } else if value == "table-of-contents" {
            Ok(LinkType::TableOfContents)
        } else {
            Err(value)
        }
    }
}

#[test]
fn link_type_name_serde() {
    use strum::IntoEnumIterator;

    for variant in LinkType::iter() {
        let output = serde_json::to_string(&variant).expect("Unable to serialize JSON");
        let serde_name: String =
            serde_json::from_str(&output).expect("Unable to deserialize JSON");

        assert_eq!(
            &serde_name,
            variant.name(),
            "Serde name does not match variant name",
        );

        let converted: LinkType = serde_name
            .as_str()
            .try_into()
            .expect("Could not convert item");

        assert_eq!(converted, variant, "Converted item does not match variant");
    }

    assert_eq!(
        LinkType::try_from("not-a-link-type"),
        Err("not-a-link-type")
    );
}
