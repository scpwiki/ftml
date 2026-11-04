/*
 * render/messages.rs
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

use icu_locale::Locale;

type Catalog = &'static [(&'static str, &'static str)];

const ENGLISH: Catalog = &[
    ("button-copy-clipboard", "Copy to Clipboard"),
    ("collapsible-open", "+ open block"),
    ("collapsible-hide", "- hide block"),
    ("table-of-contents", "Table of Contents"),
    ("footnote", "Footnote"),
    ("footnote-cite-not-found", "Footnote item not found"),
    ("footnote-block-title", "Footnotes"),
    ("bibliography-reference", "Reference"),
    ("bibliography-block-title", "Bibliography"),
    ("bibliography-cite-not-found-pre", "Bibliography item "),
    ("bibliography-cite-not-found-post", " not found."),
    (
        "bibliography-block-not-found",
        "Bibliography block not found",
    ),
    ("image-context-bad", "No images in this context"),
    ("audio-context-bad", "No audio in this context"),
    ("video-context-bad", "No videos in this context"),
    ("user-missing-pre", ""),
    (
        "user-missing-post",
        " does not match any existing user name",
    ),
];

const JAPANESE: Catalog = &[
    ("button-copy-clipboard", "クリップボードにコピー"),
    ("collapsible-open", "+ ブロックを開く"),
    ("collapsible-hide", "- ブロックを隠す"),
    ("table-of-contents", "目次"),
    ("footnote", "脚注"),
    ("footnote-cite-not-found", "脚注項目が見つかりません"),
    ("footnote-block-title", "脚注"),
    ("bibliography-reference", "参照"),
    ("bibliography-block-title", "参考文献"),
    ("bibliography-cite-not-found-pre", "参考文献項目 "),
    ("bibliography-cite-not-found-post", " が見つかりません"),
    (
        "bibliography-block-not-found",
        "参考文献ブロックが見つかりません",
    ),
    ("image-context-bad", "このコンテキストには画像がありません"),
    ("audio-context-bad", "このコンテキストには音声がありません"),
    ("video-context-bad", "このコンテキストには動画がありません"),
    ("user-missing-pre", ""),
    ("user-missing-post", " に一致するユーザー名は存在しません"),
];

pub(super) fn get(language: &str, key: &str) -> Option<&'static str> {
    let catalog = catalog_for(language);
    lookup(catalog, key).or_else(|| lookup(ENGLISH, key))
}

fn catalog_for(language: &str) -> Catalog {
    let language = language.trim();
    let normalized;
    let language = if language.contains('_') {
        normalized = language.replace('_', "-");
        normalized.as_str()
    } else {
        language
    };
    match Locale::try_from_str(language) {
        Ok(locale) if locale.id.language.as_str() == "ja" => JAPANESE,
        _ => ENGLISH,
    }
}

fn lookup(catalog: Catalog, key: &str) -> Option<&'static str> {
    catalog
        .iter()
        .find_map(|(candidate, message)| (*candidate == key).then_some(*message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bcp47_variants_select_japanese() {
        for language in ["ja", "ja-JP", "JA-jp", "ja_JP", "ja-Latn-JP"] {
            assert_eq!(get(language, "footnote-block-title"), Some("脚注"));
        }
    }

    #[test]
    fn invalid_and_untranslated_locales_fall_back_to_english() {
        for language in ["default", "", "ja--JP", "fr-FR", "und"] {
            assert_eq!(get(language, "footnote-block-title"), Some("Footnotes"),);
        }
    }

    #[test]
    fn japanese_catalog_covers_every_english_key() {
        assert_eq!(JAPANESE.len(), ENGLISH.len());
        for (key, _) in ENGLISH {
            assert!(
                lookup(JAPANESE, key).is_some(),
                "missing Japanese key {key}"
            );
        }
    }

    #[test]
    fn unknown_keys_remain_unknown() {
        assert_eq!(get("ja-JP", "unknown-key"), None);
        assert_eq!(get("en-US", "unknown-key"), None);
    }
}
