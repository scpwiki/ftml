/*
 * tree/button.rs
 *
 * ftml - Library to parse Wikidot text
 * Copyright (C) 2019-2026 Wikijump Team
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

use super::clone::{option_string_to_owned, string_to_owned};
use std::borrow::Cow;

const MAX_STANDALONE_BUTTON_STYLE_BYTES: usize = 64 * 1024;

/// A standalone button whose behavior must be supplied by the renderer's caller.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub struct StandaloneButton<'t> {
    pub action: StandaloneButtonAction<'t>,
    pub label: Cow<'t, str>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<Cow<'t, str>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<Cow<'t, str>>,
}

impl StandaloneButton<'_> {
    pub fn to_owned(&self) -> StandaloneButton<'static> {
        StandaloneButton {
            action: self.action.to_owned(),
            label: string_to_owned(&self.label),
            class: option_string_to_owned(&self.class),
            style: option_string_to_owned(&self.style),
        }
    }
}

/// The closed set of actions supported by standalone buttons.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case", tag = "type", content = "data")]
pub enum StandaloneButtonAction<'t> {
    Edit,
    History,
    Source,
    Print,
    SetTags(Vec<TagAlteration<'t>>),
}

impl StandaloneButtonAction<'_> {
    pub fn to_owned(&self) -> StandaloneButtonAction<'static> {
        match self {
            Self::Edit => StandaloneButtonAction::Edit,
            Self::History => StandaloneButtonAction::History,
            Self::Source => StandaloneButtonAction::Source,
            Self::Print => StandaloneButtonAction::Print,
            Self::SetTags(alterations) => StandaloneButtonAction::SetTags(
                alterations.iter().map(TagAlteration::to_owned).collect(),
            ),
        }
    }
}

/// One ordered tag mutation from a `set-tags` button.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case", tag = "operation", content = "tag")]
pub enum TagAlteration<'t> {
    Add(Cow<'t, str>),
    Remove(Cow<'t, str>),
    ClearVisible,
    ClearHidden,
}

impl TagAlteration<'_> {
    pub fn to_owned(&self) -> TagAlteration<'static> {
        match self {
            Self::Add(tag) => TagAlteration::Add(string_to_owned(tag)),
            Self::Remove(tag) => TagAlteration::Remove(string_to_owned(tag)),
            Self::ClearVisible => TagAlteration::ClearVisible,
            Self::ClearHidden => TagAlteration::ClearHidden,
        }
    }
}

pub(crate) fn is_safe_standalone_button_style(style: &str) -> bool {
    if style.len() > MAX_STANDALONE_BUTTON_STYLE_BYTES
        || style
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '\\' | '"' | '\''))
    {
        return false;
    }
    let normalized = style.to_ascii_lowercase();
    if [
        "expression(",
        "javascript:",
        "vbscript:",
        "data:",
        "@import",
        "behavior:",
        "-moz-binding",
    ]
    .iter()
    .any(|dangerous| normalized.contains(dangerous))
    {
        return false;
    }

    let mut saw_declaration = false;
    for declaration in style
        .split(';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let Some((property, value)) = declaration.split_once(':') else {
            return declaration.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b' ')
            });
        };
        let property = property.trim();
        let value = value.trim();
        if value.is_empty() {
            return false;
        }
        let valid = if property.eq_ignore_ascii_case("background-image") {
            safe_http_background(value)
        } else if property.eq_ignore_ascii_case("background-repeat") {
            value
                .bytes()
                .all(|byte| byte.is_ascii_alphabetic() || byte == b'-')
        } else if property.eq_ignore_ascii_case("background-position")
            || property.eq_ignore_ascii_case("padding-right")
            || property.eq_ignore_ascii_case("width")
        {
            safe_numeric_css(value)
        } else if property.eq_ignore_ascii_case("color") {
            value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(
                        byte,
                        b' ' | b'#' | b'(' | b')' | b',' | b'.' | b'%' | b'+' | b'-'
                    )
            })
        } else {
            false
        };
        if !valid {
            return false;
        }
        saw_declaration = true;
    }
    saw_declaration
}

fn safe_http_background(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    (lower.starts_with("url(http://") || lower.starts_with("url(https://"))
        && lower.ends_with(')')
        && value.bytes().all(|byte| {
            byte.is_ascii_graphic() && !matches!(byte, b'"' | b'\'' | b'\\' | b';')
        })
}

fn safe_numeric_css(value: &str) -> bool {
    value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'.' | b'%' | b'+' | b'-')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_allowlist_accepts_evidence_and_rejects_script_urls() {
        assert!(is_safe_standalone_button_style("color:red"));
        assert!(is_safe_standalone_button_style("x"));
        assert!(is_safe_standalone_button_style(
            "background-image:url(http://www.scp-wiki.net/local--files/component:theme/black.png); background-repeat:no-repeat; background-position:100% 50%; padding-right:20px"
        ));
        assert!(!is_safe_standalone_button_style(
            "background:url(javascript:alert(1))"
        ));
        assert!(!is_safe_standalone_button_style(
            "color:expression(alert(1))"
        ));
        assert!(!is_safe_standalone_button_style("behavior:url(x.htc)"));
    }
}
