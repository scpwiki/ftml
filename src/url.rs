/*
 * url.rs
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

use regex::Regex;
use std::borrow::Cow;
use std::sync::LazyLock;

#[cfg(feature = "html")]
use crate::tree::LinkLocation;

#[cfg(test)]
const URL_SCHEMES: [&str; 19] = [
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
    match url.as_bytes().first().copied() {
        Some(b'b') => url.starts_with("blob:"),
        Some(b'c') => {
            url.starts_with("chrome-extension://")
                || url.starts_with("chrome://")
                || url.starts_with("content://")
                || url.starts_with("cvs://")
        }
        Some(b'd') => url.starts_with("dns:"),
        Some(b'f') => {
            url.starts_with("feed:")
                || url.starts_with("file://")
                || url.starts_with("ftp://")
        }
        Some(b'g') => url.starts_with("git://") || url.starts_with("gopher://"),
        Some(b'h') => url.starts_with("http://") || url.starts_with("https://"),
        Some(b'i') => {
            url.starts_with("irc6://")
                || url.starts_with("irc://")
                || url.starts_with("ircs://")
        }
        Some(b'm') => url.starts_with("mailto:"),
        Some(b'r') => url.starts_with("resource://") || url.starts_with("rtmp://"),
        Some(b's') => {
            url.starts_with("sftp://")
                || url.starts_with("ssh://")
                || url.starts_with("svn://")
        }
        Some(b't') => url.starts_with("telnet://"),
        _ => false,
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum HrefKind {
    NoOp,
    Invalid,
    Anchor,
    External,
    AbsolutePath,
    Relative,
}

pub(crate) fn classify_href(url: &str) -> HrefKind {
    if url == "javascript:;" {
        HrefKind::NoOp
    } else if url.starts_with('#') {
        HrefKind::Anchor
    } else if url.starts_with("//")
        || url.starts_with("news://")
        || url.starts_with("mms://")
        || is_url(url)
    {
        HrefKind::External
    } else if url.starts_with('/') {
        HrefKind::AbsolutePath
    } else if dangerous_scheme(url) {
        HrefKind::Invalid
    } else {
        HrefKind::Relative
    }
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
                None => normalize_page_href(page, extra),
            }
        }
    }
}

#[cfg(feature = "html")]
fn normalize_page_href<'a>(page: &'a str, extra: Option<&'a str>) -> Cow<'a, str> {
    let extra = extra.unwrap_or("");
    let href = if page.starts_with('/') {
        format!("{page}{extra}")
    } else {
        format!("/{page}{extra}")
    };
    Cow::Owned(percent_encode_non_ascii(&href).into_owned())
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
    match classify_href(url) {
        HrefKind::NoOp => {
            trace!("Leaving no-op link as-is");
            Cow::Borrowed(url)
        }
        HrefKind::External => match extra {
            Some(extra) => {
                trace!("Encoding a safe external URL with extra: {url}{extra}");
                Cow::Owned(
                    percent_encode_non_ascii(&format!("{url}{extra}")).into_owned(),
                )
            }
            None => {
                trace!("Encoding a safe external URL: {url}");
                percent_encode_non_ascii(url)
            }
        },
        HrefKind::Anchor => match extra {
            Some(extra) => {
                trace!("Encoding a safe URL with extra: {url}{extra}");
                Cow::Owned(
                    percent_encode_non_ascii(&format!("{url}{extra}")).into_owned(),
                )
            }
            None => {
                trace!("Encoding a safe URL: {url}");
                percent_encode_non_ascii(url)
            }
        },
        HrefKind::AbsolutePath => match extra {
            Some(extra) => {
                trace!("Encoding a safe absolute path with extra: {url}{extra}");
                Cow::Owned(
                    percent_encode_non_ascii(&format!("{url}{extra}")).into_owned(),
                )
            }
            None => {
                trace!("Encoding a safe absolute path: {url}");
                percent_encode_non_ascii(url)
            }
        },
        HrefKind::Invalid => {
            warn!("Attempt to pass in dangerous URL: {url}");
            Cow::Borrowed("#invalid-url")
        }
        HrefKind::Relative => {
            let extra = extra.unwrap_or("");
            trace!("Adding leading slash to URL: {url}{extra}");
            Cow::Owned(format!("/{url}{extra}"))
        }
    }
}

pub(crate) fn percent_encode_non_ascii(value: &str) -> Cow<'_, str> {
    if value.is_ascii() {
        return Cow::Borrowed(value);
    }

    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii() {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    Cow::Owned(encoded)
}

pub trait BuildSiteUrl {
    fn build_url(&self, site: &str, path: &str, extra: Option<&str>) -> String;
}

#[test]
fn detect_supported_url_schemes() {
    for scheme in URL_SCHEMES {
        let input = format!("{scheme}example");
        assert!(is_url(&input), "{scheme}");
    }

    for input in ["", "page", "Http://example.com", "javascript:alert(1)"] {
        assert!(!is_url(input), "{input}");
    }
}

#[test]
fn detect_dangerous_schemes() {
    macro_rules! test {
        ($input:expr, $result:expr $(,)?) => {
            assert_eq!(
                dangerous_scheme($input),
                $result,
                "For input {:?}, dangerous scheme detection failed",
                $input,
            )
        };
    }

    test!("http://example.com/", false);
    test!("https://example.com/", false);
    test!("irc://irc.scpwiki.com", false);
    test!("javascript:alert(1)", true);
    test!("JAVASCRIPT:alert(1)", true);
    test!(" javascript:alert(1)", true);
    test!("java\nscript:alert(1)", true);
    test!("javascript\t:alert(1)", true);
    test!("wtf$1:foo", true);
    test!("JaVaScRiPt:alert(document.cookie)", true);
    test!("data:text/plain;base64,SGVsbG8sIFdvcmxkIQ==", true);
    test!("data:text/javascript,alert(1)", true);
    test!("data:text/html,<script>alert('XSS');</script>", true);
    test!("DATA:text/html,<script>alert('XSS');</script>", true);
    test!("/page", false);
    test!("/page#target", false);
    test!("/page/edit", false);
    test!("/page/edit#target", false);
    test!("/category:page", false);
    test!("/category:page#target", false);
    test!("/category:page/edit", false);
    test!("/category:page/edit#target", false);
}

#[cfg(feature = "html")]
#[test]
fn normalize_link_preserves_local_pages_with_dangerous_scheme_names() {
    struct TestSiteUrl;

    impl BuildSiteUrl for TestSiteUrl {
        fn build_url(&self, site: &str, path: &str, extra: Option<&str>) -> String {
            format!(":{site}:{path}{}", extra.unwrap_or(""))
        }
    }

    macro_rules! test {
        ($input:expr => $expected:expr $(,)?) => {{
            let location = LinkLocation::Page(crate::data::PageRef::page_only($input));
            let actual = normalize_link(&location, &TestSiteUrl);
            assert_eq!(
                actual.as_ref(),
                $expected,
                "For local page {:?}, normalize_link() doesn't match expected",
                $input,
            );
        }};
    }

    test!("data:example" => "/data:example");
    test!("javascript:example" => "/javascript:example");
    test!("data:example#target" => "/data:example#target");
    test!("javascript:example/edit" => "/javascript:example/edit");
    test!(
        "system:page-tags/tag/파무왁"
            => "/system:page-tags/tag/%ED%8C%8C%EB%AC%B4%EC%99%81",
    );

    let location = LinkLocation::Page(crate::data::PageRef {
        site: None,
        page: "/data:example".into(),
        extra: Some("#target".into()),
    });
    assert_eq!(
        normalize_link(&location, &TestSiteUrl).as_ref(),
        "/data:example#target",
        "An already root-relative local page must not gain a second leading slash",
    );
}

#[test]
fn test_normalize_href() {
    macro_rules! test {
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
            test!($input => $input)
        };
    }

    // Basic targets
    test!("#");
    test!("#target");
    test!("#edit-area");
    test!("javascript:;");
    test!("http://example.net");
    test!("https://example.net");
    test!(
        "https://ja.wikipedia.org/wiki/グイン・サーガ"
            => "https://ja.wikipedia.org/wiki/%E3%82%B0%E3%82%A4%E3%83%B3%E3%83%BB%E3%82%B5%E3%83%BC%E3%82%AC",
    );
    test!(
        "https://example.net/already/%E3/日?q=x#y"
            => "https://example.net/already/%E3/%E6%97%A5?q=x#y",
    );
    test!("irc://irc.scpwiki.com");
    test!("sftp://ftp.example.com/upload");

    // Dangerous
    test!("javascript:alert(1)" => "#invalid-url");
    test!(
        "data:text/html,<script>alert('XSS')</script>" => "#invalid-url",
    );

    // Preserve page links
    test!("/page");
    test!("/page", "#target" => "/page#target");
    test!("/page", "/edit" => "/page/edit");
    test!("page", "/edit#target" => "/page/edit#target");
    test!("/category:page");
    test!("/category:page", "#target" => "/category:page#target");
    test!("/category:page", "/edit" => "/category:page/edit");
    test!("/category:page", "/edit#target" => "/category:page/edit#target");
    test!(
        "/system:page-tags/tag/파무왁"
            => "/system:page-tags/tag/%ED%8C%8C%EB%AC%B4%EC%99%81",
    );

    // Missing / prefix
    test!("some-page" => "/some-page");
    test!("some-page#target" => "/some-page#target");
    test!("system:some-page" => "/system:some-page");
    test!("system:some-page#target" => "/system:some-page#target");
}
