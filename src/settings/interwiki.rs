/*
 * settings/interwiki.rs
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
use std::collections::HashMap;
use std::sync::LazyLock;

pub static EMPTY_INTERWIKI: LazyLock<InterwikiSettings> =
    LazyLock::new(|| InterwikiSettings {
        prefixes: hashmap! {},
    });

/// An [`InterwikiSettings`] instance that has the default prefixes.
///
/// These prefixes are:
/// - `wikipedia:path` => <https://wikipedia.org/wiki/path>
/// - `wp:path` => <https://wikipedia.org/wiki/path>
/// - `commons:path` => <https://commons.wikimedia.org/wiki/path>
/// - `google:path` => <https://google.com/search?q=path>
/// - `duckduckgo:path` => <https://duckduckgo.com/?q=path>
/// - `ddg:path` => <https://duckduckgo.com/?q=path>
/// - `dictionary:path` => <https://dictionary.com/browse/path>
/// - `thesaurus:path` => <https://thesaurus.com/browse/path>
pub static DEFAULT_INTERWIKI: LazyLock<InterwikiSettings> =
    LazyLock::new(|| InterwikiSettings {
        prefixes: hashmap! {
            cow!("wikipedia") => cow!("https://wikipedia.org/wiki/$$"),
            cow!("wp") => cow!("https://wikipedia.org/wiki/$$"),
            cow!("commons") => cow!("https://commons.wikimedia.org/wiki/$$"),
            cow!("google") => cow!("https://google.com/search?q=$$"),
            cow!("duckduckgo") => cow!("https://duckduckgo.com/?q=$$"),
            cow!("ddg") => cow!("https://duckduckgo.com/?q=$$"),
            cow!("dictionary") => cow!("https://dictionary.com/browse/$$"),
            cow!("thesaurus") => cow!("https://thesaurus.com/browse/$$"),
        },
    });

/// The interwiki table the Wikidot layout resolves against.
///
/// Live Wikidot's saved pages resolve exactly these prefixes (saved-page
/// probes on `sandbox-for-codex`, 2026-11-06, pages `muse-probe-dhiw-*`):
/// - `wikipedia:path` => <https://en.wikipedia.org/wiki/path>
/// - `google:path` => <https://www.google.com/search?q=path>
/// - `dictionary:path` => <https://dictionary.reference.com/browse/path>
///
/// Live matches the prefix without regard to ASCII case when the link has an
/// authored label, so the Wikidot layout resolves through
/// [`InterwikiSettings::build_ignore_ascii_case`] there. Prefixes live does
/// not define stay unresolved even when [`DEFAULT_INTERWIKI`] defines them.
pub static WIKIDOT_INTERWIKI: LazyLock<InterwikiSettings> =
    LazyLock::new(|| InterwikiSettings {
        prefixes: hashmap! {
            cow!("wikipedia") => cow!("https://en.wikipedia.org/wiki/$$"),
            cow!("google") => cow!("https://www.google.com/search?q=$$"),
            cow!("dictionary") => cow!("https://dictionary.reference.com/browse/$$"),
        },
    });

/// Settings that determine how to turn [`interwiki links`](http://org.wikidot.com/doc:wiki-syntax#toc21)
/// into full URLs.
#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq, Eq)]
pub struct InterwikiSettings {
    #[serde(flatten)]
    /// A map from each interwiki prefix to the interwiki URL. A '$$' in the URL indicates where the path specified in
    /// the Wikijump interwiki block should go.
    pub prefixes: HashMap<Cow<'static, str>, Cow<'static, str>>,
}

impl InterwikiSettings {
    #[inline]
    pub fn new() -> Self {
        InterwikiSettings::default()
    }

    /// Creates a full URL from an interwiki link.
    /// # Example
    /// ```
    /// # use ftml::settings::*;
    /// assert_eq!(DEFAULT_INTERWIKI.build("wikipedia:Mallard").unwrap(), "https://wikipedia.org/wiki/Mallard");
    /// ```
    ///
    /// Returns None if:
    /// - The link starts with a colon
    /// - There is no colon in the link
    /// - There is nothing after the colon
    /// - The interwiki prefix is not found
    pub fn build(&self, link: &str) -> Option<String> {
        let (prefix, path) = link.split_once(':')?;

        if prefix.is_empty() || path.is_empty() {
            return None;
        }

        self.prefixes
            .get(prefix)
            .map(|template| expand_interwiki_template(template, path))
    }

    /// Creates a full URL from an interwiki link, matching the prefix without
    /// regard to ASCII case.
    ///
    /// The Wikidot layout uses this because live resolves `[WIKIPEDIA:X label]`
    /// exactly like `[wikipedia:X label]`. Behavior is otherwise identical to
    /// [`InterwikiSettings::build`].
    pub fn build_ignore_ascii_case(&self, link: &str) -> Option<String> {
        let (prefix, path) = link.split_once(':')?;

        if prefix.is_empty() || path.is_empty() {
            return None;
        }

        let template = self.prefixes.get(prefix).or_else(|| {
            self.prefixes
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(prefix))
                .map(|(_, template)| template)
        })?;

        Some(expand_interwiki_template(template, path))
    }
}

fn expand_interwiki_template(template: &str, path: &str) -> String {
    let mut url = template.replace("$$", path);

    while let Some(idx) = url.find(' ') {
        url.replace_range(idx..idx + 1, "%20");
    }

    url
}

#[test]
fn interwiki_prefixes() {
    use ref_map::*;

    macro_rules! test {
        ($link:expr, $expected:expr $(,)?) => {{
            let actual = DEFAULT_INTERWIKI.build($link);
            let expected = $expected;

            assert_eq!(
                actual.ref_map(|s| s.as_str()),
                expected,
                "Actual interwiki result doesn't match expected",
            );
        }};
    }

    test!("my-link", None);
    test!(
        "wikipedia:Mallard",
        Some("https://wikipedia.org/wiki/Mallard"),
    );
    test!(
        "wikipedia:SCP_Foundation",
        Some("https://wikipedia.org/wiki/SCP_Foundation"),
    );
    test!(
        "wikipedia:Special:RecentChanges",
        Some("https://wikipedia.org/wiki/Special:RecentChanges"),
    );
    test!(
        "wp:SCP_Foundation",
        Some("https://wikipedia.org/wiki/SCP_Foundation"),
    );
    test!(
        "wp:it:SCP_Foundation",
        Some("https://wikipedia.org/wiki/it:SCP_Foundation"),
    );
    test!(
        "commons:File:SCP-682.jpg",
        Some("https://commons.wikimedia.org/wiki/File:SCP-682.jpg"),
    );
    test!(
        "commons:Category:SCP_Foundation",
        Some("https://commons.wikimedia.org/wiki/Category:SCP_Foundation"),
    );
    test!(
        "google:what's+my+ip",
        Some("https://google.com/search?q=what's+my+ip"),
    );
    test!(
        "duckduckgo:what's+my+ip",
        Some("https://duckduckgo.com/?q=what's+my+ip"),
    );
    test!(
        "ddg:what's+my+ip",
        Some("https://duckduckgo.com/?q=what's+my+ip"),
    );
    test!("dictionary:oak", Some("https://dictionary.com/browse/oak"));
    test!("thesaurus:oak", Some("https://thesaurus.com/browse/oak"));
    test!("banana:fruit-salad", None);
    test!(":empty", None);
    test!("no-link:", None);
}

#[test]
fn interwiki_new_has_no_prefixes_and_defaults_encode_spaces() {
    use ref_map::*;

    assert_eq!(InterwikiSettings::new().build("wp:SCP Foundation"), None);

    let actual = DEFAULT_INTERWIKI.build("wp:SCP Foundation");
    assert_eq!(
        actual.ref_map(|s| s.as_str()),
        Some("https://wikipedia.org/wiki/SCP%20Foundation"),
    );
}

#[test]
fn wikidot_interwiki_table_matches_live_saved_pages() {
    use ref_map::*;

    macro_rules! test {
        ($link:expr, $expected:expr $(,)?) => {{
            let actual = WIKIDOT_INTERWIKI.build($link);
            let expected = $expected;

            assert_eq!(
                actual.ref_map(|s| s.as_str()),
                expected,
                "Actual wikidot interwiki result doesn't match expected",
            );
        }};
    }

    test!(
        "wikipedia:Albert_Einstein",
        Some("https://en.wikipedia.org/wiki/Albert_Einstein"),
    );
    test!(
        "wikipedia:it:Albert_Einstein",
        Some("https://en.wikipedia.org/wiki/it:Albert_Einstein"),
    );
    test!(
        "google:free+wiki",
        Some("https://www.google.com/search?q=free+wiki"),
    );
    test!(
        "dictionary:wiki",
        Some("https://dictionary.reference.com/browse/wiki"),
    );
    test!("wp:SCP_Foundation", None);
    test!("w:SCP_Foundation", None);
    test!("commons:File:SCP-682.jpg", None);
    test!("thesaurus:oak", None);
    test!("duckduckgo:oak", None);
    test!("banana:fruit-salad", None);
}

#[test]
fn interwiki_prefix_matching_can_ignore_ascii_case() {
    use ref_map::*;

    let wikidot = WIKIDOT_INTERWIKI.build_ignore_ascii_case("WIKIPEDIA:Albert_Einstein");
    assert_eq!(
        wikidot.ref_map(|s| s.as_str()),
        Some("https://en.wikipedia.org/wiki/Albert_Einstein"),
    );

    let wikidot = WIKIDOT_INTERWIKI.build_ignore_ascii_case("Dictionary:wiki");
    assert_eq!(
        wikidot.ref_map(|s| s.as_str()),
        Some("https://dictionary.reference.com/browse/wiki"),
    );

    assert_eq!(DEFAULT_INTERWIKI.build("WIKIPEDIA:Mallard"), None);
    assert_eq!(
        DEFAULT_INTERWIKI
            .build_ignore_ascii_case("WIKIPEDIA:Mallard")
            .as_deref(),
        Some("https://wikipedia.org/wiki/Mallard"),
    );
    assert_eq!(
        WIKIDOT_INTERWIKI.build_ignore_ascii_case("WIKIPEDIA:"),
        None
    );
    assert_eq!(WIKIDOT_INTERWIKI.build_ignore_ascii_case("WIKIPEDIA"), None);
}
