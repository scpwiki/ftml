/*
 * tree/link.rs
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

use super::clone::string_to_owned;
use crate::data::PageRef;
use crate::settings::WikitextSettings;
use crate::url::is_url;
use std::borrow::Cow;
use strum_macros::EnumIter;

#[derive(Serialize, Deserialize, Debug, Hash, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum LinkLocation<'a> {
    /// This link points to a particular page on a wiki.
    Page(PageRef),

    /// This link is to a specific URL.
    Url(Cow<'a, str>),
}

impl<'a> LinkLocation<'a> {
    /// Like `parse()`, but also handles interwiki links.
    pub fn parse_with_interwiki(
        link: Cow<'a, str>,
        settings: &WikitextSettings,
    ) -> Option<(Self, LinkType)> {
        // Handle interwiki (starts with "!", like "!wp:Apple")
        match link.as_ref().strip_prefix('!') {
            // Not interwiki, parse as normal
            None => {
                let interwiki = Self::parse(link);
                let ltype = interwiki.link_type();
                Some((interwiki, ltype))
            }

            // Try to interpret as interwiki
            Some(link) => settings
                .interwiki
                .build(link)
                .map(|url| (LinkLocation::Url(Cow::Owned(url)), LinkType::Interwiki)),
        }
    }

    pub fn parse(link: Cow<'a, str>) -> Self {
        let mut link_str = link.as_ref();

        // Check for direct URLs or anchor links
        // TODO: parse local links into LinkLocation::Page
        // Known bug: single "/" parsed into Url instead of Page
        if is_url(link_str) || link_str.starts_with('#') || link_str.starts_with("/") {
            return LinkLocation::Url(link);
        }

        // Take only the first segment for page
        link_str = link_str
            .split('#') // get item before the first #
            .next()
            .expect("Splits always produce at least one item")
            .split('/') // get item before the first /
            .next()
            .expect("Splits always produce at least one item");

        match PageRef::parse(link_str) {
            Err(_) => LinkLocation::Url(Cow::Owned(link_str.to_owned())),
            Ok(page_ref) => LinkLocation::Page(page_ref),
        }
    }

    pub fn parse_extra(link: Cow<'a, str>) -> Option<Cow<'a, str>> {
        let link_str = link.as_ref();

        // Check for direct URLs or anchor links
        // Does not parse local links for now
        if is_url(link_str) || link_str.starts_with('#') || link_str.starts_with('/') {
            return None;
        }

        // Remove first path segment and reconstruct the remaining parts
        let mut split_anchor: Vec<&str> = link_str.splitn(2, "#").collect();
        let mut split_path: Vec<&str> = split_anchor[0].splitn(2, "/").collect();
        split_path[0] = "";
        let mut path = split_path.join("/");
        split_anchor[0] = &path;
        path = split_anchor.join("#");

        if path.is_empty() {
            None
        } else {
            Some(Cow::Owned(path))
        }
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
    macro_rules! check {
        ($input:expr => $site:expr, $page:expr) => {{
            let site_opt: Option<&str> = $site;
            let site = site_opt.map(|s| str!(s));
            let page = str!($page);
            let expected = LinkLocation::Page(PageRef { site, page });
            check!($input; expected);
        }};

        ($input:expr => $url:expr) => {
            let url = cow!($url);
            let expected = LinkLocation::Url(url);
            check!($input; expected);
        };

        ($input:expr; $expected:expr) => {{
            let actual = LinkLocation::parse(cow!($input));
            assert_eq!(
                actual,
                $expected,
                "Actual link location result doesn't match expected",
            );
        }};
    }

    check!("" => "");
    check!("#" => "#");
    check!("#anchor" => "#anchor");

    check!("page" => None, "page");
    check!("page/edit" => None, "page");
    check!("page#toc0" => None, "page");

    check!("/page" => "/page");
    check!("/page/edit" => "/page/edit");
    check!("/page#toc0" => "/page#toc0");

    check!("component:theme" => None, "component:theme");
    check!(":scp-wiki:scp-1000" => Some("scp-wiki"), "scp-1000");
    check!(":scp-wiki:component:theme" => Some("scp-wiki"), "component:theme");

    check!("http://blog.wikidot.com/" => "http://blog.wikidot.com/");
    check!("https://example.com" => "https://example.com");
    check!("mailto:test@example.net" => "mailto:test@example.net");

    check!("::page" => "::page");
    check!("::component:theme" => "::component:theme");
    check!("page:multiple:category" => None, "page:multiple:category");
}

#[test]
fn test_link_extra() {
    macro_rules! check {
        ($input:expr => $expected:expr) => {{
            let actual = LinkLocation::parse_extra(cow!($input));
            let expected = $expected.map(|s| cow!(s));

            assert_eq!(
                actual, expected,
                "Actual link extra segment doesn't match expected",
            );
        }};
    }

    check!("" => None);
    check!("page" => None);
    check!("page/edit" => Some("/edit"));
    check!("page#toc0" => Some("#toc0"));
    check!("page/edit#toc0" => Some("/edit#toc0"));

    check!("/" => None);
    check!("/page" => None);
    check!("/#/page" => None);
    check!("#" => None);
    check!("#anchor" => None);
}

#[derive(Serialize, Deserialize, Debug, Hash, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LinkLabel<'a> {
    /// Custom text link label.
    ///
    /// Can be set to any arbitrary value of the input text's choosing.
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

    /// URL-mirroring link label.
    ///
    /// This is where the label is just the same as the URL.
    Url,

    /// Article title-based link label.
    ///
    /// The label for this link is whatever the page's title is.
    Page,
}

impl LinkLabel<'_> {
    pub fn to_owned(&self) -> LinkLabel<'static> {
        match self {
            LinkLabel::Text(text) => LinkLabel::Text(string_to_owned(text)),
            LinkLabel::Slug(text) => LinkLabel::Slug(string_to_owned(text)),
            LinkLabel::Url => LinkLabel::Url,
            LinkLabel::Page => LinkLabel::Page,
        }
    }
}

#[derive(EnumIter, Serialize, Deserialize, Debug, Hash, Copy, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LinkType {
    /// This URL was specified directly.
    ///
    /// For instance, as a raw URL, or a single-bracket link.
    Direct,

    /// This URL was specified by specifying a particular Wikijump page.
    ///
    /// This variant comes from triple-bracket links.
    Page,

    /// This URL was generated via interwiki substitution.
    Interwiki,

    /// This URL points to an anchor elsewhere on this page.
    Anchor,

    /// This URL points to entries on a page in a table of contents.
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
        match value {
            "direct" => Ok(LinkType::Direct),
            "page" => Ok(LinkType::Page),
            "interwiki" => Ok(LinkType::Interwiki),
            "anchor" => Ok(LinkType::Anchor),
            "table-of-contents" => Ok(LinkType::TableOfContents),
            _ => Err(value),
        }
    }
}

/// Ensure `LinkType::name()` produces the same output as serde.
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
}
