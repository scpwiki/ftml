/*
 * examples/render_html_jsonl.rs
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
use ftml::parsing::ParseError;
use ftml::render::html::HtmlRender;
use ftml::render::text::TextRender;
use ftml::render::{PageExistenceResolver, Render};
use ftml::settings::{WikitextMode, WikitextSettings};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::io::{self, BufRead, Write};

const INPUT_SCHEMA: &str = "wikijump_syntax_differential.syntax_case.v1";
const OUTPUT_SCHEMA: &str = "wikijump_syntax_differential.ftml_render_result.v1";

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum InputLayout {
    Wikidot,
    Wikijump,
}

impl InputLayout {
    fn layout(self) -> Layout {
        match self {
            Self::Wikidot => Layout::Wikidot,
            Self::Wikijump => Layout::Wikijump,
        }
    }
}

#[derive(Debug, Deserialize)]
struct SyntaxCase {
    schema: String,
    case_id: String,
    source: String,
    title: String,
    page_context: Option<PageContext>,
    layout: Option<InputLayout>,
    existing_pages: Option<Vec<String>>,
}

struct ExistingPages(std::collections::HashSet<String>);

impl PageExistenceResolver for ExistingPages {
    fn page_exists(&self, _site: &str, page: &str) -> bool {
        self.0.contains(page)
    }
}

#[derive(Debug, Deserialize)]
struct PageContext {
    site: String,
    page: String,
}

#[derive(Debug, Serialize)]
struct Engine {
    name: &'static str,
    version: String,
    package_version: &'static str,
    git_commit: Option<&'static str>,
}

impl Engine {
    fn current() -> Self {
        Self {
            name: ftml::info::PKG_NAME,
            version: ftml::info::VERSION.as_str().to_owned(),
            package_version: ftml::info::PKG_VERSION,
            git_commit: ftml::info::GIT_COMMIT_HASH,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Status {
    Rendered,
    InputError,
}

#[derive(Debug, Serialize)]
struct RenderResult {
    schema: &'static str,
    case_id: Option<String>,
    status: Status,
    html: Option<String>,
    text: Option<String>,
    parse_errors: Vec<ParseError>,
    engine: Engine,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl RenderResult {
    fn input_error(case_id: Option<String>, error: impl Into<String>) -> Self {
        Self {
            schema: OUTPUT_SCHEMA,
            case_id,
            status: Status::InputError,
            html: None,
            text: None,
            parse_errors: Vec::new(),
            engine: Engine::current(),
            error: Some(error.into()),
        }
    }
}

fn page_info(title: String, context: Option<PageContext>) -> PageInfo<'static> {
    let context = context.unwrap_or_else(|| PageContext {
        site: "syntax-differential".to_owned(),
        page: "syntax-differential".to_owned(),
    });
    PageInfo {
        page: Cow::Owned(context.page),
        category: None,
        site: Cow::Owned(context.site),
        title: Cow::Owned(title),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn render_case(case: SyntaxCase) -> RenderResult {
    if case.schema != INPUT_SCHEMA {
        return RenderResult::input_error(
            Some(case.case_id),
            format!(
                "unsupported schema {:?}; expected {INPUT_SCHEMA:?}",
                case.schema
            ),
        );
    }

    let layout = case.layout.unwrap_or(InputLayout::Wikidot).layout();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let page_info = page_info(case.title, case.page_context);
    let mut source = case.source;
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, parse_errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    let html = match &case.existing_pages {
        Some(pages) => {
            let resolver = ExistingPages(pages.iter().cloned().collect());
            HtmlRender
                .render_with_page_existence(&tree, &page_info, &settings, &resolver)
                .body
        }
        None => HtmlRender.render(&tree, &page_info, &settings).body,
    };
    let text = TextRender.render(&tree, &page_info, &settings);

    RenderResult {
        schema: OUTPUT_SCHEMA,
        case_id: Some(case.case_id),
        status: Status::Rendered,
        html: Some(html),
        text: Some(text),
        parse_errors,
        engine: Engine::current(),
        error: None,
    }
}

fn process_lines(input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    for line in input.lines() {
        let result = match line {
            Ok(line) => match serde_json::from_str(&line) {
                Ok(case) => render_case(case),
                Err(error) => RenderResult::input_error(None, error.to_string()),
            },
            Err(error) => RenderResult::input_error(None, error.to_string()),
        };

        serde_json::to_writer(&mut output, &result).map_err(io::Error::other)?;
        output.write_all(b"\n")?;
    }

    Ok(())
}

fn main() -> io::Result<()> {
    process_lines(io::stdin().lock(), io::stdout().lock())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::io::Cursor;

    #[test]
    fn processes_multiple_cases_and_emits_wikidot_html() {
        let input = concat!(
            r#"{"schema":"wikijump_syntax_differential.syntax_case.v1","case_id":"plain","source":"**Alpha**","title":"Plain"}"#,
            "\n",
            r#"{"schema":"wikijump_syntax_differential.syntax_case.v1","case_id":"collapsible","source":"[[collapsible show=\"+ Show\" hide=\"- Hide\"]]\nBody\n[[/collapsible]]","title":"Collapsible"}"#,
            "\n",
        );
        let mut output = Vec::new();

        process_lines(Cursor::new(input), &mut output).expect("process syntax cases");

        let results: Vec<Value> = String::from_utf8(output)
            .expect("JSONL is UTF-8")
            .lines()
            .map(|line| serde_json::from_str(line).expect("result is JSON"))
            .collect();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["case_id"], "plain");
        assert_eq!(results[0]["status"], "rendered");
        assert_eq!(results[0]["html"], "<p><strong>Alpha</strong></p>");
        assert_eq!(results[1]["case_id"], "collapsible");
        assert_eq!(results[1]["status"], "rendered");
        assert!(
            results[1]["html"]
                .as_str()
                .expect("HTML string")
                .contains(r#"<div class="collapsible-block">"#),
        );
    }

    #[test]
    fn optional_layout_selects_wikijump_without_changing_the_default() {
        let input = concat!(
            r#"{"schema":"wikijump_syntax_differential.syntax_case.v1","case_id":"default","source":"[[a href=\"/x\"]]X[[/a]]","title":"Default"}"#,
            "\n",
            r#"{"schema":"wikijump_syntax_differential.syntax_case.v1","case_id":"wikijump","source":"[[a href=\"/x\"]]X[[/a]]","title":"Wikijump","layout":"wikijump"}"#,
            "\n",
        );
        let mut output = Vec::new();
        process_lines(Cursor::new(input), &mut output).expect("process syntax cases");
        let results: Vec<Value> = String::from_utf8(output)
            .expect("JSONL is UTF-8")
            .lines()
            .map(|line| serde_json::from_str(line).expect("result is JSON"))
            .collect();
        assert_eq!(results[0]["html"], "<p><a href=\"/x\">X</a></p>");
        assert_eq!(
            results[1]["html"],
            "<p><a class=\"wj-anchor\" href=\"/x\">X</a></p>"
        );
    }

    #[test]
    fn existing_pages_control_the_newpage_class() {
        let input = concat!(
            r#"{"schema":"wikijump_syntax_differential.syntax_case.v1","case_id":"undeclared","source":"[[[!banana:Missing provider]]]","title":"T"}"#,
            "\n",
            r#"{"schema":"wikijump_syntax_differential.syntax_case.v1","case_id":"missing","source":"[[[!banana:Missing provider]]]","title":"T","existing_pages":[]}"#,
            "\n",
            r#"{"schema":"wikijump_syntax_differential.syntax_case.v1","case_id":"present","source":"[[[!banana:Missing provider]]]","title":"T","existing_pages":["banana:missing-provider"]}"#,
            "\n",
        );
        let mut output = Vec::new();
        process_lines(Cursor::new(input), &mut output).expect("process syntax cases");
        let results: Vec<Value> = String::from_utf8(output)
            .expect("JSONL is UTF-8")
            .lines()
            .map(|line| serde_json::from_str(line).expect("result is JSON"))
            .collect();
        assert_eq!(
            results[0]["html"],
            "<p><a href=\"/banana:missing-provider\">Missing provider</a></p>"
        );
        assert_eq!(
            results[1]["html"],
            "<p><a class=\"newpage\" href=\"/banana:missing-provider\">Missing provider</a></p>"
        );
        assert_eq!(
            results[2]["html"],
            "<p><a href=\"/banana:missing-provider\">Missing provider</a></p>"
        );
    }

    #[test]
    fn malformed_json_does_not_stop_later_cases() {
        let input = concat!(
            "{not json}\n",
            r#"{"schema":"wikijump_syntax_differential.syntax_case.v1","case_id":"later","source":"Later","title":"Later"}"#,
            "\n",
        );
        let mut output = Vec::new();

        process_lines(Cursor::new(input), &mut output).expect("process syntax cases");

        let results: Vec<Value> = String::from_utf8(output)
            .expect("JSONL is UTF-8")
            .lines()
            .map(|line| serde_json::from_str(line).expect("result is JSON"))
            .collect();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0]["status"], "input-error");
        assert!(results[0]["error"].is_string());
        assert_eq!(results[1]["case_id"], "later");
        assert_eq!(results[1]["status"], "rendered");
    }

    #[test]
    fn page_info_uses_the_case_title() {
        assert_eq!(
            page_info("Exact case title".to_owned(), None).title,
            "Exact case title"
        );
    }

    #[test]
    fn page_info_uses_the_requested_preview_context() {
        let info = page_info(
            "Preview".to_owned(),
            Some(PageContext {
                site: "sandbox-for-codex".to_owned(),
                page: String::new(),
            }),
        );
        assert_eq!(info.site, "sandbox-for-codex");
        assert_eq!(info.page, "");
    }
}
