/*
 * tree/embed.rs
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
use std::borrow::Cow;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case", tag = "embed", content = "data")]
pub enum Embed<'t> {
    #[serde(rename_all = "kebab-case")]
    Youtube { video_id: Cow<'t, str> },

    #[serde(rename_all = "kebab-case")]
    Vimeo { video_id: Cow<'t, str> },

    GithubGist {
        username: Cow<'t, str>,
        hash: Cow<'t, str>,
    },

    #[serde(rename_all = "kebab-case")]
    GitlabSnippet { snippet_id: Cow<'t, str> },

    #[serde(rename_all = "kebab-case")]
    Iframe {
        url: Cow<'t, str>,
        html: Cow<'t, str>,
    },
}

impl Embed<'_> {
    pub fn name(&self) -> &'static str {
        match self {
            Embed::Youtube { .. } => "YouTube",
            Embed::Vimeo { .. } => "Vimeo",
            Embed::GithubGist { .. } => "GithubGist",
            Embed::GitlabSnippet { .. } => "GitlabSnippet",
            Embed::Iframe { .. } => "Iframe",
        }
    }

    pub fn direct_url(&self) -> String {
        match self {
            Embed::Youtube { video_id } => format!("https://youtu.be/{video_id}"),
            Embed::Vimeo { video_id } => format!("https://vimeo.com/{video_id}"),
            Embed::GithubGist { username, hash } => {
                format!("https://gist.github.com/{username}/{hash}")
            }
            Embed::GitlabSnippet { snippet_id } => {
                format!("https://gitlab.com/-/snippets/{snippet_id}")
            }

            Embed::Iframe { url, .. } => url.to_string(),
        }
    }

    pub fn to_owned(&self) -> Embed<'static> {
        match self {
            Embed::Youtube { video_id } => Embed::Youtube {
                video_id: string_to_owned(video_id),
            },

            Embed::Vimeo { video_id } => Embed::Vimeo {
                video_id: string_to_owned(video_id),
            },

            Embed::GithubGist { username, hash } => Embed::GithubGist {
                username: string_to_owned(username),
                hash: string_to_owned(hash),
            },

            Embed::GitlabSnippet { snippet_id } => Embed::GitlabSnippet {
                snippet_id: string_to_owned(snippet_id),
            },

            Embed::Iframe { url, html } => Embed::Iframe {
                url: string_to_owned(url),
                html: string_to_owned(html),
            },
        }
    }
}

#[test]
fn embed_helpers_cover_all_variants() {
    let cases = [
        (
            Embed::Youtube {
                video_id: cow!("yt-id"),
            },
            "YouTube",
            "https://youtu.be/yt-id",
        ),
        (
            Embed::Vimeo {
                video_id: cow!("12345"),
            },
            "Vimeo",
            "https://vimeo.com/12345",
        ),
        (
            Embed::GithubGist {
                username: cow!("user"),
                hash: cow!("abc123"),
            },
            "GithubGist",
            "https://gist.github.com/user/abc123",
        ),
        (
            Embed::GitlabSnippet {
                snippet_id: cow!("98765"),
            },
            "GitlabSnippet",
            "https://gitlab.com/-/snippets/98765",
        ),
    ];

    for (embed, expected_name, expected_url) in cases {
        assert_eq!(embed.name(), expected_name);
        assert_eq!(embed.direct_url(), expected_url);
        assert_eq!(embed.to_owned().direct_url(), expected_url);
    }
}
