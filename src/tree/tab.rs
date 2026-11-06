/*
 * tree/tab.rs
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

use super::Element;
use super::clone::{elements_to_owned, string_to_owned};
use std::borrow::Cow;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Tab<'t> {
    pub label: Cow<'t, str>,
    pub elements: Vec<Element<'t>>,
}

impl Tab<'_> {
    pub fn to_owned(&self) -> Tab<'static> {
        Tab {
            label: string_to_owned(&self.label),
            elements: elements_to_owned(&self.elements),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum TabViewItem<'t> {
    Tab(Tab<'t>),
    /// A tab that is listed in the view's nav but whose panel is rendered
    /// outside the navset (Wikidot's cross-segment tabview split). This is
    /// produced only by the Wikidot layout's quoted-tabview ejection path;
    /// the native layout never emits it.
    Remote(Tab<'t>),
    Content(Vec<Element<'t>>),
}

impl<'t> TabViewItem<'t> {
    pub fn elements(&self) -> &[Element<'t>] {
        match self {
            TabViewItem::Tab(tab) | TabViewItem::Remote(tab) => &tab.elements,
            TabViewItem::Content(elements) => elements,
        }
    }

    pub fn elements_mut(&mut self) -> &mut Vec<Element<'t>> {
        match self {
            TabViewItem::Tab(tab) | TabViewItem::Remote(tab) => &mut tab.elements,
            TabViewItem::Content(elements) => elements,
        }
    }

    pub fn to_owned(&self) -> TabViewItem<'static> {
        match self {
            TabViewItem::Tab(tab) => TabViewItem::Tab(tab.to_owned()),
            TabViewItem::Remote(tab) => TabViewItem::Remote(tab.to_owned()),
            TabViewItem::Content(elements) => {
                TabViewItem::Content(elements_to_owned(elements))
            }
        }
    }
}
