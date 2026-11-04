/*
 * tree/social.rs
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

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum SocialService {
    BlinkList,
    Blogmarks,
    Delicious,
    Digg,
    Fark,
    FeedMeLinks,
    Furl,
    LinkaGoGo,
    NewsVine,
    Netvouz,
    Reddit,
    YahooMyWeb,
    Facebook,
}

impl SocialService {
    pub const DEFAULT: [Self; 13] = [
        Self::BlinkList,
        Self::Blogmarks,
        Self::Delicious,
        Self::Digg,
        Self::Fark,
        Self::FeedMeLinks,
        Self::Furl,
        Self::LinkaGoGo,
        Self::NewsVine,
        Self::Netvouz,
        Self::Reddit,
        Self::YahooMyWeb,
        Self::Facebook,
    ];

    pub const fn token(self) -> &'static str {
        match self {
            Self::BlinkList => "blinklist",
            Self::Blogmarks => "blogmarks",
            Self::Delicious => "del.icio.us",
            Self::Digg => "digg",
            Self::Fark => "fark",
            Self::FeedMeLinks => "feedmelinks",
            Self::Furl => "furl",
            Self::LinkaGoGo => "linkagogo",
            Self::NewsVine => "newsvine",
            Self::Netvouz => "netvouz",
            Self::Reddit => "reddit",
            Self::YahooMyWeb => "yahoomyweb",
            Self::Facebook => "facebook",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "blinklist" => Self::BlinkList,
            "blogmarks" => Self::Blogmarks,
            "del.icio.us" => Self::Delicious,
            "digg" => Self::Digg,
            "fark" => Self::Fark,
            "feedmelinks" => Self::FeedMeLinks,
            "furl" => Self::Furl,
            "linkagogo" => Self::LinkaGoGo,
            "newsvine" => Self::NewsVine,
            "netvouz" => Self::Netvouz,
            "reddit" => Self::Reddit,
            "yahoomyweb" => Self::YahooMyWeb,
            "facebook" => Self::Facebook,
            _ => return None,
        })
    }

    fn parse_case_insensitive(value: &str) -> Option<Self> {
        Self::DEFAULT
            .into_iter()
            .find(|service| service.token().eq_ignore_ascii_case(value))
    }
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[serde(tag = "type", content = "service", rename_all = "kebab-case")]
pub enum SocialSelection {
    Service(SocialService),
    Empty,
}

/// Typed Wikidot `[[social ...]]` selection.
///
/// `None` means Wikidot's default provider set. `Some([])` is intentionally
/// distinct: it represents an explicit selection whose only nonempty tokens
/// were syntactically invalid (for example uppercase provider names), for
/// which Wikidot renders an empty social span rather than defaulting.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SocialButtons {
    selection: Option<Vec<SocialSelection>>,
}

impl SocialButtons {
    pub fn parse(head: &str) -> Self {
        let head = head.trim_matches(is_wikidot_space);
        if head.is_empty() {
            return Self { selection: None };
        }

        let mut selection = Vec::new();

        for token in head.split(',') {
            let token = token.trim_matches(is_wikidot_space);
            if token.is_empty() {
                continue;
            }
            if let Some(service) = SocialService::parse(token) {
                selection.push(SocialSelection::Service(service));
            } else if SocialService::parse_case_insensitive(token).is_some() {
                selection.push(SocialSelection::Empty);
            }
        }

        if selection.is_empty() {
            Self { selection: None }
        } else {
            Self {
                selection: Some(selection),
            }
        }
    }

    pub fn selection(&self) -> Option<&[SocialSelection]> {
        self.selection.as_deref()
    }

    pub fn uses_default_selection(&self) -> bool {
        self.selection.is_none()
    }
}

fn is_wikidot_space(character: char) -> bool {
    matches!(
        character,
        ' ' | '\t' | '\r' | '\n' | '\u{000b}' | '\u{000c}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_social_selection_boundaries_are_typed() {
        assert_eq!(SocialButtons::parse("").selection(), None);
        assert_eq!(
            SocialButtons::parse("reddit, facebook").selection(),
            Some(
                &[
                    SocialSelection::Service(SocialService::Reddit),
                    SocialSelection::Service(SocialService::Facebook),
                ][..]
            )
        );
        assert_eq!(
            SocialButtons::parse("reddit,reddit,facebook").selection(),
            Some(
                &[
                    SocialSelection::Service(SocialService::Reddit),
                    SocialSelection::Service(SocialService::Reddit),
                    SocialSelection::Service(SocialService::Facebook),
                ][..]
            )
        );
        assert_eq!(SocialButtons::parse("not-a-service").selection(), None);
        assert_eq!(SocialButtons::parse("foo,bar").selection(), None);
        assert_eq!(
            SocialButtons::parse("reddit,not-a-service,facebook").selection(),
            Some(
                &[
                    SocialSelection::Service(SocialService::Reddit),
                    SocialSelection::Service(SocialService::Facebook),
                ][..]
            )
        );
        assert_eq!(
            SocialButtons::parse("Reddit,FACEBOOK").selection(),
            Some(&[SocialSelection::Empty, SocialSelection::Empty][..])
        );
        assert_eq!(
            SocialButtons::parse("reddit,FACEBOOK").selection(),
            Some(
                &[
                    SocialSelection::Service(SocialService::Reddit),
                    SocialSelection::Empty,
                ][..]
            )
        );
        assert_eq!(
            SocialButtons::parse("REDDIT,facebook").selection(),
            Some(
                &[
                    SocialSelection::Empty,
                    SocialSelection::Service(SocialService::Facebook),
                ][..]
            )
        );
        assert_eq!(
            SocialButtons::parse("reddit,foo+bar").selection(),
            Some(&[SocialSelection::Service(SocialService::Reddit)][..])
        );
        assert_eq!(SocialButtons::parse("FOO").selection(), None);
        assert_eq!(SocialButtons::parse(",").selection(), None);
    }
}
