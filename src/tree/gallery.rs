/*
 * tree/gallery.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2026 Wikijump Team
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

use super::clone::string_to_owned;
use super::embed_video::SourceSha256;
use serde::{Deserializer, Serialize};
use std::borrow::Cow;

#[derive(Serialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub struct Gallery<'t> {
    source: Cow<'t, str>,
    arguments: Vec<GalleryArgument<'t>>,
    selection: GallerySelection<'t>,
    source_sha256: SourceSha256,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub struct GalleryArgument<'t> {
    source: Cow<'t, str>,
    name: Cow<'t, str>,
    value: Cow<'t, str>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case", tag = "type", content = "entries")]
pub enum GallerySelection<'t> {
    CurrentPageFiles,
    Explicit(Vec<GalleryEntry<'t>>),
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub struct GalleryEntry<'t> {
    source: Cow<'t, str>,
    image: GalleryEntrySource<'t>,
    arguments: Vec<GalleryArgument<'t>>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case", tag = "type", content = "source")]
pub enum GalleryEntrySource<'t> {
    HttpUrl(Cow<'t, str>),
    File(Cow<'t, str>),
    Inert(Cow<'t, str>),
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct GalleryData {
    source: String,
    arguments: Vec<GalleryArgumentData>,
    selection: GallerySelectionData,
    source_sha256: SourceSha256,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct GalleryArgumentData {
    source: String,
    name: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", tag = "type", content = "entries")]
enum GallerySelectionData {
    CurrentPageFiles,
    Explicit(Vec<GalleryEntryData>),
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct GalleryEntryData {
    source: String,
    image: GalleryEntrySourceData,
    arguments: Vec<GalleryArgumentData>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", tag = "type", content = "source")]
enum GalleryEntrySourceData {
    HttpUrl(String),
    File(String),
    Inert(String),
}

impl<'de, 't> serde::Deserialize<'de> for Gallery<'t> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        let data = GalleryData::deserialize(deserializer)?;
        if SourceSha256::digest(&data.source) != data.source_sha256 {
            return Err(D::Error::custom(
                "gallery source does not match its SHA-256 identity",
            ));
        }
        let Some(head_end) = gallery_head_end(&data.source) else {
            return Err(D::Error::custom("gallery source has no valid opener"));
        };
        let expected_arguments =
            parse_gallery_head_arguments(&data.source[..head_end], true).ok_or_else(
                || D::Error::custom("gallery source has malformed arguments"),
            )?;
        if !argument_data_matches(&expected_arguments, &data.arguments) {
            return Err(D::Error::custom(
                "gallery arguments do not match their authored source",
            ));
        }

        let selection = match data.selection {
            GallerySelectionData::CurrentPageFiles => {
                if head_end != data.source.len() {
                    return Err(D::Error::custom(
                        "current-page gallery source contains an unexpected body",
                    ));
                }
                GallerySelection::CurrentPageFiles
            }
            GallerySelectionData::Explicit(entries) => {
                let Some((expected, owner_end, residual)) =
                    parse_explicit_gallery_entries(&data.source, head_end)
                else {
                    return Err(D::Error::custom(
                        "explicit gallery source has no valid entry body",
                    ));
                };
                if owner_end != data.source.len()
                    || residual
                    || expected.len() != entries.len()
                {
                    return Err(D::Error::custom(
                        "explicit gallery source has mismatched ownership",
                    ));
                }
                let mut output = Vec::with_capacity(entries.len());
                for (expected, actual) in expected.into_iter().zip(entries) {
                    if expected.source() != actual.source
                        || !argument_data_matches(expected.arguments(), &actual.arguments)
                        || !entry_source_data_matches(expected.image(), &actual.image)
                    {
                        return Err(D::Error::custom(
                            "gallery entry does not match its authored source",
                        ));
                    }
                    output.push(actual.into_gallery_entry());
                }
                GallerySelection::Explicit(output)
            }
        };

        Ok(Gallery {
            source: Cow::Owned(data.source),
            arguments: data
                .arguments
                .into_iter()
                .map(GalleryArgumentData::into_gallery_argument)
                .collect(),
            selection,
            source_sha256: data.source_sha256,
        })
    }
}

impl GalleryArgumentData {
    fn into_gallery_argument(self) -> GalleryArgument<'static> {
        GalleryArgument {
            source: Cow::Owned(self.source),
            name: Cow::Owned(self.name),
            value: Cow::Owned(self.value),
        }
    }
}

impl GalleryEntryData {
    fn into_gallery_entry(self) -> GalleryEntry<'static> {
        GalleryEntry {
            source: Cow::Owned(self.source),
            image: self.image.into_gallery_entry_source(),
            arguments: self
                .arguments
                .into_iter()
                .map(GalleryArgumentData::into_gallery_argument)
                .collect(),
        }
    }
}

impl GalleryEntrySourceData {
    fn into_gallery_entry_source(self) -> GalleryEntrySource<'static> {
        match self {
            Self::HttpUrl(source) => GalleryEntrySource::HttpUrl(Cow::Owned(source)),
            Self::File(source) => GalleryEntrySource::File(Cow::Owned(source)),
            Self::Inert(source) => GalleryEntrySource::Inert(Cow::Owned(source)),
        }
    }
}

impl<'t> Gallery<'t> {
    pub(crate) fn new_cow(
        source: Cow<'t, str>,
        arguments: Vec<GalleryArgument<'t>>,
        selection: GallerySelection<'t>,
    ) -> Self {
        let source_sha256 = SourceSha256::digest(&source);
        Self {
            source,
            arguments,
            selection,
            source_sha256,
        }
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn arguments(&self) -> &[GalleryArgument<'t>] {
        &self.arguments
    }

    pub fn selection(&self) -> &GallerySelection<'t> {
        &self.selection
    }

    pub const fn source_sha256(&self) -> SourceSha256 {
        self.source_sha256
    }

    pub fn to_owned(&self) -> Gallery<'static> {
        Gallery {
            source: string_to_owned(&self.source),
            arguments: self
                .arguments
                .iter()
                .map(GalleryArgument::to_owned)
                .collect(),
            selection: self.selection.to_owned(),
            source_sha256: self.source_sha256,
        }
    }
}

impl<'t> GalleryArgument<'t> {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub(crate) fn to_owned(&self) -> GalleryArgument<'static> {
        GalleryArgument {
            source: string_to_owned(&self.source),
            name: string_to_owned(&self.name),
            value: string_to_owned(&self.value),
        }
    }
}

impl GallerySelection<'_> {
    fn to_owned(&self) -> GallerySelection<'static> {
        match self {
            Self::CurrentPageFiles => GallerySelection::CurrentPageFiles,
            Self::Explicit(entries) => GallerySelection::Explicit(
                entries.iter().map(GalleryEntry::to_owned).collect(),
            ),
        }
    }
}

impl<'t> GalleryEntry<'t> {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn image(&self) -> &GalleryEntrySource<'t> {
        &self.image
    }

    pub fn arguments(&self) -> &[GalleryArgument<'t>] {
        &self.arguments
    }

    fn to_owned(&self) -> GalleryEntry<'static> {
        GalleryEntry {
            source: string_to_owned(&self.source),
            image: self.image.to_owned(),
            arguments: self
                .arguments
                .iter()
                .map(GalleryArgument::to_owned)
                .collect(),
        }
    }
}

impl GalleryEntrySource<'_> {
    pub fn source(&self) -> &str {
        match self {
            Self::HttpUrl(source) | Self::File(source) | Self::Inert(source) => source,
        }
    }

    fn to_owned(&self) -> GalleryEntrySource<'static> {
        match self {
            Self::HttpUrl(source) => GalleryEntrySource::HttpUrl(string_to_owned(source)),
            Self::File(source) => GalleryEntrySource::File(string_to_owned(source)),
            Self::Inert(source) => GalleryEntrySource::Inert(string_to_owned(source)),
        }
    }
}

pub(crate) fn gallery_head_end(source: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    if !source.starts_with("[[") {
        return None;
    }
    let mut index = 2;
    let mut quote = None;
    while index + 1 < bytes.len() {
        match (quote, bytes[index]) {
            (Some(_), b'\\') => index = (index + 2).min(bytes.len()),
            (Some(active), byte) if byte == active => {
                quote = None;
                index += 1;
            }
            (Some(_), _) => index += 1,
            (None, byte @ (b'\'' | b'"')) => {
                quote = Some(byte);
                index += 1;
            }
            (None, b']') if bytes[index + 1] == b']' => return Some(index + 2),
            (None, _) => index += 1,
        }
    }
    None
}

pub(crate) fn parse_gallery_head_arguments(
    source: &str,
    recover_non_assignment: bool,
) -> Option<Vec<GalleryArgument<'_>>> {
    let head_end = gallery_head_end(source)?;
    if head_end != source.len() {
        return None;
    }
    let inner = source
        .strip_prefix("[[")
        .and_then(|inner| inner.strip_suffix("]]"))?;
    let inner = inner.trim_start_matches([' ', '\t']);
    let name_end = inner.find([' ', '\t', '\r', '\n']).unwrap_or(inner.len());
    if !inner[..name_end].eq_ignore_ascii_case("gallery") {
        return None;
    }
    let tail = &inner[name_end..];
    parse_gallery_arguments(tail)
        .or_else(|| (recover_non_assignment && !tail.contains('=')).then(Vec::new))
}

pub(crate) fn parse_explicit_gallery_entries<'t>(
    source: &'t str,
    head_end: usize,
) -> Option<(Vec<GalleryEntry<'t>>, usize, bool)> {
    let mut cursor = if source[head_end..].starts_with("\r\n") {
        head_end + 2
    } else if source[head_end..].starts_with('\n') {
        head_end + 1
    } else {
        return None;
    };
    let mut entries = Vec::new();

    loop {
        let line_start = cursor;
        let line_end = source[cursor..]
            .find('\n')
            .map_or(source.len(), |offset| cursor + offset);
        let line = source[line_start..line_end]
            .strip_suffix('\r')
            .unwrap_or(&source[line_start..line_end]);
        let trimmed = line.trim_matches([' ', '\t']);
        let residual = trimmed
            .get(.."[[/gallery]]]".len())
            .is_some_and(|close| close.eq_ignore_ascii_case("[[/gallery]]]"))
            && trimmed.len() == "[[/gallery]]]".len();
        let normal = trimmed.eq_ignore_ascii_case("[[/gallery]]");
        if normal || residual {
            if entries.is_empty() {
                return None;
            }
            let owner_end = if residual {
                line_start + line.len() - 1
            } else {
                line_end
            };
            return Some((entries, owner_end, residual));
        }

        entries.push(parse_gallery_entry(line)?);
        if line_end == source.len() {
            return None;
        }
        cursor = line_end + 1;
    }
}

pub(crate) fn parse_gallery_entry_owned(line: String) -> Option<GalleryEntry<'static>> {
    let entry = parse_gallery_entry(&line)?;
    let image = entry.image.to_owned();
    let arguments = entry
        .arguments
        .iter()
        .map(GalleryArgument::to_owned)
        .collect();
    Some(GalleryEntry {
        source: Cow::Owned(line),
        image,
        arguments,
    })
}

pub(crate) fn parse_gallery_entry(line: &str) -> Option<GalleryEntry<'_>> {
    let content = line.trim_start_matches([' ', '\t']);
    let tail = content.strip_prefix(':')?.trim_start_matches([' ', '\t']);
    let (image_source, arguments_source) = split_gallery_entry_source(tail)?;
    let image = classify_gallery_entry_source(image_source);
    let arguments = parse_gallery_arguments(arguments_source)?;
    Some(GalleryEntry {
        source: Cow::Borrowed(line),
        image,
        arguments,
    })
}

fn split_gallery_entry_source(value: &str) -> Option<(&str, &str)> {
    let first = *value.as_bytes().first()?;
    if matches!(first, b'\'' | b'"') {
        let end = value.as_bytes()[1..]
            .iter()
            .position(|byte| *byte == first)?
            + 2;
        return Some((&value[..end], &value[end..]));
    }
    let end = value.find([' ', '\t']).unwrap_or(value.len());
    (end > 0).then_some((&value[..end], &value[end..]))
}

fn classify_gallery_entry_source(source: &str) -> GalleryEntrySource<'_> {
    let unquoted = source
        .as_bytes()
        .first()
        .copied()
        .filter(|quote| matches!(quote, b'\'' | b'"'))
        .filter(|quote| source.as_bytes().last() == Some(quote))
        .and_then(|_| source.get(1..source.len().saturating_sub(1)))
        .unwrap_or(source);
    if unquoted
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
        || unquoted
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
    {
        GalleryEntrySource::HttpUrl(Cow::Borrowed(source))
    } else if crate::url::dangerous_scheme(unquoted)
        || unquoted.split_once(':').is_some()
        || unquoted.starts_with("//")
    {
        GalleryEntrySource::Inert(Cow::Borrowed(source))
    } else {
        GalleryEntrySource::File(Cow::Borrowed(source))
    }
}

fn parse_gallery_arguments(value: &str) -> Option<Vec<GalleryArgument<'_>>> {
    let bytes = value.as_bytes();
    let mut cursor = 0;
    let mut output = Vec::new();
    while cursor < bytes.len() {
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if cursor == bytes.len() {
            break;
        }
        let start = cursor;
        while bytes.get(cursor).is_some_and(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
        }) {
            cursor += 1;
        }
        if cursor == start {
            return None;
        }
        let name = &value[start..cursor];
        while bytes
            .get(cursor)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
        {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'=') {
            return None;
        }
        cursor += 1;
        while bytes
            .get(cursor)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
        {
            cursor += 1;
        }
        let (content_start, content_end) = match bytes.get(cursor).copied() {
            Some(quote @ (b'\'' | b'"')) => {
                cursor += 1;
                let content_start = cursor;
                while bytes.get(cursor).is_some_and(|byte| *byte != quote) {
                    cursor += 1;
                }
                if bytes.get(cursor) != Some(&quote) {
                    return None;
                }
                let content_end = cursor;
                cursor += 1;
                (content_start, content_end)
            }
            Some(_) => {
                let content_start = cursor;
                while bytes
                    .get(cursor)
                    .is_some_and(|byte| !byte.is_ascii_whitespace())
                {
                    cursor += 1;
                }
                (content_start, cursor)
            }
            None => return None,
        };
        output.push(GalleryArgument {
            source: Cow::Borrowed(&value[start..cursor]),
            name: Cow::Borrowed(name),
            value: Cow::Borrowed(&value[content_start..content_end]),
        });
    }
    Some(output)
}

fn argument_data_matches(
    expected: &[GalleryArgument<'_>],
    actual: &[GalleryArgumentData],
) -> bool {
    expected.len() == actual.len()
        && expected.iter().zip(actual).all(|(expected, actual)| {
            expected.source() == actual.source
                && expected.name() == actual.name
                && expected.value() == actual.value
        })
}

fn entry_source_data_matches(
    expected: &GalleryEntrySource<'_>,
    actual: &GalleryEntrySourceData,
) -> bool {
    match (expected, actual) {
        (
            GalleryEntrySource::HttpUrl(expected),
            GalleryEntrySourceData::HttpUrl(actual),
        )
        | (GalleryEntrySource::File(expected), GalleryEntrySourceData::File(actual))
        | (GalleryEntrySource::Inert(expected), GalleryEntrySourceData::Inert(actual)) => {
            expected == actual
        }
        _ => false,
    }
}
