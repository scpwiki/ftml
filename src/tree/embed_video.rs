/*
 * tree/embed_video.rs
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
use serde::{Deserializer, Serializer};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::fmt::{self, Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceSha256([u8; 32]);

impl SourceSha256 {
    pub(crate) fn digest(source: &str) -> Self {
        Self(Sha256::digest(source.as_bytes()).into())
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        use std::fmt::Write;

        let mut output = String::with_capacity(64);
        for byte in self.0 {
            write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
        }
        output
    }
}

impl Display for SourceSha256 {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl serde::Serialize for SourceSha256 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> serde::Deserialize<'de> for SourceSha256 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        let value = <Cow<'_, str> as serde::Deserialize>::deserialize(deserializer)?;
        if value.len() != 64 {
            return Err(D::Error::custom(
                "SHA-256 must contain 64 hexadecimal digits",
            ));
        }
        fn nibble(byte: u8) -> Option<u8> {
            match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            }
        }

        let mut digest = [0; 32];
        for (pair, output) in value.as_bytes().as_chunks::<2>().0.iter().zip(&mut digest)
        {
            let Some(high) = nibble(pair[0]) else {
                return Err(D::Error::custom("SHA-256 contains a non-hexadecimal digit"));
            };
            let Some(low) = nibble(pair[1]) else {
                return Err(D::Error::custom("SHA-256 contains a non-hexadecimal digit"));
            };
            *output = (high << 4) | low;
        }
        Ok(Self(digest))
    }
}

/// An authored Wikidot `[[embedvideo]]` owner with an opaque payload.
///
/// FTML deliberately does not interpret providers, URLs, or embedded HTML.
#[derive(Serialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub struct EmbedVideo<'t> {
    source: Cow<'t, str>,
    payload: Cow<'t, str>,
    source_sha256: SourceSha256,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct EmbedVideoData {
    source: String,
    payload: String,
    source_sha256: SourceSha256,
}

impl<'de, 't> serde::Deserialize<'de> for EmbedVideo<'t> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        let data = EmbedVideoData::deserialize(deserializer)?;
        if SourceSha256::digest(&data.source) != data.source_sha256 {
            return Err(D::Error::custom(
                "embedvideo source does not match its SHA-256 identity",
            ));
        }
        let payload_start = "[[embedvideo]]".len();
        let payload_end = payload_start + data.payload.len();
        let opener_is_exact = data
            .source
            .get(..payload_start)
            .is_some_and(|opener| opener.eq_ignore_ascii_case("[[embedvideo]]"));
        let payload_is_exact =
            data.source.get(payload_start..payload_end) == Some(data.payload.as_str());
        let closer_is_complete = data
            .source
            .get(payload_end..)
            .is_some_and(|closer| closer.starts_with("[[/") && closer.ends_with("]]"));
        if !opener_is_exact || !payload_is_exact || !closer_is_complete {
            return Err(D::Error::custom(
                "embedvideo payload is not the exact body of its source",
            ));
        }
        Ok(Self {
            source: Cow::Owned(data.source),
            payload: Cow::Owned(data.payload),
            source_sha256: data.source_sha256,
        })
    }
}

impl<'t> EmbedVideo<'t> {
    pub(crate) fn new(source: &'t str, payload: &'t str) -> Self {
        Self {
            source: Cow::Borrowed(source),
            payload: Cow::Borrowed(payload),
            source_sha256: SourceSha256::digest(source),
        }
    }

    pub(crate) fn new_cow(source: Cow<'t, str>, payload: &'t str) -> Self {
        Self {
            source_sha256: SourceSha256::digest(&source),
            source,
            payload: Cow::Borrowed(payload),
        }
    }

    /// Returns the exact authored block, including its opener and closer.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns the exact uninterpreted bytes between the opener and closer.
    pub fn payload(&self) -> &str {
        &self.payload
    }

    pub const fn source_sha256(&self) -> SourceSha256 {
        self.source_sha256
    }

    pub fn to_owned(&self) -> EmbedVideo<'static> {
        EmbedVideo {
            source: string_to_owned(&self.source),
            payload: string_to_owned(&self.payload),
            source_sha256: self.source_sha256,
        }
    }
}
