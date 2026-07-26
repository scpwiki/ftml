/*
 * render/html/element/style.rs
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

use super::prelude::*;
use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};

/// Prevent CSS from terminating the HTML `<style>` raw-text element.
fn escape_style_end_tags(css: &mut String) {
    const HTML_END_TAG_START: &str = "</";
    const CSS_ESCAPED_END_TAG_START: &str = r"\3c/";

    replace_all(css, HTML_END_TAG_START, CSS_ESCAPED_END_TAG_START);
}

fn replace_all(string: &mut String, pattern: &str, replacement: &str) {
    let mut offset = 0;
    while let Some(relative_start) = string[offset..].find(pattern) {
        let start = offset + relative_start;
        string.replace_range(start..start + pattern.len(), replacement);
        offset = start + replacement.len();
    }
}

pub fn render_style(ctx: &mut HtmlContext, input_css: &str) {
    let minify = ctx.settings().minify_css;

    let parser_options = ParserOptions {
        error_recovery: true,
        ..Default::default()
    };

    let print_options = PrinterOptions {
        minify,
        ..Default::default()
    };

    debug!("Parsing input CSS ({} bytes)", input_css.len());
    let stylesheet = StyleSheet::parse(input_css, parser_options)
        .expect("Produced error with recovery enabled");

    trace!("Rendering CSS into HTML (minify: {minify})");
    let output_css = match stylesheet.to_css(print_options) {
        Ok(output) => output.code,
        Err(error) => {
            error!("Problem outputting CSS from stylesheet: {error}");
            trace!("Input CSS:\n{input_css}");
            trace!("Parsed stylesheet:\n{stylesheet:#?}");
            return;
        }
    };

    let mut output_css = output_css;
    escape_style_end_tags(&mut output_css);

    ctx.html().style().inner(|ctx| {
        // SAFETY: LightningCSS parses and serializes the stylesheet removing
        //         invalid CSS constructs. `escape_style_end_tags` additionally
        //         neutralizes any `</` retained in valid CSS so raw insertion
        //         cannot terminate the surrounding HTML `<style>` element.
        ctx.push_raw_str(&output_css);
    });
}

#[cfg(test)]
mod tests {
    use super::escape_style_end_tags;

    fn escaped(css: &str) -> String {
        let mut css = css.to_owned();
        escape_style_end_tags(&mut css);
        css
    }

    #[test]
    fn escapes_any_html_end_tag_start() {
        assert_eq!(
            escaped(r#"content: "</style><script>";"#),
            r#"content: "\3c/style><script>";"#,
        );
        assert_eq!(
            escaped(r#"content: "</script>";"#),
            r#"content: "\3c/script>";"#,
        );
        assert_eq!(
            escaped(r#"content: "</ style>";"#),
            r#"content: "\3c/ style>";"#,
        );
    }

    #[test]
    fn escapes_the_style_terminator_exactly() {
        assert_eq!(
            escaped(r#"x { content: "</style"; }"#),
            r#"x { content: "\3c/style"; }"#,
        );
    }

    #[test]
    fn preserves_non_ascii_css() {
        assert_eq!(
            escaped(r#"x { content: "café </style>"; }"#),
            r#"x { content: "café \3c/style>"; }"#,
        );
    }

    #[test]
    fn leaves_css_without_html_end_tag_start_untouched() {
        let css = r#"@media (width < 600px) { x { content: "< /style>"; } }"#;
        assert_eq!(escaped(css), css);
    }
}
