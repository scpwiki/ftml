/*
 * tree/file_source.rs
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
use crate::url::is_url;
use std::borrow::Cow;
use strum_macros::IntoStaticStr;

#[derive(Serialize, Deserialize, IntoStaticStr, Debug, Hash, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case", tag = "type", content = "data")]
pub enum FileSource<'a> {
    /// File is sourced from an arbitrary URL.
    Url(Cow<'a, str>),

    /// File is attached the current page.
    File1 { file: Cow<'a, str> },

    /// File is attached to another page on the site.
    File2 {
        page: Cow<'a, str>,
        file: Cow<'a, str>,
    },

    /// File is attached to another page on another site.
    File3 {
        site: Cow<'a, str>,
        page: Cow<'a, str>,
        file: Cow<'a, str>,
    },
}

impl<'t> FileSource<'t> {
    pub fn parse(source: &'t str) -> Option<FileSource<'t>> {
        if is_url(source) || source.starts_with("/local--files/") {
            return Some(FileSource::Url(cow!(source)));
        }

        let source = source.strip_prefix('/').unwrap_or(source);

        let parts: Vec<&str> = source.split('/').collect();

        let source = match parts.len() {
            1 => FileSource::File1 {
                file: cow!(parts[0]),
            },
            2 => FileSource::File2 {
                page: cow!(parts[0]),
                file: cow!(parts[1]),
            },
            3 => FileSource::File3 {
                site: cow!(parts[0]),
                page: cow!(parts[1]),
                file: cow!(parts[2]),
            },
            _ => return None,
        };

        Some(source)
    }

    pub fn parse_wikidot(source: &'t str) -> Option<FileSource<'t>> {
        let quoted_inner = source
            .as_bytes()
            .first()
            .copied()
            .filter(|quote| matches!(quote, b'\'' | b'"'))
            .filter(|quote| source.as_bytes().last() == Some(quote))
            .and_then(|_| source.get(1..source.len().saturating_sub(1)));
        if quoted_inner.is_some_and(is_url) {
            return Some(FileSource::Url(cow!(source)));
        }
        if quoted_inner.is_some_and(is_safe_quoted_wikidot_filename) {
            return Some(FileSource::File1 { file: cow!(source) });
        }
        if quoted_inner.is_some() {
            return None;
        }
        if is_url(source) || source.starts_with("/local--files/") {
            return Some(FileSource::Url(cow!(source)));
        }

        if source.starts_with('/') && !source[1..].contains('/') {
            return Some(FileSource::File1 { file: cow!(source) });
        }

        let source = source.strip_prefix('/').unwrap_or(source);
        match source.rsplit_once('/') {
            Some((page, file)) if !page.is_empty() && !file.is_empty() => {
                Some(FileSource::File2 {
                    page: cow!(page),
                    file: cow!(file),
                })
            }
            None if !source.is_empty() => Some(FileSource::File1 { file: cow!(source) }),
            _ => None,
        }
    }

    #[inline]
    pub fn name(&self) -> &'static str {
        self.into()
    }

    pub fn to_owned(&self) -> FileSource<'static> {
        match self {
            FileSource::Url(url) => FileSource::Url(string_to_owned(url)),
            FileSource::File1 { file } => FileSource::File1 {
                file: string_to_owned(file),
            },
            FileSource::File2 { page, file } => FileSource::File2 {
                page: string_to_owned(page),
                file: string_to_owned(file),
            },
            FileSource::File3 { site, page, file } => FileSource::File3 {
                site: string_to_owned(site),
                page: string_to_owned(page),
                file: string_to_owned(file),
            },
        }
    }
}

fn is_safe_quoted_wikidot_filename(filename: &str) -> bool {
    if filename.is_empty() || matches!(filename, "." | "..") {
        return false;
    }
    if filename
        .bytes()
        .any(|byte| byte.is_ascii_control() || matches!(byte, b'/' | b'\\' | b'?' | b'#'))
    {
        return false;
    }

    let lowercase = filename.to_ascii_lowercase();
    !["%2f", "%5c", "%3f", "%23", "%2e%2e", "%2e.", ".%2e", "%25"]
        .iter()
        .any(|syntax| lowercase.contains(syntax))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_file_source_variants() {
        assert_eq!(
            FileSource::parse("https://example.com/image.png"),
            Some(FileSource::Url(cow!("https://example.com/image.png"))),
        );
        assert_eq!(
            FileSource::parse("/image.png"),
            Some(FileSource::File1 {
                file: cow!("image.png"),
            }),
        );
        assert_eq!(
            FileSource::parse("/page/image.png"),
            Some(FileSource::File2 {
                page: cow!("page"),
                file: cow!("image.png"),
            }),
        );
        assert_eq!(
            FileSource::parse("/site/page/image.png"),
            Some(FileSource::File3 {
                site: cow!("site"),
                page: cow!("page"),
                file: cow!("image.png"),
            }),
        );
    }

    #[test]
    fn parses_canonical_wikidot_local_files_path_as_literal_url() {
        assert_eq!(
            FileSource::parse("/local--files/page/assets/charts/image.png"),
            Some(FileSource::Url(cow!(
                "/local--files/page/assets/charts/image.png"
            ))),
        );
    }

    #[test]
    fn rejects_unknown_deeper_path_shapes() {
        assert_eq!(FileSource::parse("site/page/assets/image.png"), None);
        assert_eq!(
            FileSource::parse_wikidot("site/page/assets/image.png"),
            Some(FileSource::File2 {
                page: cow!("site/page/assets"),
                file: cow!("image.png"),
            }),
        );
        assert_eq!(
            FileSource::parse_wikidot("/local--files/page/image.png"),
            Some(FileSource::Url(cow!("/local--files/page/image.png"))),
        );
        assert_eq!(
            FileSource::parse_wikidot("/image.png"),
            Some(FileSource::File1 {
                file: cow!("/image.png"),
            }),
        );
    }

    #[test]
    fn wikidot_preserves_quotes_around_absolute_image_urls() {
        for source in [
            r#""https://example.com/image.png""#,
            "'https://example.com/image.png'",
        ] {
            assert_eq!(
                FileSource::parse_wikidot(source),
                Some(FileSource::Url(cow!(source))),
                "{source}",
            );
        }

        for source in [r#""image.png""#, "'image.png'", "'Ranch_Hand'_run.jpg"] {
            assert_eq!(
                FileSource::parse_wikidot(source),
                Some(FileSource::File1 { file: cow!(source) }),
                "{source}",
            );
        }
    }

    #[test]
    fn wikidot_rejects_structural_quoted_local_paths() {
        for source in [
            r#""/local--files/page/image.png""#,
            "'/local--files/page/image.png'",
            r#""page/image.png""#,
            "'../private/image.png?x'",
            r#""%2e%2e%2fprivate%2fimage.png""#,
            r#""double%252fencoded.png""#,
            r#""fragment#image.png""#,
        ] {
            assert_eq!(FileSource::parse_wikidot(source), None, "{source}");
        }
    }
}
