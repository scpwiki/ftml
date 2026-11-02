/*
 * examples/corpus_profile.rs
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

use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::Render;
use ftml::render::html::HtmlRender;
use ftml::settings::{WikitextMode, WikitextSettings};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::fs::File;
use std::io::{self, BufRead, BufWriter, Write};
use std::time::{Duration, Instant};

#[derive(Deserialize)]
struct Page {
    page_id: i64,
    site: String,
    slug: String,
    wikitext: String,
}

fn main() {
    let output = std::env::args()
        .nth(1)
        .expect("usage: corpus_profile DIGESTS < pages.jsonl");
    let limit: usize = std::env::var("CORPUS_LIMIT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(usize::MAX);
    let skip: Vec<i64> = std::env::var("CORPUS_SKIP")
        .unwrap_or_default()
        .split(',')
        .filter_map(|id| id.parse().ok())
        .collect();
    let random_ids = regex::Regex::new(concat!(
        r"(wj-id-)[A-Za-z0-9]{16}|(bibcite-[0-9]+-)[0-9]+a|",
        r"(wiki-tabview-|wj-button-|wj-embed-video-|wj-gallery-|wj-social-)[0-9a-f]{32}",
    ))
    .expect("random ID pattern");
    let mut digests = BufWriter::new(File::create(output).expect("digest file"));
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut stages = [Duration::ZERO; 4];
    let mut pages = 0;
    let mut bytes = 0;

    for line in io::stdin().lock().lines().take(limit) {
        let page: Page =
            serde_json::from_str(&line.expect("input line")).expect("page JSON");
        if skip.contains(&page.page_id) {
            continue;
        }
        let (category, slug) = match page.slug.split_once(':') {
            Some((category, slug)) => {
                (Some(Cow::Owned(category.to_owned())), slug.to_owned())
            }
            None => (None, page.slug.clone()),
        };
        let page_info = PageInfo {
            page: Cow::Owned(slug),
            category,
            site: Cow::Owned(page.site.clone()),
            title: Cow::Owned(page.slug.clone()),
            alt_title: None,
            score: ScoreValue::Integer(0),
            tags: vec![],
            language: Cow::Borrowed("en"),
        };

        let mut text = page.wikitext;
        bytes += text.len();
        let started = Instant::now();
        ftml::preprocess(&mut text);
        let preprocessed = Instant::now();
        let tokens = ftml::tokenize(&text);
        let tokenized = Instant::now();
        let (tree, errors) = ftml::parse(&tokens, &page_info, &settings).into();
        let parsed = Instant::now();
        let html = HtmlRender.render(&tree, &page_info, &settings);
        let rendered = Instant::now();
        stages[0] += preprocessed - started;
        stages[1] += tokenized - preprocessed;
        stages[2] += parsed - tokenized;
        stages[3] += rendered - parsed;

        let mut hash = Sha256::new();
        hash.update(random_ids.replace_all(&html.body, "$1$2$3*").as_bytes());
        hash.update([0]);
        for style in &html.styles {
            hash.update(style.as_bytes());
            hash.update([0]);
        }
        hash.update(format!("{:?}", errors).as_bytes());
        let digest: String = hash
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let micros = (rendered - started).as_micros();
        writeln!(digests, "{}\t{digest}\t{micros}", page.page_id).expect("digest write");
        pages += 1;
    }

    let total: Duration = stages.iter().sum();
    eprintln!(
        "pages={pages} bytes={bytes} total={:.3}s preprocess={:.3}s tokenize={:.3}s parse={:.3}s render={:.3}s",
        total.as_secs_f64(),
        stages[0].as_secs_f64(),
        stages[1].as_secs_f64(),
        stages[2].as_secs_f64(),
        stages[3].as_secs_f64(),
    );
}
