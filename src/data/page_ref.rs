/*
 * data/page_ref.rs
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

use std::borrow::Cow;
use std::fmt::{self, Display};
use wikidot_normalize::normalize;

/// Canonicalizes an `[[include]]` site segment: a host-form
/// `<slug>.wikidot.com` spelling (ASCII case-insensitive, with an optional
/// single trailing `.`) becomes `<slug>`, before slug normalization runs.
///
/// Wikidot looks an include site segment up as a site *or a host*, so the
/// full domain spelling names the same site as the bare slug. The strip must
/// happen here, on the raw segment: after `wikidot_normalize::normalize`
/// runs, `scp-jp.wikidot.com` has already become `scp-jp-wikidot-com`, a
/// legal slug that Wikidot treats as a different, nonexistent site.
///
/// The prefix is accepted only when it is non-empty and matches
/// `[A-Za-z0-9-]+` (no further dots). Anything else — including the dashed
/// spelling, `*.wdfiles.com`, custom domains, and bare `wikidot.com` — is
/// returned unchanged. Case is preserved; `normalize` case-folds afterwards.
///
/// This is include-specific: triple-bracket links keep the dotted spelling in
/// their local slug, so link parsing must keep using [`PageRef::parse`].
pub fn canonical_include_site(raw: &str) -> Cow<'_, str> {
    let trimmed = raw.trim();
    let trimmed = trimmed.strip_suffix('.').unwrap_or(trimmed);
    const SUFFIX: &str = ".wikidot.com";
    if trimmed.len() < SUFFIX.len() {
        return Cow::Borrowed(raw);
    }
    let (prefix, suffix) = trimmed.split_at(trimmed.len() - SUFFIX.len());
    if !suffix.eq_ignore_ascii_case(SUFFIX) {
        return Cow::Borrowed(raw);
    }
    if prefix.is_empty()
        || !prefix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Cow::Borrowed(raw);
    }
    Cow::Borrowed(prefix)
}

/// Represents a reference to a page on the wiki, as used by include notation.
///
/// It tracks whether it refers to a page on this wiki, or some other,
/// and what the names of these are.
///
/// The Wikidot syntax here allows for two cases:
/// * `:wiki-name:page` (off-site)
/// * `page` (on-site)
///
/// Additionally "`page`" here may also contain colons, such as `component:some-thing`.
#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct PageRef {
    pub site: Option<String>,
    pub page: String,
    pub extra: Option<String>,
}

impl PageRef {
    fn split_page(page: &str) -> (&str, Option<&str>) {
        match page.find(['#', '/']) {
            None => (page, None),
            Some(index) => {
                let (page, extra) = page.split_at(index);
                (page, Some(extra))
            }
        }
    }

    pub fn new<S1, S2>(site: Option<S1>, page: S2) -> Self
    where
        S1: Into<String>,
        S2: AsRef<str>,
    {
        match site {
            Some(site) => Self::page_and_site(site, page),
            None => Self::page_only(page),
        }
    }

    #[inline]
    pub fn page_and_site<S1, S2>(site: S1, page: S2) -> Self
    where
        S1: Into<String>,
        S2: AsRef<str>,
    {
        let (page, extra) = Self::split_page(page.as_ref());
        let mut site = site.into();
        let mut page = str!(page);
        let extra = extra.map(String::from);
        normalize(&mut site);
        normalize(&mut page);

        PageRef {
            site: Some(site),
            page,
            extra,
        }
    }

    #[inline]
    pub fn page_only<S>(page: S) -> Self
    where
        S: AsRef<str>,
    {
        let (page, extra) = Self::split_page(page.as_ref());
        let mut page = str!(page);
        let extra = extra.map(String::from);
        normalize(&mut page);
        PageRef {
            site: None,
            page,
            extra,
        }
    }

    #[inline]
    pub fn site(&self) -> Option<&str> {
        self.site.as_deref()
    }

    #[inline]
    pub fn page(&self) -> &str {
        &self.page
    }

    #[inline]
    pub fn extra(&self) -> Option<&str> {
        self.extra.as_deref()
    }

    #[inline]
    pub fn fields(&self) -> (Option<&str>, &str, Option<&str>) {
        (self.site(), self.page(), self.extra())
    }

    /// Like `fields()`, but uses the current site value to avoid returning `Option`.
    pub fn fields_or<'a>(&'a self, current_site: &'a str) -> (&'a str, &'a str, &'a str) {
        (
            self.site().unwrap_or(current_site),
            self.page(),
            self.extra().unwrap_or(""),
        )
    }

    pub fn parse(s: &str) -> Result<PageRef, PageRefParseError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(PageRefParseError);
        }

        let result = match s.find(':') {
            Some(0) => {
                let idx = match s[1..].find(':') {
                    Some(0) | None => return Err(PageRefParseError),

                    Some(idx) => idx + 1,
                };

                let site = s[1..idx].trim();
                let page = s[idx + 1..].trim();
                PageRef::page_and_site(site, page)
            }

            Some(_) => PageRef::page_only(s),

            None => PageRef::page_only(s),
        };

        if result.page().is_empty() {
            return Err(PageRefParseError);
        }

        Ok(result)
    }

    /// Parses an `[[include]]` target, canonicalizing a host-form
    /// `<slug>.wikidot.com` site segment to `<slug>` before normalization.
    ///
    /// Identical to [`PageRef::parse`] except for the site slice, which goes
    /// through [`canonical_include_site`]. Both `[[include]]` parsers must
    /// use this; [`PageRef::parse`] stays untouched for triple-bracket links,
    /// where Wikidot keeps the dotted spelling in the local slug.
    pub fn parse_include(s: &str) -> Result<PageRef, PageRefParseError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(PageRefParseError);
        }

        let result = match s.find(':') {
            Some(0) => {
                let idx = match s[1..].find(':') {
                    Some(0) | None => return Err(PageRefParseError),

                    Some(idx) => idx + 1,
                };

                let site = s[1..idx].trim();
                let page = s[idx + 1..].trim();
                let site = canonical_include_site(site);
                PageRef::page_and_site(site.into_owned(), page)
            }

            Some(_) => PageRef::page_only(s),

            None => PageRef::page_only(s),
        };

        if result.page().is_empty() {
            return Err(PageRefParseError);
        }

        Ok(result)
    }
}

impl Display for PageRef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(site) = self.site() {
            write!(f, ":{}:", site)?;
        }

        write!(f, "{}", self.page)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct PageRefParseError;

#[test]
fn split_page() {
    macro_rules! test {
        ($input:expr => $page:expr, $extra:expr $(,)?) => {
            assert_eq!(
                PageRef::split_page($input),
                ($page, $extra),
                "Split page portion does not have correct page and extra parts",
            )
        };

        ($input:expr, $page:expr, $extra:expr $(,)?) => {
            test!($input => $page, Some($extra))
        };

        ($input:expr) => {
            test!($input => $input, None)
        };
    }

    test!("scp-001");
    test!("scp-001/edit", "scp-001", "/edit");
    test!("scp-001/edit/true", "scp-001", "/edit/true");
    test!("Ethics Committee Orientation");
    test!(
        "Ethics Committee Orientation/edit",
        "Ethics Committee Orientation",
        "/edit",
    );
    test!(
        "Ethics Committee Orientation/edit/true",
        "Ethics Committee Orientation",
        "/edit/true",
    );
    test!("main#toc4", "main", "#toc4");
    test!("SCP-500/edit#page-options", "SCP-500", "/edit#page-options");
}

#[test]
fn page_ref_new_normalizes_site_and_extra_parts() {
    let off_site = PageRef::new(
        Some(String::from("SCP Wiki")),
        String::from("Main Page/edit"),
    );
    let on_site = PageRef::new(None::<String>, String::from("Sandbox Page#toc2"));

    assert_eq!(
        off_site.fields(),
        (Some("scp-wiki"), "main-page", Some("/edit"))
    );
    assert_eq!(on_site.fields(), (None, "sandbox-page", Some("#toc2")));
}

#[test]
fn page_ref() {
    macro_rules! test {
        ($input:expr $(,)?) => {
            test!($input => None)
        };

        ($input:expr, $expected:expr $(,)?) => {
            test!($input => Some($expected))
        };

        ($input:expr => $expected:expr) => {{
            let actual = PageRef::parse($input);
            let expected = $expected.ok_or(PageRefParseError);

            println!("Input: {:?}", $input);
            println!("Output: {:?}", actual);
            println!();

            assert_eq!(actual, expected, "Actual parse results don't match expected");
        }};
    }

    test!("");
    test!(":page");
    test!("::page");
    test!(":scp-wiki:");
    test!("page", PageRef::page_only("page"));
    test!("component:page", PageRef::page_only("component:page"));
    test!(
        "deleted:secret:fragment:page",
        PageRef::page_only("deleted:secret:fragment:page"),
    );
    test!(":scp-wiki:page", PageRef::page_and_site("scp-wiki", "page"));
    test!(
        ":scp-wiki:component:page",
        PageRef::page_and_site("scp-wiki", "component:page"),
    );
    test!(
        ":scp-wiki:deleted:secret:fragment:page",
        PageRef::page_and_site("scp-wiki", "deleted:secret:fragment:page"),
    );
}

#[test]
fn canonical_include_site_strips_host_form_before_normalization() {
    use std::borrow::Cow;

    for (input, expected) in [
        ("scp-jp.wikidot.com", "scp-jp"),
        ("SCP-JP.WIKIDOT.COM", "SCP-JP"),
        ("scp-jp.wikidot.com.", "scp-jp"),
        ("  scp-jp.wikidot.com  ", "scp-jp"),
        ("scp-wiki.wikidot.com", "scp-wiki"),
        ("scp-sandbox-3.wikidot.com", "scp-sandbox-3"),
        ("05command.wikidot.com", "05command"),
        ("www.wikidot.com", "www"),
    ] {
        assert_eq!(
            canonical_include_site(input),
            Cow::Borrowed(expected),
            "host-form site must canonicalize: {input:?}",
        );
    }

    for input in [
        "wikidot.com",
        ".wikidot.com",
        "a.b.wikidot.com",
        "scp-jp-wikidot-com",
        "scp-jp.wdfiles.com",
        "kontainer.djkakt.us",
        "scp.avn.sh",
        "scp-wiki",
        "",
    ] {
        assert_eq!(
            canonical_include_site(input),
            Cow::Borrowed(input),
            "non-host site must pass through: {input:?}",
        );
    }
}

#[test]
fn parse_include_canonicalizes_dotted_site_segments() {
    assert_eq!(
        PageRef::parse_include(":scp-jp.wikidot.com:component:jstyles"),
        PageRef::parse(":scp-jp:component:jstyles"),
    );
    assert_eq!(
        PageRef::parse_include(":SCP-JP.WIKIDOT.COM:component:jstyles"),
        PageRef::parse(":scp-jp:component:jstyles"),
    );
    assert_eq!(
        PageRef::parse_include(":scp-jp-sandbox3:component:jstyles"),
        PageRef::parse(":scp-jp-sandbox3:component:jstyles"),
    );
    assert!(PageRef::parse_include("").is_err());
    assert!(PageRef::parse_include(":scp-jp.wikidot.com:").is_err());
}

#[test]
fn parse_keeps_dotted_site_for_links() {
    // Triple-bracket links have no cross-site form: Wikidot keeps the dotted
    // spelling in the local slug, so the general parser must not strip it.
    let actual = PageRef::parse(":scp-jp.wikidot.com:component:jstyles")
        .expect("dotted target parses");
    assert_eq!(actual.site(), Some("scp-jp-wikidot-com"));
    assert_eq!(
        actual,
        PageRef::page_and_site("scp-jp-wikidot-com", "component:jstyles"),
    );
}

#[cfg(test)]
mod prop {
    use super::*;
    use proptest::prelude::*;

    const PROPTEST_CASES: u32 = 512;

    fn page_ref_input() -> impl Strategy<Value = String> {
        prop_oneof![
            proptest::string::string_regex(r".{0,96}").unwrap(),
            proptest::string::string_regex(r".{1,64}:.{1,64}").unwrap(),
            proptest::string::string_regex(r".{1,48}:.{1,48}[#/].{1,48}").unwrap(),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(PROPTEST_CASES))]

        #[test]
        fn page_ref_parse_prop(s in page_ref_input()) {
            let _ = PageRef::parse(&s);
        }
    }
}
