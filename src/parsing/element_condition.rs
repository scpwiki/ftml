/*
 * parsing/element_condition.rs
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

use std::borrow::Cow;
use strum_macros::IntoStaticStr;

/// Representation of a single condition to determine element presence.
///
/// A list of these constitutes a full condition specification, and is
/// used in blocks like `[[iftags]]` and `[[ifcategory]]`.
#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq)]
pub struct ElementCondition<'t> {
    #[serde(rename = "condition")]
    pub ctype: ElementConditionType,
    pub value: Cow<'t, str>,
}

impl<'t> ElementCondition<'t> {
    fn new(ctype: ElementConditionType, value: &'t str) -> Self {
        Self {
            ctype,
            value: Cow::Borrowed(value),
        }
    }

    /// Parse out a specification.
    ///
    /// The specification is a space separated list of strings, prefixed with
    /// either `+` or `-` or nothing.
    pub fn parse(raw_spec: &'t str) -> Vec<ElementCondition<'t>> {
        let mut conditions = Vec::new();
        for spec in raw_spec.split(' ').filter(|s| !s.is_empty()) {
            let (ctype, value) = Self::split_spec(spec);
            conditions.push(Self::new(ctype, value));
        }

        conditions
    }

    fn split_spec(value: &str) -> (ElementConditionType, &str) {
        if let Some(value) = value.strip_prefix('+') {
            return (ElementConditionType::Required, value);
        }

        if let Some(value) = value.strip_prefix('-') {
            return (ElementConditionType::Prohibited, value);
        }

        (ElementConditionType::Present, value)
    }

    pub fn parse_cow(spec: Cow<'t, str>) -> Vec<ElementCondition<'t>> {
        match spec {
            Cow::Borrowed(spec) => Self::parse(spec),
            Cow::Owned(spec) => spec
                .split(' ')
                .filter(|s| !s.is_empty())
                .map(|part| {
                    let (ctype, value) = Self::split_spec(part);
                    ElementCondition {
                        ctype,
                        value: Cow::Owned(value.to_owned()),
                    }
                })
                .collect(),
        }
    }

    /// Determines if this condition is satisfied.
    ///
    /// * `ElementConditionType::Required` -- All values of this kind must be present.
    /// * `ElementConditionType::Prohibited` -- All values of this kind must be absent.
    /// * `ElementConditionType::Present` -- Some values of this kind must be present.
    ///
    /// The full logic is essentially `all(required) && any(present) && all(prohibited)`.
    pub fn check(conditions: &[ElementCondition], values: &[Cow<str>]) -> bool {
        let mut required = true;
        let mut prohibited = true;
        let mut present = false;
        let mut had_present = false;

        for condition in conditions {
            let has_value = values.contains(&condition.value);

            match condition.ctype {
                ElementConditionType::Required => required &= has_value,
                ElementConditionType::Prohibited => prohibited &= !has_value,
                ElementConditionType::Present => {
                    present |= has_value;
                    had_present = true;
                }
            }
        }

        if !had_present {
            present = true;
        }

        required && prohibited && present
    }
}

#[derive(
    Serialize, Deserialize, IntoStaticStr, Debug, Copy, Clone, Hash, PartialEq, Eq,
)]
#[serde(rename_all = "kebab-case")]
pub enum ElementConditionType {
    Required,
    Prohibited,
    Present,
}

impl ElementConditionType {
    #[inline]
    pub fn name(self) -> &'static str {
        self.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_conditions_parse_prefixes_and_ignore_empty_segments() {
        let conditions = ElementCondition::parse(" +alpha  -beta gamma ");

        assert_eq!(
            conditions,
            vec![
                ElementCondition {
                    ctype: ElementConditionType::Required,
                    value: cow!("alpha"),
                },
                ElementCondition {
                    ctype: ElementConditionType::Prohibited,
                    value: cow!("beta"),
                },
                ElementCondition {
                    ctype: ElementConditionType::Present,
                    value: cow!("gamma"),
                },
            ]
        );
        assert!(ElementCondition::check(
            &conditions,
            &[cow!("alpha"), cow!("gamma")]
        ));
        assert!(!ElementCondition::check(
            &conditions,
            &[cow!("alpha"), cow!("beta")]
        ));
    }

    #[test]
    fn element_condition_type_names_match_variant_names() {
        assert_eq!(ElementConditionType::Required.name(), "Required");
        assert_eq!(ElementConditionType::Prohibited.name(), "Prohibited");
        assert_eq!(ElementConditionType::Present.name(), "Present");
    }
}
