/*
 * render/html/random.rs
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

use rand::RngExt;
use rand::SeedableRng;
use rand::distr::{Alphanumeric, SampleString};
use rand::rngs::SmallRng;

#[derive(Debug)]
pub struct Random {
    rng: SmallRng,
}

impl Default for Random {
    #[inline]
    fn default() -> Self {
        // PagePreview draws a new bibcite suffix on every request, and the
        // frozen captures do not share a seed FTML can recompute. Seed 1
        // keeps a render stable. It is not Wikidot's suffix.
        let rng = SmallRng::seed_from_u64(1);
        Random { rng }
    }
}

impl Random {
    pub fn generate_html_id_into(&mut self, buffer: &mut String) {
        buffer.push_str("wj-id-");
        Alphanumeric.append_string(&mut self.rng, buffer, 16);
    }

    pub fn generate_html_id(&mut self) -> String {
        let mut buffer = String::new();
        self.generate_html_id_into(&mut buffer);
        buffer
    }

    pub fn generate_wikidot_bibcite_id(&mut self, index: usize) -> String {
        let suffix = self.rng.random_range(1000..100_000);
        format!("bibcite-{index}-{suffix}a")
    }

    pub fn generate_wikidot_tabview_id(&mut self) -> String {
        let mut id = String::from("wiki-tabview-");
        for _ in 0..32 {
            let digit = self.rng.random_range(0..16);
            id.push(char::from_digit(digit, 16).expect("hex digit is in range"));
        }
        id
    }

    pub fn generate_standalone_button_id(&mut self) -> String {
        let mut id = String::from("wj-button-");
        for _ in 0..32 {
            let digit = self.rng.random_range(0..16);
            id.push(char::from_digit(digit, 16).expect("hex digit is in range"));
        }
        id
    }

    pub fn generate_embed_video_id(&mut self) -> String {
        let mut id = String::from("wj-embed-video-");
        for _ in 0..32 {
            let digit = self.rng.random_range(0..16);
            id.push(char::from_digit(digit, 16).expect("hex digit is in range"));
        }
        id
    }

    pub fn generate_gallery_id(&mut self) -> String {
        let mut id = String::from("wj-gallery-");
        for _ in 0..32 {
            let digit = self.rng.random_range(0..16);
            id.push(char::from_digit(digit, 16).expect("hex digit is in range"));
        }
        id
    }

    pub fn generate_social_id(&mut self) -> String {
        let mut id = String::from("wj-social-");
        for _ in 0..32 {
            let digit = self.rng.random_range(0..16);
            id.push(char::from_digit(digit, 16).expect("hex digit is in range"));
        }
        id
    }
}

#[test]
fn html_id() {
    let mut rand = Random::default();
    let mut buffer = String::new();

    rand.generate_html_id_into(&mut buffer);
    assert_eq!(
        buffer, "wj-id-zvGvLlhGI6VEZFKj",
        "Generated HTML ID doesn't match expected",
    );

    let html_id = rand.generate_html_id();
    assert_eq!(
        html_id, "wj-id-e9pQyKaPmLnulpgn",
        "Generated HTML ID doesn't match expected",
    );

    let bibcite_id = rand.generate_wikidot_bibcite_id(2);
    assert!(
        bibcite_id.starts_with("bibcite-2-") && bibcite_id.ends_with('a'),
        "{bibcite_id}",
    );
}
