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

use crate::parsing::{ParseError, ParseErrorKind, Parser, parse_boolean};
use crate::settings::WikitextSettings;
use crate::tree::{AttributeMap, RawModuleArgument};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::str::FromStr;

macro_rules! make_err {
    ($parser:expr) => {
        $parser.make_err(ParseErrorKind::BlockMalformedArguments)
    };
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
    bare_source_present: bool,
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
        self.bare_source_present = true;
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
        self.bare_source_present = true;
        self.bare.insert(argument_key);
    }

    #[must_use = "non-idempotent getter method"]
    pub fn get(&mut self, key: &'t str) -> Option<Cow<'t, str>> {
        let key = self.key(key);
        self.bare.remove(&key);
        self.inner.remove(&key)
    }

    #[must_use = "the normalized Wikidot class value must be used"]
    pub(crate) fn take_normalized_wikidot_class(&mut self) -> Option<Cow<'t, str>> {
        let key = ArgumentKey {
            value: Cow::Borrowed("class"),
            case_sensitive: true,
        };
        self.bare.remove(&key);
        self.inner
            .remove(&key)
            .map(|value| normalize_wikidot_html_attribute_value(&value))
    }

    pub fn get_with_bare(&mut self, key: &'t str) -> Option<(Cow<'t, str>, bool)> {
        let key = self.key(key);
        let bare = self.bare.remove(&key);
        self.inner.remove(&key).map(|value| (value, bare))
    }

    #[must_use = "non-idempotent getter method"]
    pub fn get_bool(
        &mut self,
        parser: &Parser<'_, 't>,
        key: &'t str,
    ) -> Result<Option<bool>, ParseError> {
        match self.get(key) {
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
        match self.get(key) {
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
    pub(crate) fn has_bare_source(&self) -> bool {
        self.bare_source_present
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
                let key = key.value.clone();
                let value = value.clone();

                (key, value)
            })
            .collect()
    }

    pub fn into_raw_vec(self) -> Vec<RawModuleArgument<'t>> {
        self.raw
    }

    #[inline]
    pub fn to_attribute_map(&self, settings: &WikitextSettings) -> AttributeMap<'t> {
        let mut map = self.attribute_map_from_entries(settings, |_, _| true);
        map.isolate_id(settings);
        map
    }

    pub fn to_wikidot_anchor_attribute_map(
        &self,
        settings: &WikitextSettings,
    ) -> AttributeMap<'t> {
        let mut map =
            self.attribute_map_from_entries(settings, |_, value| !value.is_empty());
        map.remove("title");
        if let Some(href) = self
            .inner
            .get(&self.key("href"))
            .filter(|value| !contains_unresolved_wikidot_variable(value))
        {
            if *href == "javascript:;" {
            } else if wikidot_anchor_href_is_verbatim(href) {
                let value = normalize_wikidot_html_attribute_value(href);
                if value.contains(' ') {
                    map.insert_wikidot_relative_href(Cow::Owned(
                        value.replace(' ', "%20"),
                    ));
                }
            } else {
                map.insert_wikidot_relative_href(normalize_wikidot_unsafe_anchor_href(
                    href,
                ));
            }
        }
        map.isolate_id(settings);
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

            if settings.layout.legacy() && contains_unresolved_wikidot_variable(value) {
                continue;
            }

            let key = key.as_str();
            if settings.layout.legacy() && key.eq_ignore_ascii_case("hidden") {
                continue;
            }
            if self.case_sensitive && key.bytes().any(|byte| byte.is_ascii_uppercase()) {
                continue;
            }

            let value = if settings.layout.legacy() {
                normalize_wikidot_html_attribute_value(value)
            } else {
                value.clone()
            };
            attributes.insert(key, value);
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
    let mut pending_dash = false;
    for character in value.chars() {
        if character == '\\' || character == '\u{00a0}' {
            continue;
        }
        if character == ':' {
            while output.ends_with('-') {
                output.pop();
            }
            output.push(':');
            pending_dash = false;
            continue;
        }
        if character.is_ascii_alphanumeric() {
            if pending_dash && !output.ends_with(':') && !output.ends_with('-') {
                output.push('-');
            }
            pending_dash = false;
            output.push(character.to_ascii_lowercase());
            continue;
        }
        pending_dash = !output.is_empty();
    }
    while output.ends_with('-') {
        output.pop();
    }
    Cow::Owned(output)
}

fn contains_unresolved_wikidot_variable(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index + 3 < bytes.len() {
        if bytes[index] != b'{' || bytes[index + 1] != b'$' {
            index += 1;
            continue;
        }

        let escaped = bytes[..index]
            .iter()
            .rev()
            .take_while(|byte| **byte == b'\\')
            .count()
            % 2
            == 1;
        if escaped {
            index += 2;
            continue;
        }

        let name_start = index + 2;
        let mut end = name_start;
        while bytes.get(end).is_some_and(u8::is_ascii_alphanumeric) {
            end += 1;
        }
        if end > name_start && bytes.get(end) == Some(&b'}') {
            return true;
        }
        index += 2;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikidot_attribute_maps_drop_only_complete_unescaped_variables() {
        let settings = WikitextSettings::from_mode(
            crate::settings::WikitextMode::Page,
            crate::layout::Layout::Wikidot,
        );
        let mut arguments = Arguments::new();
        arguments.insert("title", cow!("{$x}"));
        arguments.insert("class", cow!("prefix {$X1} suffix"));
        arguments.insert("data-malformed", cow!("{$x"));
        arguments.insert("data-escaped", cow!(r"\{$x}"));
        arguments.insert("data-dash", cow!("{$foo-bar}"));
        arguments.insert("data-underscore", cow!("{$foo_bar}"));
        arguments.insert("data-double-dollar", cow!("{$$x}"));

        let attributes = arguments.to_attribute_map(&settings);
        assert!(!attributes.get().contains_key("title"));
        assert!(!attributes.get().contains_key("class"));
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
