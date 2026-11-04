/*
 * tree/module.rs
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

use super::AttributeMap;
use super::clone::{option_string_to_owned, string_to_owned};
use std::borrow::Cow;
use std::num::NonZeroU32;
use strum_macros::IntoStaticStr;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct RawModuleArgument<'t> {
    pub name: Cow<'t, str>,
    pub value: Cow<'t, str>,
}

impl RawModuleArgument<'_> {
    pub fn to_owned(&self) -> RawModuleArgument<'static> {
        RawModuleArgument {
            name: string_to_owned(&self.name),
            value: string_to_owned(&self.value),
        }
    }
}

#[derive(Serialize, Deserialize, IntoStaticStr, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case", tag = "module", content = "data")]
pub enum Module<'t> {
    /// Lists all the backlinks on the given page.
    ///
    /// If no page is listed, the backlinks are returned for the current page.
    Backlinks {
        page: Option<Cow<'t, str>>,
    },

    #[serde(rename_all = "kebab-case")]
    Categories {
        include_hidden: bool,
    },

    #[serde(rename_all = "kebab-case")]
    Join {
        button_text: Option<Cow<'t, str>>,
        attributes: AttributeMap<'t>,
    },

    /// A delayed ListPages module invocation.
    ///
    /// FTML preserves the raw arguments and body. Runtime execution belongs to
    /// the application layer, where page queries and permissions are available.
    ListPages {
        arguments: Vec<RawModuleArgument<'t>>,
        body: Cow<'t, str>,
    },

    /// A body-bearing module whose execution belongs to the application layer.
    ///
    /// FTML preserves the authored name, ordered arguments, and body without
    /// asserting that the runtime recognizes or authorizes the invocation.
    Runtime {
        name: Cow<'t, str>,
        arguments: Vec<RawModuleArgument<'t>>,
        body: Cow<'t, str>,
    },

    /// Lists the structure of pages as connected by parenthood.
    ///
    /// Shows the hierarchy of parent relationships present on the given page.
    /// If no root page is listed, the tree returned is for the current page.
    #[serde(rename_all = "kebab-case")]
    PageTree {
        root: Option<Cow<'t, str>>,
        show_root: bool,
        depth: Option<NonZeroU32>,
    },

    Rate,

    /// Redirects the browser to another page or URL.
    ///
    /// The destination is preserved verbatim. HTTP redirection, trailing-slash
    /// mapping, and the no-redirect notice belong to the application layer,
    /// which has the request URL and viewer context.
    #[serde(rename_all = "kebab-case")]
    Redirect {
        destination: Option<Cow<'t, str>>,
    },

    /// A delayed TagCloud module invocation.
    ///
    /// FTML preserves the raw arguments in source order, including duplicate
    /// names, empty values, and unknown names. Tag selection, limits, style
    /// interpolation, link building, and the legacy 3D wrapper belong to the
    /// application layer, where the site's tags are available.
    TagCloud {
        arguments: Vec<RawModuleArgument<'t>>,
    },
}

impl Module<'_> {
    #[inline]
    pub fn name(&self) -> &'static str {
        self.into()
    }

    pub fn to_owned(&self) -> Module<'static> {
        match self {
            Module::Backlinks { page } => Module::Backlinks {
                page: option_string_to_owned(page),
            },
            Module::Categories { include_hidden } => Module::Categories {
                include_hidden: *include_hidden,
            },
            Module::Join {
                button_text,
                attributes,
            } => Module::Join {
                button_text: option_string_to_owned(button_text),
                attributes: attributes.to_owned(),
            },
            Module::ListPages { arguments, body } => Module::ListPages {
                arguments: arguments.iter().map(RawModuleArgument::to_owned).collect(),
                body: string_to_owned(body),
            },
            Module::Runtime {
                name,
                arguments,
                body,
            } => Module::Runtime {
                name: string_to_owned(name),
                arguments: arguments.iter().map(RawModuleArgument::to_owned).collect(),
                body: string_to_owned(body),
            },
            Module::PageTree {
                root,
                show_root,
                depth,
            } => Module::PageTree {
                root: option_string_to_owned(root),
                show_root: *show_root,
                depth: *depth,
            },
            Module::Rate => Module::Rate,
            Module::Redirect { destination } => Module::Redirect {
                destination: option_string_to_owned(destination),
            },
            Module::TagCloud { arguments } => Module::TagCloud {
                arguments: arguments.iter().map(RawModuleArgument::to_owned).collect(),
            },
        }
    }
}
