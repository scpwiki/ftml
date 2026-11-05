#![allow(dead_code)]
// Verbatim copy of deepwell compat/fallback_code.rs (non-test part) for the differential probe.
use crate::tree::CodeBlock;
use std::borrow::Cow;

const MAX_COMPAT_CODE_BLOCKS: usize = 4_096;
const MAX_CODE_LANGUAGE_BYTES: usize = 64;
const MAX_CODE_NAME_BYTES: usize = 255;

#[derive(Debug)]
pub(crate) struct WikidotCompatibilityFallbackOutput {
    pub(crate) body: String,
    pub(crate) html_block_texts: Vec<String>,
    pub(crate) code_blocks: Vec<CodeBlock<'static>>,
}

impl WikidotCompatibilityFallbackOutput {
    pub(crate) fn body(body: String) -> Self {
        Self {
            body,
            html_block_texts: Vec::new(),
            code_blocks: Vec::new(),
        }
    }
}

/// Where a compatibility render's `[[html]]` blocks are served from, and
/// what its SCPnet interwiki frames report.
///
/// Hosted blocks are looked up under the slug of the page whose saved
/// render registered them. A page's own blocks register under the page
/// itself, but a navigation bar renders in the viewed page's context while
/// its blocks stay registered under the navigation page, so a navigation
/// render names that page as the owner. The interwiki frames always
/// report the page being viewed, as they do on Wikidot.
#[derive(Clone, Copy, Default)]
pub(crate) struct WikidotHtmlBlockEmission<'a> {
    pub owner_page_slug: Option<&'a str>,
    pub root_page_slug: Option<&'a str>,
    pub page_language: Option<&'a str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompatCodeBlock {
    pub(crate) start_line: usize,
    pub(crate) end_line: usize,
    pub(crate) language: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) contents: String,
}

impl CompatCodeBlock {
    pub(crate) fn into_ftml(self) -> CodeBlock<'static> {
        CodeBlock {
            contents: Cow::Owned(self.contents),
            language: self.language.map(Cow::Owned),
            name: self.name.map(Cow::Owned),
            wikidot_block_open_elements: Vec::new(),
        }
    }
}

/// Locates complete Wikidot code blocks without consuming ambiguous input.
///
/// This is a compatibility boundary rather than a second wikitext parser. If a code-looking marker is malformed, nested, unmatched, or exceeds the bounded representation below, the entire scan fails closed. The caller must then render the original source literally so no text can disappear. FTML should eventually expose this as a delayed structure; Deepwell only retains the metadata here because hosted `/local--code/` resources require runtime persistence today.
pub(crate) fn scan_compat_code_blocks(
    wikitext: &str,
) -> Result<Vec<CompatCodeBlock>, CompatCodeScanError> {
    let mut blocks = Vec::new();
    let mut open: Option<(usize, CodeMarker, String)> = None;

    for (line_index, line) in wikitext.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some((_, _, contents)) = &mut open {
            if is_code_close(trimmed) {
                let (start_line, marker, contents) = open.take().expect("open block");
                if blocks.len() == MAX_COMPAT_CODE_BLOCKS {
                    return Err(CompatCodeScanError);
                }
                blocks.push(CompatCodeBlock {
                    start_line,
                    end_line: line_index,
                    language: marker.language,
                    name: marker.name,
                    contents: contents.trim_end_matches('\n').to_owned(),
                });
            } else if looks_like_code_open(trimmed) {
                return Err(CompatCodeScanError);
            } else {
                contents.push_str(line);
                contents.push('\n');
            }
        } else if looks_like_code_close(trimmed) {
            return Err(CompatCodeScanError);
        } else if looks_like_code_open(trimmed) {
            let marker = parse_code_open(trimmed).ok_or(CompatCodeScanError)?;
            open = Some((line_index, marker, String::new()));
        }
    }

    if open.is_some() {
        return Err(CompatCodeScanError);
    }
    Ok(blocks)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CompatCodeScanError;

#[derive(Clone, Debug, Eq, PartialEq)]
struct CodeMarker {
    language: Option<String>,
    name: Option<String>,
}

fn looks_like_code_open(marker: &str) -> bool {
    let Some(prefix) = marker.get(..6) else {
        return false;
    };
    if !prefix.eq_ignore_ascii_case("[[code") {
        return false;
    }
    let Some(rest) = marker.get(6..) else {
        return false;
    };
    rest.starts_with("]]") || rest.starts_with(char::is_whitespace)
}

fn looks_like_code_close(marker: &str) -> bool {
    marker
        .get(..9)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("[[/code]]"))
        && marker[9..].trim().is_empty()
}

fn is_code_close(marker: &str) -> bool {
    looks_like_code_close(marker)
}

fn parse_code_open(marker: &str) -> Option<CodeMarker> {
    let inner = marker.strip_prefix("[[")?.strip_suffix("]]")?;
    parse_code_open_inner(inner)
}

fn parse_code_open_inner(inner: &str) -> Option<CodeMarker> {
    let mut rest = inner.trim();
    if !rest.get(..4)?.eq_ignore_ascii_case("code") {
        return None;
    }
    rest = &rest[4..];
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }

    let mut marker = CodeMarker {
        language: None,
        name: None,
    };
    while !rest.trim_start().is_empty() {
        rest = rest.trim_start();
        let equals = rest.find('=')?;
        let key = rest[..equals].trim();
        if key.is_empty() || key.chars().any(char::is_whitespace) {
            return None;
        }
        rest = &rest[equals + 1..];
        let quote = rest.as_bytes().first().copied()?;
        if quote != b'"' && quote != b'\'' {
            return None;
        }
        rest = &rest[1..];
        let end = rest.find(char::from(quote))?;
        let value = &rest[..end];
        rest = &rest[end + 1..];

        if key.eq_ignore_ascii_case("type") {
            if marker.language.is_some()
                || value.is_empty()
                || value.len() > MAX_CODE_LANGUAGE_BYTES
            {
                return None;
            }
            marker.language = Some(value.to_owned());
        } else if key.eq_ignore_ascii_case("name") {
            if marker.name.is_some()
                || value.is_empty()
                || value.len() > MAX_CODE_NAME_BYTES
            {
                return None;
            }
            marker.name = Some(value.to_owned());
        }
    }
    Some(marker)
}
