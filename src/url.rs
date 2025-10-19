/*
 * url.rs
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

use regex::Regex;
use std::borrow::Cow;
use std::sync::LazyLock;

#[cfg(feature = "html")]
use crate::tree::LinkLocation;

pub const URL_SCHEMES: [&str; 19] = [
    "blob:",
    "chrome-extension://",
    "chrome://",
    "content://",
    "dns:",
    "feed:",
    "file://",
    "ftp://",
    "git://",
    "gopher://",
    "http://",
    "https://",
    "irc6://",
    "irc://",
    "ircs://",
    "mailto:",
    "resource://",
    "rtmp://",
    "sftp://",
];

pub fn is_url(url: &str) -> bool {
    // If it's a URL
    for scheme in &URL_SCHEMES {
        if url.starts_with(scheme) {
            return true;
        }
    }

    false
}

/// Returns true if the scheme for this URL is `javascript:` or `data:`.
/// This function works case-insensitively (for ASCII).
///
/// Additionally, there is a check to make sure that there isn't any
/// funny business going on with the scheme, such as insertion of
/// whitespace. In such cases, the URL is rejected.
///
/// This function does not check anything starting with `/`, since
/// this would be a relative link.
pub fn dangerous_scheme(url: &str) -> bool {
    static SCHEME_REGEX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[\w\-]+$").unwrap());

    // Ignore relative links
    if url.starts_with('/') {
        return false;
    }

    // Get the scheme from the URL
    url.split_once(':')
        .map(|(scheme, _)| {
            if !SCHEME_REGEX.is_match(scheme) {
                // Weird scheme like "java\nscript", reject.
                return true;
            }

            // Now that we've confirmed it's normal,
            // check for these specific dangerous schemes.
            scheme.eq_ignore_ascii_case("javascript")
                || scheme.eq_ignore_ascii_case("data")
        })
        .unwrap_or(false)
}

#[cfg(feature = "html")]
pub fn normalize_link<'a>(
    link: &'a LinkLocation<'a>,
    helper: &dyn BuildSiteUrl,
) -> Cow<'a, str> {
    match link {
        LinkLocation::Url(url) => normalize_href(url, None),
        LinkLocation::Page(page_ref) => {
            let (site, page, extra) = page_ref.fields();
            match site {
                Some(site) => Cow::Owned(helper.build_url(site, page, extra)),
                None => normalize_href(page, extra),
            }
        }
    }
}

/// Normalize a URL string.
///
/// This performs a few operations:
/// * Blocking dangerous URLs (e.g. `javascript:alert(1)`)
/// * For relative links, normalizing the page portion (e.g. `/SCP-001/edit`)
/// * Adds a leading `/` if it is missing.
///
/// The `extra` argument corresponds to `PageRef.extra`.
/// It shouldn't be `Some(_)` for other kinds of links.
pub fn normalize_href<'a>(url: &'a str, extra: Option<&'a str>) -> Cow<'a, str> {
    if url == "javascript:;" {
        trace!("Leaving no-op link as-is");
        Cow::Borrowed(url)
    } else if is_url(url) || url.starts_with('/') || url.starts_with('#') {
        match extra {
            Some(extra) => {
                trace!("Leaving safe URL with extra as-is: {url}{extra}");
                Cow::Owned(format!("{url}{extra}"))
            }
            None => {
                trace!("Leaving safe URL as-is: {url}");
                Cow::Borrowed(url)
            }
        }
    } else if dangerous_scheme(url) {
        warn!("Attempt to pass in dangerous URL: {url}");
        Cow::Borrowed("#invalid-url")
    } else {
        // In this branch, the URL is not absolute (e.g. https://example.com)
        // and so must be a relative link with no leading / (e.g. just "some-page").
        let extra = extra.unwrap_or("");
        trace!("Adding leading slash to URL: {url}{extra}");
        Cow::Owned(format!("/{url}{extra}"))
    }
}

pub trait BuildSiteUrl {
    fn build_url(&self, site: &str, path: &str, extra: Option<&str>) -> String;
}

#[test]
fn detect_dangerous_schemes() {
    macro_rules! check {
        ($input:expr, $result:expr $(,)?) => {
            assert_eq!(
                dangerous_scheme($input),
                $result,
                "For input {:?}, dangerous scheme detection failed",
                $input,
            )
        };
    }

    check!("http://example.com/", false);
    check!("https://example.com/", false);
    check!("irc://irc.scpwiki.com", false);
    check!("javascript:alert(1)", true);
    check!("JAVASCRIPT:alert(1)", true);
    check!(" javascript:alert(1)", true);
    check!("java\nscript:alert(1)", true);
    check!("javascript\t:alert(1)", true);
    check!("wtf$1:foo", true);
    check!("JaVaScRiPt:alert(document.cookie)", true);
    check!("data:text/plain;base64,SGVsbG8sIFdvcmxkIQ==", true);
    check!("data:text/javascript,alert(1)", true);
    check!("data:text/html,<script>alert('XSS');</script>", true);
    check!("DATA:text/html,<script>alert('XSS');</script>", true);
    check!("/page", false);
    check!("/page#target", false);
    check!("/page/edit", false);
    check!("/page/edit#target", false);
    check!("/category:page", false);
    check!("/category:page#target", false);
    check!("/category:page/edit", false);
    check!("/category:page/edit#target", false);
}

#[test]
fn test_normalize_href() {
    macro_rules! check {
        ($input:expr => $expected:expr $(,)?) => {{
            let actual = normalize_href($input, None);
            assert_eq!(
                actual.as_ref(),
                $expected,
                "For input {:?}, normalize_href() doesn't match expected",
                $input,
            );
        }};

        ($url_input:expr, $extra_input:expr => $expected:expr $(,)?) => {{
            let actual = normalize_href($url_input, Some($extra_input));
            assert_eq!(
                actual.as_ref(),
                $expected,
                "For input {:?} / {:?}, normalize_href() doesn't match expected",
                $url_input,
                $extra_input,
            );
        }};

        // For when the input is the same as the output
        ($input:expr) => {
            check!($input => $input)
        };
    }

    // Basic targets
    check!("#");
    check!("#target");
    check!("#edit-area");
    check!("javascript:;");
    check!("http://example.net");
    check!("https://example.net");
    check!("irc://irc.scpwiki.com");
    check!("sftp://ftp.example.com/upload");

    // Dangerous
    check!("javascript:alert(1)" => "#invalid-url");
    check!(
        "data:text/html,<script>alert('XSS')</script>" => "#invalid-url",
    );

    // Preserve page links
    check!("/page");
    check!("/page", "#target" => "/page#target");
    check!("/page", "/edit" => "/page/edit");
    check!("page", "/edit#target" => "/page/edit#target");
    check!("/category:page");
    check!("/category:page", "#target" => "/category:page#target");
    check!("/category:page", "/edit" => "/category:page/edit");
    check!("/category:page", "/edit#target" => "/category:page/edit#target");

    // Missing / prefix
    check!("some-page" => "/some-page");
    check!("some-page#target" => "/some-page#target");
    check!("system:some-page" => "/system:some-page");
    check!("system:some-page#target" => "/system:some-page#target");
}
