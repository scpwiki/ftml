/*
 * tree/attribute/mod.rs
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

mod safe;

use super::clone::string_to_owned;
use crate::id_prefix::isolate_ids;
use crate::parsing::parse_boolean;
use crate::settings::WikitextSettings;
use crate::url::normalize_href;
use serde::{Deserialize, Deserializer};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::fmt::{self, Debug};
use unicase::UniCase;

pub use self::safe::{
    BOOLEAN_ATTRIBUTES, SAFE_ATTRIBUTE_PREFIXES, SAFE_ATTRIBUTES, URL_ATTRIBUTES,
    is_safe_attribute,
};

#[derive(Serialize, Default, Clone)]
pub struct AttributeMap<'t> {
    #[serde(flatten)]
    inner: BTreeMap<Cow<'t, str>, Cow<'t, str>>,
    #[serde(skip)]
    order: Vec<Cow<'t, str>>,
}

impl PartialEq for AttributeMap<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl Eq for AttributeMap<'_> {}

impl<'t> AttributeMap<'t> {
    #[inline]
    pub fn new() -> Self {
        AttributeMap::default()
    }

    pub fn from_arguments(arguments: &HashMap<UniCase<&'t str>, Cow<'t, str>>) -> Self {
        let mut attributes = AttributeMap::new();

        for (key, value) in arguments {
            if is_safe_attribute(*key)
                && let Some(value) = normalize_attribute_value(*key, Cow::clone(value))
            {
                let key = key.into_inner().to_ascii_lowercase();
                attributes.inner.insert(Cow::Owned(key), value);
            }
        }

        attributes
    }

    pub fn insert(&mut self, attribute: &str, value: Cow<'t, str>) -> bool {
        if !is_safe_attribute(UniCase::ascii(attribute)) {
            return false;
        }

        let value = match normalize_attribute_value(UniCase::ascii(attribute), value) {
            Some(value) => value,
            None => return false,
        };

        let key = attribute.to_ascii_lowercase();
        self.insert_key(Cow::Owned(key), value);
        true
    }

    pub(crate) fn insert_raw(&mut self, key: Cow<'t, str>, value: Cow<'t, str>) {
        self.insert_key(key, value);
    }

    pub(crate) fn insert_wikidot_relative_href(&mut self, value: Cow<'t, str>) {
        self.insert_key(cow!("href"), value);
    }

    fn insert_key(&mut self, key: Cow<'t, str>, value: Cow<'t, str>) {
        if !self.inner.contains_key(&key) {
            self.order.push(key.clone());
        }
        self.inner.insert(key, value);
    }

    pub(crate) fn ordered_keys(&self) -> impl Iterator<Item = &Cow<'t, str>> {
        self.order
            .iter()
            .filter(|key| self.inner.contains_key(*key))
    }

    pub(crate) fn insert_wikidot_iframe_attribute(
        &mut self,
        attribute: &str,
        value: Cow<'t, str>,
    ) {
        self.inner.insert(Cow::Owned(attribute.to_owned()), value);
    }

    #[inline]
    pub fn remove(&mut self, attribute: &str) -> Option<Cow<'t, str>> {
        self.inner.remove(attribute)
    }

    #[inline]
    pub fn get(&self) -> &BTreeMap<Cow<'t, str>, Cow<'t, str>> {
        std::convert::identity(&self.inner)
    }

    pub fn isolate_id(&mut self, settings: &WikitextSettings) {
        if settings.isolate_user_ids
            && let Some(value) = self.inner.get_mut("id")
        {
            trace!("Found 'id' attribute, isolating value");
            let isolated = isolate_ids(value);
            if isolated.is_empty() {
                self.inner.remove("id");
                self.order.retain(|key| key.as_ref() != "id");
            } else {
                *value = Cow::Owned(isolated);
            }
        }
    }

    pub fn to_owned(&self) -> AttributeMap<'static> {
        let mut inner = BTreeMap::new();

        for (key, value) in self.inner.iter() {
            let key = string_to_owned(key);
            let value = string_to_owned(value);

            inner.insert(key, value);
        }

        let order = self.order.iter().map(|key| string_to_owned(key)).collect();

        AttributeMap { inner, order }
    }
}

impl<'de, 't> Deserialize<'de> for AttributeMap<'t> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let map = BTreeMap::<String, String>::deserialize(deserializer)?;
        let mut attributes = AttributeMap::new();

        for (key, value) in map {
            attributes.insert(&key, Cow::Owned(value));
        }

        Ok(attributes)
    }
}

fn normalize_attribute_value<'t>(
    attribute: UniCase<&str>,
    mut value: Cow<'t, str>,
) -> Option<Cow<'t, str>> {
    if BOOLEAN_ATTRIBUTES.contains(&attribute)
        && let Ok(boolean_value) = parse_boolean(&value)
    {
        if boolean_value {
            value = cow!("");
        } else {
            return None;
        }
    }

    if attribute == UniCase::ascii("usemap") {
        value = normalize_usemap(value);
    } else if URL_ATTRIBUTES.contains(&attribute) {
        let url = normalize_href(&value, None).into_owned();
        value = Cow::Owned(url);
    }

    Some(value)
}

fn normalize_usemap(value: Cow<'_, str>) -> Cow<'_, str> {
    if value.len() > 1 && value.starts_with('#') {
        value
    } else {
        cow!("#invalid-url")
    }
}

impl Debug for AttributeMap<'_> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.inner.fmt(f)
    }
}

impl<'t> From<BTreeMap<Cow<'t, str>, Cow<'t, str>>> for AttributeMap<'t> {
    fn from(map: BTreeMap<Cow<'t, str>, Cow<'t, str>>) -> AttributeMap<'t> {
        let mut attributes = AttributeMap::new();

        for (key, value) in map {
            attributes.insert(key.as_ref(), value);
        }

        attributes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_map_filters_and_normalizes_arguments() {
        let arguments = hashmap! {
            UniCase::ascii("CHECKED") => cow!("true"),
            UniCase::ascii("DATA-UPPER") => cow!("caps"),
            UniCase::ascii("ArIa-DeSc") => cow!("mixed"),
            UniCase::ascii("disabled") => cow!("false"),
            UniCase::ascii("href") => cow!("javascript:alert(1)"),
            UniCase::ascii("srcset") => cow!("javascript:alert(1) 1x"),
            UniCase::ascii("onclick") => cow!("alert(1)"),
        };

        let map = AttributeMap::from_arguments(&arguments);
        let attributes = map.get();

        assert_eq!(attributes.get("checked").map(Cow::as_ref), Some(""));
        assert_eq!(attributes.get("data-upper").map(Cow::as_ref), Some("caps"));
        assert_eq!(attributes.get("aria-desc").map(Cow::as_ref), Some("mixed"));
        assert_eq!(
            attributes.get("href").map(Cow::as_ref),
            Some("#invalid-url"),
        );
        assert!(!attributes.contains_key("disabled"));
        assert!(!attributes.contains_key("onclick"));
        assert!(!attributes.contains_key("srcset"));
        assert!(format!("{map:?}").contains("checked"));

        for attribute in ["background", "cite", "poster", "src", "usemap"] {
            let arguments = hashmap! {
                UniCase::ascii(attribute) => cow!("javascript:alert(1)"),
            };
            let map = AttributeMap::from_arguments(&arguments);
            assert_eq!(
                map.get().get(attribute).map(Cow::as_ref),
                Some("#invalid-url"),
                "{attribute} should be URL-normalized",
            );
        }

        for value in ["map", "#"] {
            let arguments = hashmap! {
                UniCase::ascii("usemap") => cow!(value),
            };
            let map = AttributeMap::from_arguments(&arguments);
            assert_eq!(
                map.get().get("usemap").map(Cow::as_ref),
                Some("#invalid-url"),
                "usemap should only preserve non-empty hash references",
            );
        }

        let mut inserted = AttributeMap::new();
        assert!(inserted.insert("data-value", cow!("ok")));
        assert!(inserted.insert("DATA-UPPER", cow!("caps")));
        assert!(inserted.insert("aria-label", cow!("label")));
        assert!(inserted.insert("ArIa-DeSc", cow!("mixed")));
        assert!(inserted.insert("HREF", cow!("javascript:alert(1)")));
        assert!(inserted.insert("poster", cow!("javascript:alert(1)")));
        assert!(inserted.insert("usemap", cow!("#map")));
        assert!(inserted.insert("checked", cow!("true")));
        assert!(!inserted.insert("disabled", cow!("false")));
        assert!(!inserted.insert("data-x onclick", cow!("bad")));
        assert!(!inserted.insert("aria-label\"", cow!("bad")));
        assert!(inserted.insert("data-", cow!("value")));
        assert!(!inserted.insert("srcset", cow!("javascript:alert(1) 1x")));
        assert!(!inserted.insert("onclick", cow!("alert(1)")));
        assert_eq!(
            inserted.get().get("data-value").map(Cow::as_ref),
            Some("ok")
        );
        assert_eq!(
            inserted.get().get("data-upper").map(Cow::as_ref),
            Some("caps")
        );
        assert_eq!(inserted.get().get("data-").map(Cow::as_ref), Some("value"));
        assert_eq!(
            inserted.get().get("aria-label").map(Cow::as_ref),
            Some("label")
        );
        assert_eq!(
            inserted.get().get("aria-desc").map(Cow::as_ref),
            Some("mixed")
        );
        assert_eq!(
            inserted.get().get("href").map(Cow::as_ref),
            Some("#invalid-url")
        );
        assert_eq!(
            inserted.get().get("poster").map(Cow::as_ref),
            Some("#invalid-url")
        );
        assert_eq!(inserted.get().get("usemap").map(Cow::as_ref), Some("#map"));

        let mut invalid_usemap = AttributeMap::new();
        assert!(invalid_usemap.insert("usemap", cow!("map")));
        assert_eq!(
            invalid_usemap.get().get("usemap").map(Cow::as_ref),
            Some("#invalid-url")
        );
        assert!(invalid_usemap.insert("usemap", cow!("#")));
        assert_eq!(
            invalid_usemap.get().get("usemap").map(Cow::as_ref),
            Some("#invalid-url")
        );
        assert_eq!(inserted.get().get("checked").map(Cow::as_ref), Some(""));
        assert!(!inserted.get().contains_key("disabled"));
        assert!(!inserted.get().contains_key("srcset"));
    }

    #[test]
    fn attribute_map_sanitizes_deserialized_and_raw_maps() {
        let mut raw = BTreeMap::new();
        raw.insert(cow!("onclick"), cow!("alert(1)"));
        raw.insert(cow!("HREF"), cow!("javascript:alert(1)"));
        raw.insert(cow!("data-safe"), cow!("ok"));
        raw.insert(cow!("disabled"), cow!("false"));

        let map = AttributeMap::from(raw);
        assert!(!map.get().contains_key("onclick"));
        assert!(!map.get().contains_key("disabled"));
        assert_eq!(map.get().get("href").map(Cow::as_ref), Some("#invalid-url"));
        assert_eq!(map.get().get("data-safe").map(Cow::as_ref), Some("ok"));

        let map: AttributeMap<'static> = serde_json::from_str(
            r##"{"onclick":"alert(1)","href":"javascript:alert(1)","usemap":"map","data-safe":"ok"}"##,
        )
        .expect("attribute map should deserialize");

        assert!(!map.get().contains_key("onclick"));
        assert_eq!(map.get().get("href").map(Cow::as_ref), Some("#invalid-url"));
        assert_eq!(
            map.get().get("usemap").map(Cow::as_ref),
            Some("#invalid-url"),
        );
        assert_eq!(map.get().get("data-safe").map(Cow::as_ref), Some("ok"));
    }
}
