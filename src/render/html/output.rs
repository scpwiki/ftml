/*
 * render/html/output.rs
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

use super::meta::HtmlMeta;
use crate::data::Backlinks;
use crate::tree::{EmbedVideo, Gallery, SocialButtons, StandaloneButtonAction};

#[derive(Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct GalleryRequirement {
    id: String,
    gallery: Gallery<'static>,
}

impl GalleryRequirement {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn gallery(&self) -> &Gallery<'static> {
        &self.gallery
    }
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct EmbedVideoRequirement {
    id: String,
    embed_video: EmbedVideo<'static>,
}

impl EmbedVideoRequirement {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn embed_video(&self) -> &EmbedVideo<'static> {
        &self.embed_video
    }
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct StandaloneButtonRequirement {
    id: String,
    action: StandaloneButtonAction<'static>,
}

impl StandaloneButtonRequirement {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn action(&self) -> &StandaloneButtonAction<'static> {
        &self.action
    }
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WikidotTabViewRequirement {
    id: String,
}

impl WikidotTabViewRequirement {
    pub fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct SocialRequirement {
    id: String,
    social: SocialButtons,
}

impl SocialRequirement {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn social(&self) -> &SocialButtons {
        &self.social
    }
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(tag = "type", content = "requirement", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum HtmlResourceRequirement {
    EmbedVideo(EmbedVideoRequirement),
    Gallery(GalleryRequirement),
    Social(SocialRequirement),
    StandaloneButton(StandaloneButtonRequirement),
    WikidotTabView(WikidotTabViewRequirement),
}

impl HtmlResourceRequirement {
    pub(crate) fn embed_video(id: String, embed_video: &EmbedVideo<'_>) -> Self {
        assert!(
            valid_embed_video_id(&id),
            "embedvideo requirement id must be renderer-generated",
        );
        Self::EmbedVideo(EmbedVideoRequirement {
            id,
            embed_video: embed_video.to_owned(),
        })
    }

    pub(crate) fn gallery(id: String, gallery: &Gallery<'_>) -> Self {
        assert!(
            valid_gallery_id(&id),
            "gallery requirement id must be renderer-generated",
        );
        Self::Gallery(GalleryRequirement {
            id,
            gallery: gallery.to_owned(),
        })
    }

    pub(crate) fn social(id: String, social: &SocialButtons) -> Self {
        assert!(
            valid_social_id(&id),
            "social requirement id must be renderer-generated",
        );
        Self::Social(SocialRequirement {
            id,
            social: social.clone(),
        })
    }

    pub(crate) fn standalone_button(
        id: String,
        action: &StandaloneButtonAction<'_>,
    ) -> Self {
        assert!(
            valid_standalone_button_id(&id),
            "button requirement id must be renderer-generated",
        );
        Self::StandaloneButton(StandaloneButtonRequirement {
            id,
            action: action.to_owned(),
        })
    }

    pub(crate) fn wikidot_tab_view(id: String) -> Self {
        assert!(
            valid_wikidot_tab_view_id(&id),
            "tabview requirement id must be renderer-generated",
        );
        Self::WikidotTabView(WikidotTabViewRequirement { id })
    }

    pub fn wikidot_tab_view_requirement(&self) -> Option<&WikidotTabViewRequirement> {
        match self {
            Self::WikidotTabView(requirement) => Some(requirement),
            Self::EmbedVideo(_) => None,
            Self::Gallery(_) => None,
            Self::Social(_) => None,
            Self::StandaloneButton(_) => None,
        }
    }

    pub fn standalone_button_requirement(&self) -> Option<&StandaloneButtonRequirement> {
        match self {
            Self::StandaloneButton(requirement) => Some(requirement),
            Self::EmbedVideo(_) => None,
            Self::Gallery(_) => None,
            Self::Social(_) => None,
            Self::WikidotTabView(_) => None,
        }
    }

    pub fn embed_video_requirement(&self) -> Option<&EmbedVideoRequirement> {
        match self {
            Self::EmbedVideo(requirement) => Some(requirement),
            Self::Gallery(_)
            | Self::Social(_)
            | Self::StandaloneButton(_)
            | Self::WikidotTabView(_) => None,
        }
    }

    pub fn gallery_requirement(&self) -> Option<&GalleryRequirement> {
        match self {
            Self::Gallery(requirement) => Some(requirement),
            Self::EmbedVideo(_)
            | Self::Social(_)
            | Self::StandaloneButton(_)
            | Self::WikidotTabView(_) => None,
        }
    }

    pub fn social_requirement(&self) -> Option<&SocialRequirement> {
        match self {
            Self::Social(requirement) => Some(requirement),
            Self::EmbedVideo(_)
            | Self::Gallery(_)
            | Self::StandaloneButton(_)
            | Self::WikidotTabView(_) => None,
        }
    }
}

fn valid_gallery_id(id: &str) -> bool {
    let Some(suffix) = id.strip_prefix("wj-gallery-") else {
        return false;
    };
    suffix.len() == 32
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn valid_social_id(id: &str) -> bool {
    let Some(suffix) = id.strip_prefix("wj-social-") else {
        return false;
    };
    suffix.len() == 32
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn valid_embed_video_id(id: &str) -> bool {
    let Some(suffix) = id.strip_prefix("wj-embed-video-") else {
        return false;
    };
    suffix.len() == 32
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn valid_standalone_button_id(id: &str) -> bool {
    let Some(suffix) = id.strip_prefix("wj-button-") else {
        return false;
    };
    suffix.len() == 32 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_wikidot_tab_view_id(id: &str) -> bool {
    let Some(suffix) = id.strip_prefix("wiki-tabview-") else {
        return false;
    };
    suffix.len() == 32 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Serialize, Debug, Clone)]
pub struct HtmlOutput {
    pub body: String,
    pub meta: Vec<HtmlMeta>,
    pub styles: Vec<String>,
    pub resource_requirements: Vec<HtmlResourceRequirement>,
    pub backlinks: Backlinks<'static>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn social_requirement_serializes_only_typed_services_and_generated_id() {
        let id = "wj-social-0123456789abcdef0123456789abcdef";
        let social = SocialButtons::parse("reddit,not-a-service,facebook");
        let requirement = HtmlResourceRequirement::social(id.to_owned(), &social);

        assert_eq!(
            serde_json::to_value(&requirement).unwrap(),
            serde_json::json!({
                "type": "social",
                "requirement": {
                    "id": id,
                    "social": {
                        "selection": [
                            { "type": "service", "service": "reddit" },
                            { "type": "service", "service": "facebook" },
                        ],
                    },
                },
            }),
        );
        let typed = requirement.social_requirement().unwrap();
        assert_eq!(typed.id(), id);
        assert_eq!(
            typed.social().selection(),
            Some(
                &[
                    crate::tree::SocialSelection::Service(
                        crate::tree::SocialService::Reddit,
                    ),
                    crate::tree::SocialSelection::Service(
                        crate::tree::SocialService::Facebook,
                    ),
                ][..]
            )
        );
    }

    #[test]
    #[should_panic(expected = "social requirement id must be renderer-generated")]
    fn social_requirement_rejects_authored_script_data() {
        HtmlResourceRequirement::social(
            "wj-social-x');alert(1)//".to_owned(),
            &SocialButtons::parse("reddit"),
        );
    }

    #[test]
    fn gallery_requirement_serializes_typed_source_and_generated_id() {
        let id = "wj-gallery-0123456789abcdef0123456789abcdef";
        let gallery = Gallery::new_cow(
            std::borrow::Cow::Borrowed("[[gallery]]"),
            Vec::new(),
            crate::tree::GallerySelection::CurrentPageFiles,
        );
        let requirement = HtmlResourceRequirement::gallery(id.to_owned(), &gallery);

        assert_eq!(
            serde_json::to_value(&requirement).unwrap(),
            serde_json::json!({
                "type": "gallery",
                "requirement": {
                    "id": id,
                    "gallery": {
                        "source": "[[gallery]]",
                        "arguments": [],
                        "selection": { "type": "current-page-files" },
                        "source-sha256": "2e1fa80e22bbbc83b0a8ce41a4793c4b84cb339a08b38ac307f3e04e680207d7",
                    },
                },
            }),
        );
        assert_eq!(requirement.gallery_requirement().unwrap().id(), id);
    }

    #[test]
    #[should_panic(expected = "gallery requirement id must be renderer-generated")]
    fn gallery_requirement_rejects_authored_script_data() {
        let gallery = Gallery::new_cow(
            std::borrow::Cow::Borrowed("[[gallery]]"),
            Vec::new(),
            crate::tree::GallerySelection::CurrentPageFiles,
        );
        HtmlResourceRequirement::gallery(
            "wj-gallery-x');alert(1)//".to_owned(),
            &gallery,
        );
    }

    #[test]
    fn standalone_button_requirement_serializes_typed_action_and_generated_id() {
        let id = "wj-button-0123456789abcdef0123456789abcdef";
        let requirement = HtmlResourceRequirement::standalone_button(
            id.to_owned(),
            &StandaloneButtonAction::SetTags(vec![
                crate::tree::TagAlteration::ClearVisible,
                crate::tree::TagAlteration::Add(cow!("favorite")),
            ]),
        );

        assert_eq!(
            serde_json::to_value(&requirement).unwrap(),
            serde_json::json!({
                "type": "standalone-button",
                "requirement": {
                    "id": id,
                    "action": {
                        "type": "set-tags",
                        "data": [
                            { "operation": "clear-visible" },
                            { "operation": "add", "tag": "favorite" },
                        ],
                    },
                },
            }),
        );
    }

    #[test]
    #[should_panic(expected = "button requirement id must be renderer-generated")]
    fn standalone_button_requirement_rejects_authored_script_data() {
        HtmlResourceRequirement::standalone_button(
            "wj-button-x');alert(1)//".to_owned(),
            &StandaloneButtonAction::Print,
        );
    }

    #[test]
    fn tabview_requirement_serializes_only_its_typed_generated_id() {
        let id = "wiki-tabview-0123456789abcdef0123456789abcdef";
        let requirement = HtmlResourceRequirement::wikidot_tab_view(id.to_owned());

        assert_eq!(
            serde_json::to_value(&requirement).unwrap(),
            serde_json::json!({
                "type": "wikidot-tab-view",
                "requirement": { "id": id },
            }),
        );
        assert_eq!(requirement.wikidot_tab_view_requirement().unwrap().id(), id,);
    }

    #[test]
    #[should_panic(expected = "tabview requirement id must be renderer-generated")]
    fn tabview_requirement_rejects_authored_script_data() {
        HtmlResourceRequirement::wikidot_tab_view(
            "wiki-tabview-x');alert(1)//".to_owned(),
        );
    }
}
