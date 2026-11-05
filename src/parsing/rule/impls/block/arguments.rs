/*
 * parsing/rule/impls/block/arguments.rs
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

use crate::id_prefix::isolate_ids;
use crate::parsing::{ParseError, ParseErrorKind, Parser, parse_boolean};
use crate::settings::WikitextSettings;
use crate::tree::{AttributeMap, RawModuleArgument};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::str::FromStr;

const WIKIDOT_ANCHOR_ATTRIBUTE_KEYS: [&str; 6] =
    ["class", "id", "role", "style", "target", "type"];

macro_rules! make_err {
    ($parser:expr) => {
        $parser.make_err(ParseErrorKind::BlockMalformedArguments)
    };
}

fn normalize_wikidot_html_id_value<'t>(
    value: &Cow<'t, str>,
    settings: &WikitextSettings,
) -> Cow<'t, str> {
    if !(settings.layout.legacy() && settings.isolate_user_ids) {
        return normalize_wikidot_html_attribute_value(value);
    }
    let isolated = isolate_ids(value.as_ref());
    normalize_wikidot_html_attribute_value(&Cow::Owned(isolated))
}

fn normalize_wikidot_html_attribute_value<'t>(value: &Cow<'t, str>) -> Cow<'t, str> {
    let input = value.as_ref();
    let mut output = String::with_capacity(input.len());
    let mut pending_space = false;
    let mut changed = false;

    for ch in input.chars() {
        match ch {
            ' ' | '\t' | '\n' | '\r' => {
                if output.is_empty() {
                    changed = true;
                    continue;
                }
                if pending_space || ch != ' ' {
                    changed = true;
                }
                pending_space = true;
            }
            '\0'..='\u{001F}' | '\u{007F}' => {
                changed = true;
            }
            _ => {
                if pending_space {
                    output.push(' ');
                    pending_space = false;
                }
                output.push(ch);
            }
        }
    }

    if pending_space {
        changed = true;
    }

    if changed {
        Cow::Owned(output)
    } else {
        value.clone()
    }
}

#[derive(Debug, Clone)]
struct ArgumentKey<'t> {
    value: Cow<'t, str>,
    case_sensitive: bool,
}

impl<'t> ArgumentKey<'t> {
    #[inline]
    fn as_str(&self) -> &str {
        &self.value
    }
}

impl PartialEq for ArgumentKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.case_sensitive == other.case_sensitive
            && if self.case_sensitive {
                self.value == other.value
            } else {
                self.value.eq_ignore_ascii_case(other.value.as_ref())
            }
    }
}

impl Eq for ArgumentKey<'_> {}

impl Hash for ArgumentKey<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.case_sensitive.hash(state);
        self.value.len().hash(state);
        for byte in self.value.bytes() {
            state.write_u8(if self.case_sensitive {
                byte
            } else {
                byte.to_ascii_lowercase()
            });
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Arguments<'t> {
    inner: HashMap<ArgumentKey<'t>, Cow<'t, str>>,
    raw: Vec<RawModuleArgument<'t>>,
    bare: HashSet<ArgumentKey<'t>>,
    /// Wikidot head attributes whose quoted value evaluated to empty because it
    /// contained raw inline markup; live emits these as `key=""`.
    explicit_empty: HashSet<ArgumentKey<'t>>,
    /// Authored values that are only whitespace: live keeps the attribute and
    /// serializes its normalized-empty value as `key=""`.
    whitespace_empty: HashSet<ArgumentKey<'t>>,
    case_sensitive: bool,
    empty_key_present: bool,
    source_present: bool,
    spaced_equals: bool,
}

impl<'t> Arguments<'t> {
    #[inline]
    pub fn new() -> Self {
        Arguments::default()
    }

    #[inline]
    pub(crate) fn new_case_sensitive() -> Self {
        Arguments {
            case_sensitive: true,
            ..Arguments::default()
        }
    }

    #[inline]
    fn key(&self, key: &'t str) -> ArgumentKey<'t> {
        ArgumentKey {
            value: Cow::Borrowed(key),
            case_sensitive: self.case_sensitive,
        }
    }

    #[inline]
    fn key_cow(&self, key: Cow<'t, str>) -> ArgumentKey<'t> {
        ArgumentKey {
            value: key,
            case_sensitive: self.case_sensitive,
        }
    }

    pub fn insert(&mut self, key: &'t str, value: Cow<'t, str>) {
        self.source_present = true;
        let argument_key = self.key(key);
        self.bare.remove(&argument_key);
        self.raw.push(RawModuleArgument {
            name: cow!(key),
            value: value.clone(),
        });
        self.inner.insert(argument_key, value);
    }

    pub fn insert_bare(&mut self, key: &'t str, value: Cow<'t, str>) {
        self.insert(key, value);
        self.bare.insert(self.key(key));
    }

    pub fn insert_cow(&mut self, key: Cow<'t, str>, value: Cow<'t, str>) {
        self.source_present = true;
        let argument_key = self.key_cow(key);
        self.bare.remove(&argument_key);
        self.raw.push(RawModuleArgument {
            name: argument_key.value.clone(),
            value: value.clone(),
        });
        self.inner.insert(argument_key, value);
    }

    pub fn insert_bare_cow(&mut self, key: Cow<'t, str>, value: Cow<'t, str>) {
        let argument_key = self.key_cow(key.clone());
        self.insert_cow(key, value);
        self.bare.insert(argument_key);
    }

    /// Whether the quoted value of `key` evaluated to empty because it held
    /// complete raw inline markup, which live serializes as `key=""`.
    pub(crate) fn is_explicit_empty(&self, key: &str) -> bool {
        self.explicit_empty.contains(&self.key(key))
    }

    /// Take the raw-escaped placeholder text live kept for an attribute whose
    /// value evaluated to empty (the date format class exposes it).
    #[must_use = "non-idempotent getter method"]
    pub(crate) fn take_explicit_empty_value(
        &mut self,
        key: &'t str,
    ) -> Option<Cow<'t, str>> {
        let key = self.key(key);
        if !self.explicit_empty.contains(&key) {
            return None;
        }
        self.bare.remove(&key);
        self.inner.remove(&key)
    }

    #[must_use = "non-idempotent getter method"]
    pub fn get(&mut self, key: &'t str) -> Option<Cow<'t, str>> {
        let key = self.key(key);
        self.bare.remove(&key);
        let explicit_empty = self.explicit_empty.contains(&key);
        self.inner.remove(&key).map(|value| {
            if explicit_empty {
                Cow::Borrowed("")
            } else {
                value
            }
        })
    }

    #[must_use = "the normalized Wikidot class value must be used"]
    pub(crate) fn take_normalized_wikidot_class(&mut self) -> Option<Cow<'t, str>> {
        let key = ArgumentKey {
            value: Cow::Borrowed("class"),
            case_sensitive: true,
        };
        self.bare.remove(&key);
        let explicit_empty = self.explicit_empty.contains(&key);
        self.inner.remove(&key).map(|value| {
            if explicit_empty {
                Cow::Borrowed("")
            } else {
                normalize_wikidot_html_attribute_value(&value)
            }
        })
    }

    pub fn get_with_bare(&mut self, key: &'t str) -> Option<(Cow<'t, str>, bool)> {
        let key = self.key(key);
        let bare = self.bare.remove(&key);
        let explicit_empty = self.explicit_empty.contains(&key);
        self.inner.remove(&key).map(|value| {
            (
                if explicit_empty {
                    Cow::Borrowed("")
                } else {
                    value
                },
                bare,
            )
        })
    }

    #[must_use = "non-idempotent getter method"]
    pub fn get_bool(
        &mut self,
        parser: &Parser<'_, 't>,
        key: &'t str,
    ) -> Result<Option<bool>, ParseError> {
        let key = self.key(key);
        self.bare.remove(&key);
        let explicit_empty = self.explicit_empty.contains(&key);
        match self.inner.remove(&key) {
            Some(_) if explicit_empty => Ok(None),
            Some(argument) => match parse_boolean(argument) {
                Ok(value) => Ok(Some(value)),
                Err(_) => Err(make_err!(parser)),
            },
            None => Ok(None),
        }
    }

    #[must_use = "non-idempotent getter method"]
    pub fn get_value<T: FromStr>(
        &mut self,
        parser: &Parser<'_, 't>,
        key: &'t str,
    ) -> Result<Option<T>, ParseError> {
        let argument_key = self.key(key);
        self.bare.remove(&argument_key);
        let explicit_empty = self.explicit_empty.contains(&argument_key);
        match self.inner.remove(&argument_key) {
            Some(_) if explicit_empty => Ok(None),
            Some(argument) => match argument.parse() {
                Ok(value) => Ok(Some(value)),
                Err(_) => Err(make_err!(parser)),
            },
            None => Ok(None),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[inline]
    pub(crate) fn mark_source_present(&mut self) {
        self.source_present = true;
    }

    #[inline]
    pub(crate) fn has_source(&self) -> bool {
        self.source_present
    }

    #[inline]
    pub(crate) fn mark_empty_key_present(&mut self) {
        self.empty_key_present = true;
        self.source_present = true;
    }

    #[inline]
    pub(crate) fn has_empty_key(&self) -> bool {
        self.empty_key_present
    }

    #[inline]
    pub(crate) fn mark_explicit_empty(&mut self, key: Cow<'t, str>) {
        let key = self.key_cow(key);
        self.explicit_empty.insert(key);
    }

    #[inline]
    pub(crate) fn mark_whitespace_empty(&mut self, key: Cow<'t, str>) {
        let key = self.key_cow(key);
        self.whitespace_empty.insert(key);
    }

    /// Records the attributes whose Wikidot-layout value evaluated to empty in
    /// the block's attribute map, so the renderer can emit `key=""` for them.
    pub(crate) fn insert_explicit_empty_marker(&self, attributes: &mut AttributeMap<'t>) {
        let mut names: Vec<Cow<'t, str>> = Vec::new();
        let mut push_name = |name: Cow<'t, str>| {
            if !names.contains(&name) {
                names.push(name);
            }
        };

        // Keep authored order for the serialized attributes, with attributes
        // live drops from the map (an isolated empty id) at their raw position.
        for argument in &self.raw {
            let key = self.key_cow(argument.name.clone());
            let explicit = self.explicit_empty.contains(&key);
            let whitespace_only = self.whitespace_empty.contains(&key);
            if !explicit && !whitespace_only {
                continue;
            }
            if let Some((stored, _)) = attributes
                .get()
                .iter()
                .find(|(stored, _)| stored.as_ref() == key.as_str())
            {
                push_name(stored.clone());
            } else if explicit && key.as_str() == "id" {
                push_name(Cow::Borrowed("id"));
            }
        }

        if names.is_empty() {
            return;
        }

        let joined = names
            .iter()
            .map(|name| name.as_ref())
            .collect::<Vec<_>>()
            .join(" ");
        attributes.insert(
            crate::tree::WIKIDOT_EMPTY_VALUE_ATTRIBUTE_MARKER,
            Cow::Owned(joined),
        );
    }

    pub(crate) fn count_key(&self, key: &str) -> usize {
        self.raw
            .iter()
            .filter(|argument| argument.name.eq_ignore_ascii_case(key))
            .count()
    }

    pub fn mark_spaced_equals(&mut self) {
        self.spaced_equals = true;
    }

    pub fn has_spaced_equals(&self) -> bool {
        self.spaced_equals
    }

    pub fn to_hash_map(&self) -> HashMap<Cow<'t, str>, Cow<'t, str>> {
        self.inner
            .iter()
            .map(|(key, value)| {
                let explicit_empty = self.explicit_empty.contains(key);
                let key = key.value.clone();
                let value = if explicit_empty {
                    Cow::Borrowed("")
                } else {
                    value.clone()
                };

                (key, value)
            })
            .collect()
    }

    pub fn into_raw_vec(self) -> Vec<RawModuleArgument<'t>> {
        let explicit_empty = self.explicit_empty.clone();
        self.raw
            .into_iter()
            .map(|argument| {
                let explicit = explicit_empty
                    .iter()
                    .any(|key| key.value.eq_ignore_ascii_case(argument.name.as_ref()));
                if explicit {
                    RawModuleArgument {
                        name: argument.name,
                        value: Cow::Borrowed(""),
                    }
                } else {
                    argument
                }
            })
            .collect()
    }

    #[inline]
    pub fn to_attribute_map(&self, settings: &WikitextSettings) -> AttributeMap<'t> {
        let mut map = self.attribute_map_from_entries(settings, |_, _| true);
        map.isolate_id(settings);
        self.insert_explicit_empty_marker(&mut map);
        map
    }

    pub(crate) fn to_wikidot_iframe_attribute_map(
        &self,
        settings: &WikitextSettings,
    ) -> AttributeMap<'t> {
        let mut map = self.attribute_map_from_entries(settings, |_, _| true);
        if settings.layout.legacy()
            && let Some(value) = self.inner.get(&self.key("scrolling"))
        {
            let value = normalize_wikidot_html_attribute_value(value);
            map.insert_wikidot_iframe_attribute("scrolling", value);
        }
        map.isolate_id(settings);
        map
    }

    pub fn to_wikidot_anchor_attribute_map(
        &self,
        settings: &WikitextSettings,
    ) -> AttributeMap<'t> {
        let mut map = AttributeMap::new();
        let mut seen: Vec<&str> = Vec::new();
        for argument in &self.raw {
            let name = argument.name.as_ref();
            if seen.contains(&name) {
                continue;
            }
            seen.push(name);
            let argument_key = self.key_cow(argument.name.clone());
            let Some(value) = self.inner.get(&argument_key) else {
                continue;
            };
            let authored_value = value.clone();
            let explicit_empty = self.explicit_empty.contains(&argument_key);
            if name.eq_ignore_ascii_case("href") {
                continue;
            }
            if settings.layout.legacy() && name.eq_ignore_ascii_case("hidden") {
                continue;
            }
            if name.starts_with("data-") {
                let value = if explicit_empty {
                    Cow::Borrowed("")
                } else {
                    normalize_wikidot_html_attribute_value(value)
                };
                map.insert_raw(Cow::Owned(name.to_ascii_lowercase()), value);
                continue;
            }
            if self.case_sensitive && name.bytes().any(|byte| byte.is_ascii_uppercase()) {
                continue;
            }
            if !WIKIDOT_ANCHOR_ATTRIBUTE_KEYS.contains(&name) {
                continue;
            }
            let value = if explicit_empty {
                Cow::Borrowed("")
            } else if settings.layout.legacy() && name.eq_ignore_ascii_case("id") {
                normalize_wikidot_html_id_value(value, settings)
            } else {
                normalize_wikidot_html_attribute_value(value)
            };
            if value.is_empty() && !explicit_empty && authored_value.is_empty() {
                continue;
            }
            map.insert(name, value);
        }
        if let Some(href) = self.inner.get(&self.key("href")) {
            if self.explicit_empty.contains(&self.key("href"))
                && wikidot_anchor_href_is_verbatim(href)
            {
                // Live rejects a verbatim href whose value is an escaped raw
                // placeholder, leaving the attribute explicitly empty.
                map.insert_wikidot_relative_href(Cow::Borrowed(""));
            } else if wikidot_anchor_href_is_verbatim(href) {
                let value = normalize_wikidot_html_attribute_value(href);
                let spaced = value.replace(' ', "%20");
                let encoded = crate::url::percent_encode_non_ascii(&spaced);
                map.insert_wikidot_relative_href(if encoded != value {
                    Cow::Owned(encoded.into_owned())
                } else {
                    value
                });
            } else {
                map.insert_wikidot_relative_href(normalize_wikidot_unsafe_anchor_href(
                    href,
                ));
            }
        }
        map.isolate_id(settings);
        self.insert_explicit_empty_marker(&mut map);
        map
    }

    pub(crate) fn to_attribute_map_without_bare_where(
        &self,
        settings: &WikitextSettings,
        include: impl Fn(&str, &str) -> bool,
    ) -> AttributeMap<'t> {
        let mut map = self.attribute_map_from_entries(settings, |key, value| {
            !self.bare.contains(key) && include(key.as_str(), value)
        });
        map.isolate_id(settings);
        self.insert_explicit_empty_marker(&mut map);
        map
    }

    fn attribute_map_from_entries(
        &self,
        settings: &WikitextSettings,
        include: impl Fn(&ArgumentKey<'t>, &Cow<'t, str>) -> bool,
    ) -> AttributeMap<'t> {
        let mut attributes = AttributeMap::new();

        for (key, value) in &self.inner {
            if !include(key, value) {
                continue;
            }

            let explicit_empty =
                self.explicit_empty.contains(key) || self.whitespace_empty.contains(key);
            let key = key.as_str();
            if settings.layout.legacy() && key.eq_ignore_ascii_case("hidden") {
                continue;
            }
            let key = if self.case_sensitive
                && key.bytes().any(|byte| byte.is_ascii_uppercase())
            {
                if settings.layout.legacy()
                    && let Some(key) = wikidot_lowercased_data_attribute_key(key)
                {
                    key
                } else {
                    continue;
                }
            } else {
                Cow::Borrowed(key)
            };

            let value = if explicit_empty {
                Cow::Borrowed("")
            } else if settings.layout.legacy() {
                if key.eq_ignore_ascii_case("id") {
                    normalize_wikidot_html_id_value(value, settings)
                } else {
                    normalize_wikidot_html_attribute_value(value)
                }
            } else {
                value.clone()
            };
            attributes.insert(&key, value);
        }

        attributes
    }
}

fn wikidot_anchor_href_is_verbatim(value: &str) -> bool {
    value.starts_with('#')
        || value.starts_with('/')
        || [
            "http://",
            "https://",
            "ftp://",
            "mailto:",
            "gopher://",
            "news://",
            "mms://",
            "git://",
            "sftp://",
            "ssh://",
            "svn://",
            "telnet://",
            "cvs://",
        ]
        .iter()
        .any(|scheme| value.starts_with(scheme))
}

fn normalize_wikidot_unsafe_anchor_href<'t>(value: &Cow<'t, str>) -> Cow<'t, str> {
    let value = normalize_wikidot_html_attribute_value(value);
    let nbsp_padded = value.starts_with('\u{00a0}') || value.ends_with('\u{00a0}');
    let value = if nbsp_padded {
        Cow::Owned(value.trim_matches('\u{00a0}').to_owned())
    } else {
        value
    };
    let mut output = String::with_capacity(value.len());
    let mut separator = false;
    let mut separator_colon = false;
    for character in value.chars() {
        if character == '\\' || character == '\u{00a0}' {
            continue;
        }
        if character.is_ascii_alphanumeric() {
            if separator && !output.is_empty() {
                output.push(if separator_colon { ':' } else { '-' });
            }
            separator = false;
            separator_colon = false;
            output.push(character.to_ascii_lowercase());
            continue;
        }
        separator = true;
        separator_colon |= character == ':';
    }
    Cow::Owned(output)
}

fn wikidot_lowercased_data_attribute_key(key: &str) -> Option<Cow<'_, str>> {
    if key.starts_with("data-") {
        Some(Cow::Owned(key.to_ascii_lowercase()))
    } else {
        None
    }
}

pub(crate) fn retain_wikidot_html_attributes(attributes: &mut AttributeMap<'_>) {
    let rejected = attributes
        .get()
        .keys()
        .filter(|key| {
            !matches!(key.as_ref(), "class" | "id" | "style" | "role" | "type")
                && !key.starts_with("data-")
        })
        .cloned()
        .collect::<Vec<_>>();

    for key in rejected {
        attributes.remove(&key);
    }

    // The explicit-empty marker lists attributes by name; drop names whose
    // attribute did not survive the filter so the renderer never re-adds an
    // attribute live omits.
    let marker_names = attributes
        .get()
        .get(crate::tree::WIKIDOT_EMPTY_VALUE_ATTRIBUTE_MARKER)
        .map(|names| names.to_string());
    if let Some(marker_names) = marker_names {
        let kept: Vec<&str> = marker_names
            .split(' ')
            .filter(|name| {
                !name.is_empty()
                    && attributes.get().keys().any(|key| key.as_ref() == *name)
            })
            .collect();
        if kept.is_empty() {
            attributes.remove(crate::tree::WIKIDOT_EMPTY_VALUE_ATTRIBUTE_MARKER);
        } else {
            attributes.insert(
                crate::tree::WIKIDOT_EMPTY_VALUE_ATTRIBUTE_MARKER,
                Cow::Owned(kept.join(" ")),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikidot_attribute_maps_keep_literal_values_and_lowercase_data_names() {
        let settings = WikitextSettings::from_mode(
            crate::settings::WikitextMode::Page,
            crate::layout::Layout::Wikidot,
        );
        let mut arguments = Arguments::new_case_sensitive();
        arguments.insert("title", cow!("{$x}"));
        arguments.insert("class", cow!("prefix {$X1} suffix"));
        arguments.insert("data-malformed", cow!("{$x"));
        arguments.insert("data-escaped", cow!(r"\{$x}"));
        arguments.insert("data-dash", cow!("{$foo-bar}"));
        arguments.insert("data-underscore", cow!("{$foo_bar}"));
        arguments.insert("data-double-dollar", cow!("{$$x}"));
        arguments.insert("data-Mixed", cow!("lowered"));
        arguments.insert("DATA-UPPER", cow!("dropped"));
        arguments.insert("hidden", cow!("true"));

        let attributes = arguments.to_attribute_map(&settings);
        assert_eq!(attributes.get().get("title").map(Cow::as_ref), Some("{$x}"));
        assert_eq!(
            attributes.get().get("class").map(Cow::as_ref),
            Some("prefix {$X1} suffix"),
        );
        assert_eq!(
            attributes.get().get("data-malformed").map(Cow::as_ref),
            Some("{$x"),
        );
        assert_eq!(
            attributes.get().get("data-escaped").map(Cow::as_ref),
            Some(r"\{$x}"),
        );
        assert_eq!(
            attributes.get().get("data-dash").map(Cow::as_ref),
            Some("{$foo-bar}"),
        );
        assert_eq!(
            attributes.get().get("data-underscore").map(Cow::as_ref),
            Some("{$foo_bar}"),
        );
        assert_eq!(
            attributes.get().get("data-double-dollar").map(Cow::as_ref),
            Some("{$$x}"),
        );
        assert_eq!(
            attributes.get().get("data-mixed").map(Cow::as_ref),
            Some("lowered"),
        );
        assert!(!attributes.get().contains_key("data-upper"));
        assert!(!attributes.get().contains_key("hidden"));
    }
    use crate::data::PageInfo;
    use crate::layout::Layout;
    use crate::settings::{WikitextMode, WikitextSettings};

    #[test]
    fn get_bool_rejects_malformed_boolean_argument() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[collapsible folded=\"maybe\"]]");
        let parser = Parser::new(&tokenization, &page_info, &settings);
        let mut arguments = Arguments::new();
        arguments.insert("folded", cow!("maybe"));

        let error = arguments
            .get_bool(&parser, "folded")
            .expect_err("malformed boolean should fail");
        assert_eq!(error.kind(), ParseErrorKind::BlockMalformedArguments);
    }

    #[test]
    fn get_value_rejects_malformed_parseable_argument() {
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
        let tokenization = crate::tokenize("[[module PageTree depth=\"many\"]]");
        let parser = Parser::new(&tokenization, &page_info, &settings);
        let mut arguments = Arguments::new();
        arguments.insert("depth", cow!("many"));
        arguments.insert("root", cow!("start"));

        let snapshot = arguments.to_hash_map();
        assert_eq!(
            snapshot.get("root").map(|value| value.as_ref()),
            Some("start")
        );

        let error = arguments
            .get_value::<u32>(&parser, "depth")
            .expect_err("malformed integer should fail");
        assert_eq!(error.kind(), ParseErrorKind::BlockMalformedArguments);
    }
}
