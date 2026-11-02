use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::Render;
use ftml::render::html::HtmlRender;
use ftml::render::text::TextRender;
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("canonical-source"),
        category: Some(Cow::Borrowed("test")),
        site: Cow::Borrowed("coverage"),
        title: Cow::Borrowed("Canonical Source"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: vec![Cow::Borrowed("test")],
        language: Cow::Borrowed("en"),
    }
}

fn render_text_and_html(input: &str) -> (String, String) {
    render_text_and_html_with_layout(input, Layout::Wikidot)
}

fn render_text_and_html_with_layout(input: &str, layout: Layout) -> (String, String) {
    render_text_and_html_with_layout_and_errors(input, layout, false)
}

fn render_text_and_html_with_layout_and_errors(
    input: &str,
    layout: Layout,
    allow_literal_marker_errors: bool,
) -> (String, String) {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut text = input.to_owned();
    ftml::preprocess_for_layout(&mut text, layout);
    let tokens = ftml::tokenize(&text);
    let result = ftml::parse(&tokens, &page_info, &settings);
    let (tree, errors) = result.into();

    assert!(
        allow_literal_marker_errors || errors.is_empty(),
        "{input:?}: {errors:?}"
    );

    let text_output = TextRender.render(&tree, &page_info, &settings);
    let html_output = HtmlRender.render(&tree, &page_info, &settings).body;
    (text_output, html_output)
}

#[test]
fn wikidot_aodaisho_anchor_body_drops_its_opening_space() {
    let (_, html) = render_text_and_html(concat!(
        "when opposing ",
        r#"[[a href="http://scp-wiki.wikidot.com/goc-hub-page"]]"#,
        " the Bookburners.[[/a]] afterward",
    ));

    assert_eq!(
        html,
        concat!(
            r#"<p>when opposing <a href="http://scp-wiki.wikidot.com/goc-hub-page">"#,
            "the Bookburners.</a> afterward</p>",
        ),
    );
}

#[test]
fn wikidot_aodaisho_footnote_drops_the_preceding_space() {
    let (_, html) = render_text_and_html(concat!(
        "Kashiasu [[footnote]]Note[[/footnote]] là một người.\n",
        "[[footnoteblock]]",
    ));

    assert!(
        html.contains("Kashiasu<sup class=\"footnoteref\">"),
        "{html}"
    );
    assert!(!html.contains("Kashiasu <sup"), "{html}");
    assert!(html.contains("</sup> là một người."), "{html}");
}

#[test]
fn wikidot_aodaisho_nested_ruby_keeps_the_size_scope_outermost() {
    let (_, html) = render_text_and_html(concat!(
        r#"[[size 175%]] **//[[span class="ruby"]]일본열도 뱀의 손"#,
        r#"[[span class="rt"]]Japanese Sarpent's Hand[[/span]][[/span]]//** [[/size]]"#,
    ));

    assert_eq!(
        html,
        concat!(
            r#"<p> <span style="font-size:175%;"><strong><em>"#,
            r#"<span class="ruby">일본열도 뱀의 손"#,
            r#"<span class="rt">Japanese Sarpent&#39;s Hand</span>"#,
            "</span></em></strong></span> </p>",
        ),
    );
}

#[test]
fn wikidot_empty_italics_delimiters_disappear_inside_list_pages_rows() {
    // Provenance-backed exact ListPages row shape:
    // vn:component:mega-cool-author-page-tool:L195:B5624. When both comment
    // variables are empty, Wikidot renders ` (////)` as plain ` ()`.
    let (_, html) = render_text_and_html("Before (////) After");

    assert_eq!(html, "<p>Before () After</p>");
}

#[test]
fn wikidot_advanced_table_cell_reopens_a_paragraph_after_a_div() {
    // Provenance-backed exact ListPages shape:
    // cn:extended-felis-proposal:L315:B8006.
    let (_, html) = render_text_and_html(concat!(
        "[[table]]\n[[row]]\n[[cell]]\n",
        "Before\n\n",
        "[[div class=\"blockquote\"]]\nInside\n[[/div]]\n\n",
        "After\n",
        "[[/cell]]\n[[/row]]\n[[/table]]",
    ));

    assert_eq!(
        html,
        concat!(
            "<table>\n<tr>\n<td>",
            r#"<p>Before</p><div class="blockquote"><p>Inside</p></div>"#,
            "<p>After</p></td>\n</tr>\n</table>",
        ),
    );
}

#[test]
fn wikidot_unsupported_inline_block_closer_recovers_as_single_bracket_link() {
    for (tag, alias, html_tag) in
        [("super", "superscript", "sup"), ("sub", "subscript", "sub")]
    {
        let input = format!(
            "Aliases: [[{tag}]]{tag}[[/{tag}]] and [[{alias}]]{alias}[[/{alias}]]"
        );
        let (_, html) =
            render_text_and_html_with_layout_and_errors(&input, Layout::Wikidot, true);

        assert_eq!(
            html,
            format!(
                "<p>Aliases: [[{tag}]]{tag}[<a href=\"/{tag}]]\">and [[{alias}</a>]{alias}[[/{alias}]]</p>"
            ),
        );

        let (_, html) = render_text_and_html_with_layout(&input, Layout::Wikijump);
        assert!(
            html.contains(&format!("<{html_tag}>{tag}</{html_tag}>")),
            "{html}"
        );
        assert!(
            html.contains(&format!("<{html_tag}>{alias}</{html_tag}>")),
            "{html}"
        );
        assert!(!html.contains(&format!("href=\"/{tag}]]\"")), "{html}");
    }
}

#[test]
fn wikidot_document_leading_indented_quote_remains_literal() {
    // Live sandbox provenance:
    // ftml-oracle-20260712T214547Z/run-quote-indentation and
    // ftml-oracle-20260712T215005Z/run-quote-document-leading-whitespace.
    let (text, html) =
        render_text_and_html("\n\t  > OMEGA_FIRST\n  > OMEGA_SECOND\nOMEGA_AFTER");

    assert!(text.contains("OMEGA_FIRST"), "{text}");
    assert!(text.contains("> OMEGA_SECOND"), "{text}");
    assert!(text.contains("OMEGA_AFTER"), "{text}");
    assert_eq!(html.matches("<blockquote>").count(), 0, "{html}");
    assert!(html.contains("&gt; OMEGA_FIRST"), "{html}");
    assert!(html.contains("&gt; OMEGA_SECOND"), "{html}");

    let (text, html) = render_text_and_html("[!-- comment --]\n  > OMEGA_AFTER_COMMENT");
    assert!(text.contains("> OMEGA_AFTER_COMMENT"), "{text}");
    assert!(!html.contains("<blockquote>"), "{html}");
}

#[test]
fn wikidot_document_leading_indented_list_requires_a_later_root_marker() {
    // The July 31 live recheck in
    // issue-scout-20260731/v7-existing-live-current supersedes the earlier
    // saved-page normalization inference: FTML keeps the indented marker
    // literal at the syntax seam. A later unindented marker starts its own
    // root list.
    for input in [
        " * saved-list-item",
        "                     * saved-list-item",
    ] {
        let (text, html) = render_text_and_html(input);
        assert_eq!(text, "* saved-list-item");
        assert_eq!(html, "<p>* saved-list-item</p>");
    }

    let (text, html) = render_text_and_html(" * orphan\n* root");
    assert_eq!(text, "* orphan\nroot");
    assert_eq!(html, "<p>* orphan</p><ul>\n<li>root</li>\n</ul>",);
}

#[test]
fn wikidot_closed_quote_prefixed_iftags_evaluate_before_native_quotes() {
    // Live sandbox provenance: run-quoted-conditionals, 2026-07-13.
    for (input, body, included) in [
        (
            "> [[iftags -codex-never]]\n> OMEGA_D1_TRUE\n> [[/iftags]]\nOMEGA_AFTER",
            "OMEGA_D1_TRUE",
            true,
        ),
        (
            "> [[iftags +codex-never]]\n> OMEGA_D1_FALSE\n> [[/iftags]]\nOMEGA_AFTER",
            "OMEGA_D1_FALSE",
            false,
        ),
        (
            ">> [[iftags +codex-never]]\n>> OMEGA_D2_FALSE\n>> [[/iftags]]\nOMEGA_AFTER",
            "OMEGA_D2_FALSE",
            false,
        ),
        (
            ">[[iftags +codex-never]]\nOMEGA_TIGHT_FALSE\n>[[/iftags]]\nOMEGA_AFTER",
            "OMEGA_TIGHT_FALSE",
            false,
        ),
        (
            ">[[iftags -codex-never]]\nOMEGA_TIGHT_TRUE\n>[[/iftags]]\nOMEGA_AFTER",
            "OMEGA_TIGHT_TRUE",
            true,
        ),
        (
            ">[[iftags]]\nOMEGA_TIGHT_NOARG\n>[[/iftags]]\nOMEGA_AFTER",
            "OMEGA_TIGHT_NOARG",
            true,
        ),
        (
            "[[iftags]]\nOMEGA_ROOT_NOARG\n[[/iftags]]\nOMEGA_AFTER",
            "OMEGA_ROOT_NOARG",
            true,
        ),
        (
            ">> [[iftags +codex-never]]\n>> OMEGA_SHALLOW_FALSE\n> [[/iftags]]\nOMEGA_AFTER",
            "OMEGA_SHALLOW_FALSE",
            false,
        ),
    ] {
        let (text, html) = render_text_and_html(input);

        assert_eq!(text.contains(body), included, "{input:?}: {text}");
        assert!(text.contains("OMEGA_AFTER"), "{input:?}: {text}");
        assert!(!html.contains("[[iftags"), "{input:?}: {html}");
        assert!(!html.contains("[[/iftags"), "{input:?}: {html}");
        if included && !input.starts_with(">[[") && input.starts_with('>') {
            assert!(html.contains("<blockquote>"), "{input:?}: {html}");
        }
    }
}

#[test]
fn wikidot_false_quoted_iftags_does_not_leave_an_empty_quote_row() {
    let (text, html) = render_text_and_html(
        "> [[iftags +codex-never]]\n> OMEGA_HIDDEN\n> [[/iftags]]\nOMEGA_AFTER",
    );

    assert_eq!(text, "OMEGA_AFTER", "{text}");
    assert!(!html.contains("<blockquote>"), "{html}");
    assert!(!html.contains("OMEGA_HIDDEN"), "{html}");
    assert!(!html.contains("[[iftags"), "{html}");
}

#[test]
fn wikidot_false_quoted_iftags_between_visible_rows_adds_no_blank_row() {
    let (text, html) = render_text_and_html(concat!(
        "> OMEGA_BEFORE\n",
        "> [[iftags +codex-never]]\n",
        "> OMEGA_HIDDEN\n",
        "> [[/iftags]]\n",
        "> OMEGA_AFTER",
    ));

    assert!(text.contains("OMEGA_BEFORE"), "{text}");
    assert!(text.contains("OMEGA_AFTER"), "{text}");
    assert!(!text.contains("OMEGA_HIDDEN"), "{text}");
    assert!(html.contains("OMEGA_BEFORE<br>\nOMEGA_AFTER"), "{html}");
    assert!(!html.contains("<br>\n<br>"), "{html}");
}

#[test]
fn wikidot_empty_native_quote_lines_do_not_create_empty_blockquotes() {
    // Live sandbox provenance:
    // ftml-oracle-20260713T112423Z/run-iftags-quoted-partial/empty-quote-controls.
    for empty_quote in [">", "> ", ">> "] {
        let input = format!("OMEGA_BEGIN\n{empty_quote}\nOMEGA_AFTER");
        let (text, html) = render_text_and_html(&input);

        assert!(text.contains("OMEGA_BEGIN"), "{empty_quote:?}: {text}");
        assert!(text.contains("OMEGA_AFTER"), "{empty_quote:?}: {text}");
        assert_eq!(
            html.matches("<blockquote>").count(),
            0,
            "{empty_quote:?}: {html}"
        );
        assert_eq!(
            html, "<p>OMEGA_BEGIN</p><p>OMEGA_AFTER</p>",
            "{empty_quote:?}"
        );
    }
}

#[test]
fn wikidot_unquoted_blank_line_starts_a_sibling_native_quote_run() {
    // Live sandbox provenance:
    // ftml-oracle-20260713T143737Z/run-include-expansion-boundary-separators/
    // direct-unquoted-blank-quote-control.
    let (text, html) = render_text_and_html("> OMEGA_QUOTE_A\n\n> OMEGA_QUOTE_B");

    assert!(text.contains("OMEGA_QUOTE_A"), "{text}");
    assert!(text.contains("OMEGA_QUOTE_B"), "{text}");
    assert_eq!(html.matches("<blockquote>").count(), 2, "{html}");
    assert_eq!(html.matches("</blockquote>").count(), 2, "{html}");
    assert!(!html.contains("OMEGA_QUOTE_A<br>OMEGA_QUOTE_B"), "{html}");
}

#[test]
fn wikidot_unquoted_blank_line_starts_a_sibling_native_list_run() {
    // Live sandbox provenance:
    // ftml-oracle-20260713T154248Z/run-include-structural-boundaries.
    let (text, html) = render_text_and_html("* OMEGA_LIST_A\n\n* OMEGA_LIST_B");

    assert!(text.contains("OMEGA_LIST_A"), "{text}");
    assert!(text.contains("OMEGA_LIST_B"), "{text}");
    assert_eq!(html.matches("<ul>").count(), 2, "{html}");
    assert_eq!(html.matches("</ul>").count(), 2, "{html}");
}

#[test]
fn repeated_sibling_native_list_runs_stay_bounded() {
    let source = (0..2_048)
        .map(|index| format!("* OMEGA_LIST_{index}\n\n"))
        .collect::<String>();
    let started = Instant::now();
    let (_, html) = render_text_and_html(&source);

    assert_eq!(html.matches("<ul>").count(), 2_048);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn wikidot_empty_native_quote_lines_preserve_surrounding_run_semantics() {
    // Live sandbox provenance:
    // ftml-oracle-20260713T112423Z/run-iftags-quoted-partial/empty-quote-run-controls.
    for (input, depth, separated_paragraphs) in [
        ("> OMEGA_A\n> \n> OMEGA_B", 1, true),
        ("> OMEGA_A\n>\n> OMEGA_B", 1, false),
        (">> OMEGA_A\n>>\n>> OMEGA_B", 2, false),
        ("> OMEGA_A\n>>\n> OMEGA_B", 1, false),
    ] {
        let (text, html) = render_text_and_html(input);

        assert!(text.contains("OMEGA_A"), "{input:?}: {text}");
        assert!(text.contains("OMEGA_B"), "{input:?}: {text}");
        assert_eq!(
            html.matches("<blockquote>").count(),
            depth,
            "{input:?}: {html}"
        );
        assert_eq!(
            html.matches("<p>").count(),
            if separated_paragraphs { 2 } else { 1 },
            "{input:?}: {html}"
        );
        assert_eq!(
            html.contains("OMEGA_A<br>\nOMEGA_B"),
            !separated_paragraphs,
            "{input:?}: {html}"
        );
    }
}

#[test]
fn wikidot_multiple_spaces_after_an_empty_quote_marker_preserve_the_blank_row() {
    // Live anonymous PagePreviewModule provenance:
    // listpages-synchronized-final-20260730/quote-trailing-space-matrix-live-20260731.jsonl.
    for empty_quote in [">  ", ">   "] {
        let input = format!("> OMEGA_A\n{empty_quote}\n> OMEGA_B");
        let (text, html) = render_text_and_html(&input);

        assert!(text.contains("OMEGA_A"), "{empty_quote:?}: {text}");
        assert!(text.contains("OMEGA_B"), "{empty_quote:?}: {text}");
        assert_eq!(
            html, "<blockquote><p>OMEGA_A<br>\n<br>\nOMEGA_B</p></blockquote>",
            "{empty_quote:?}"
        );
    }
}

#[test]
fn wikidot_quote_text_keeps_raw_urls_out_of_email_autolinking() {
    // Live anonymous PagePreviewModule provenance:
    // listpages-synchronized-final-20260730/raw-color-boundary-live-20260801-v4.jsonl,
    // case quoted-raw-url-14.
    let input = concat!(
        "> <script type=\"text/javascript\" src=\"",
        "@@http://scp-jp.wdfiles.com/local--code/example/1@@",
        "\"></script>",
    );
    let (text, html) = render_text_and_html(input);

    assert_eq!(
        text,
        concat!(
            "<script type=\"text/javascript\" src=\"",
            "http://scp-jp.wdfiles.com/local--code/example/1",
            "\"></script>",
        ),
    );
    assert_eq!(
        html,
        concat!(
            "<blockquote><p>",
            "&lt;script type=&quot;text/javascript&quot; src=&quot;",
            "<span style=\"white-space: pre-wrap;\">",
            "http://scp-jp.wdfiles.com/local--code/example/1",
            "</span>&quot;&gt;&lt;/script&gt;",
            "</p></blockquote>",
        ),
    );
    assert!(!html.contains("wiki-email"), "{html}");
    assert!(!html.contains("mailto:"), "{html}");
}

#[test]
fn wikidot_raw_space_lines_keep_both_physical_breaks_at_block_boundaries() {
    // Live anonymous PagePreviewModule provenance:
    // listpages-synchronized-final-20260730/raw-space-block-boundary-live-20260731.jsonl.
    let cases = [
        (
            "OMEGA_A\n@@ @@\nOMEGA_B",
            concat!(
                "<p>OMEGA_A<br>\n",
                "<span style=\"white-space: pre-wrap;\"> </span><br>\n",
                "OMEGA_B</p>",
            ),
        ),
        (
            "OMEGA_A\n@@ @@\n[[=]]\nOMEGA_CENTER\n[[/=]]",
            concat!(
                "<p>OMEGA_A<br>\n",
                "<span style=\"white-space: pre-wrap;\"> </span><br>\n</p>",
                "<div style=\"text-align: center;\"><p>OMEGA_CENTER</p></div>",
            ),
        ),
        (
            "[[=]]\nOMEGA_CENTER\n[[/=]]\n@@ @@\nOMEGA_B",
            concat!(
                "<div style=\"text-align: center;\"><p>OMEGA_CENTER</p></div>",
                "<br>\n<span style=\"white-space: pre-wrap;\"> </span><br>\n",
                "OMEGA_B",
            ),
        ),
    ];

    for (input, expected) in cases {
        let (_, html) = render_text_and_html(input);
        assert_eq!(html, expected, "{input:?}");
    }
}

#[test]
fn wikidot_consecutive_empty_raw_lines_before_a_heading_keep_a_paragraph() {
    // Live anonymous PagePreviewModule provenance:
    // listpages-synchronized-final-20260730/actionable-23-references-20260731.jsonl,
    // case en:scp-8658:L7:B70.
    let (_, html) =
        render_text_and_html("@@@@ @@@@\n@@@@ @@@@\n\n++* ##crimson|OMEGA_HEADING##");

    assert_eq!(
        html,
        concat!(
            "<p><br>\n</p>",
            "<h2><span><span style=\"color: crimson\">",
            "OMEGA_HEADING",
            "</span></span></h2>",
        ),
    );
}

#[test]
fn wikidot_complete_empty_raw_lines_preserve_their_physical_breaks() {
    // Live anonymous PagePreviewModule provenance:
    // listpages-synchronized-final-20260730/actionable-23-references-20260731.jsonl,
    // cases cn:scp-cn-2100:L6:B166 and jp:neko-sagashi:L17:B190.
    let cases = [
        ("@@@@\n@@@@\nOMEGA_TEXT", "<p><br>\n<br>\nOMEGA_TEXT</p>"),
        (
            "@@@@\n[[=]]\nOMEGA_CENTER\n[[/=]]",
            concat!(
                "<p><br>\n</p>",
                "<div style=\"text-align: center;\"><p>OMEGA_CENTER</p></div>",
            ),
        ),
        (
            "[[=]]\nOMEGA_CENTER\n[[/=]]\n@@@@\n@@@@\n\n-----",
            concat!(
                "<div style=\"text-align: center;\"><p>OMEGA_CENTER</p></div>",
                "<br>\n<br>\n<hr>",
            ),
        ),
    ];

    for (input, expected) in cases {
        let (_, html) = render_text_and_html(input);
        assert_eq!(html, expected, "{input:?}");
    }
}

#[test]
fn wikidot_literal_advanced_table_openers_keep_block_boundary_whitespace() {
    // Live anonymous PagePreviewModule provenance:
    // listpages-synchronized-final-20260730/actionable-23-references-20260731.jsonl,
    // case cn:most-recently-created:L125:B3128. The failed table include
    // leaves row/cell controls literal; after the first generated block,
    // Wikidot retains the next cell opener's physical newline as insignificant
    // whitespace before its block body.
    let (_, html) = render_text_and_html_with_layout_and_errors(
        concat!(
            "[[row]]\n",
            "[[cell style=\"vertical-align: top;\"]]\n",
            "[[div]]\n",
            "FIRST\n",
            "[[/div]]\n",
            "[[/cell]]\n",
            "[[/row]]\n",
            "[[row]]\n",
            "[[cell style=\"vertical-align: top;\"]]\n",
            "[[div]]\n",
            "SECOND\n",
            "[[/div]]\n",
            "[[/cell]]\n",
            "[[/row]]",
        ),
        Layout::Wikidot,
        true,
    );

    assert!(
        html.contains(concat!(
            "[[/row]]<br>\n",
            "[[row]]<br>\n",
            "[[cell style=&quot;vertical-align: top;&quot;]]\n",
            "<div><p>SECOND</p></div>",
        )),
        "{html}",
    );
}

#[test]
fn wikidot_unwrapped_line_after_alignment_uses_a_newline_at_a_paragraph_break() {
    // Live anonymous PagePreviewModule provenance:
    // listpages-synchronized-final-20260730/unwrapped-paragraph-break-live-20260731.jsonl.
    let cases = [
        (
            concat!(
                "[[=]]\nOMEGA_CENTER\n[[/=]]\n",
                "@@ @@\nOMEGA_DESCRIPTION\n\nOMEGA_HISTORY",
            ),
            concat!(
                "<div style=\"text-align: center;\"><p>OMEGA_CENTER</p></div>",
                "<br>\n<span style=\"white-space: pre-wrap;\"> </span><br>\n",
                "OMEGA_DESCRIPTION\n<p>OMEGA_HISTORY</p>",
            ),
        ),
        (
            concat!(
                "[[=]]\nOMEGA_CENTER\n[[/=]]\n",
                "OMEGA_DESCRIPTION\n\nOMEGA_HISTORY",
            ),
            concat!(
                "<div style=\"text-align: center;\"><p>OMEGA_CENTER</p></div>",
                "<br>\nOMEGA_DESCRIPTION\n<p>OMEGA_HISTORY</p>",
            ),
        ),
    ];

    for (input, expected) in cases {
        let (_, html) = render_text_and_html(input);
        assert_eq!(html, expected, "{input:?}");
    }
}

#[test]
fn wikidot_inline_line_before_a_simple_table_stays_unwrapped_with_a_break() {
    // Live anonymous PagePreviewModule provenance:
    // listpages-synchronized-final-20260730/simple-table-boundary-live-20260731.jsonl.
    let (text, html) =
        render_text_and_html("**OMEGA_LABEL:** OMEGA_TEXT\n||~ HEAD||\n||CELL||");

    assert!(text.contains("OMEGA_LABEL: OMEGA_TEXT"), "{text}");
    assert_eq!(
        html,
        concat!(
            "<strong>OMEGA_LABEL:</strong> OMEGA_TEXT<br>\n",
            "<table class=\"wiki-content-table\">\n",
            "<tr>\n<th>HEAD</th>\n</tr>\n",
            "<tr>\n<td>CELL</td>\n</tr>\n",
            "</table>",
        ),
    );
}

#[test]
fn wikidot_simple_table_cells_close_crossing_color_and_bold_markers() {
    let (_, html) = render_text_and_html(concat!(
        "||##FFE4C4|**SCPs##**||",
        "##FFE4C4|**Đánh Giá##** ||",
    ));

    assert_eq!(
        html,
        concat!(
            "<table class=\"wiki-content-table\">\n",
            "<tr>\n",
            "<td><span style=\"color: #ffe4c4\"><strong>SCPs</strong></span></td>\n",
            "<td><span style=\"color: #ffe4c4\"><strong>Đánh Giá</strong></span></td>\n",
            "</tr>\n",
            "</table>",
        ),
    );
}

#[test]
fn wikidot_simple_table_color_keeps_separator_space_outside_the_span() {
    let (_, html) = render_text_and_html("|| ⠀##green| 1## ||");

    assert_eq!(
        html,
        concat!(
            "<table class=\"wiki-content-table\">\n",
            "<tr>\n",
            "<td>⠀ <span style=\"color: green\">1</span></td>\n",
            "</tr>\n",
            "</table>",
        ),
    );
}

#[test]
fn wikidot_discarded_tight_quote_row_does_not_split_the_active_paragraph() {
    // Live sandbox provenance: discarded-tight-middle in
    // ftml-oracle-20260713T112423Z/run-iftags-quoted-partial/empty-quote-run-controls.
    let (text, html) = render_text_and_html("> OMEGA_A\n>OMEGA_DROP\n> OMEGA_B");

    assert!(text.contains("OMEGA_A"), "{text}");
    assert!(text.contains("OMEGA_B"), "{text}");
    assert!(!text.contains("OMEGA_DROP"), "{text}");
    assert_eq!(html.matches("<blockquote>").count(), 1, "{html}");
    assert_eq!(html.matches("<p>").count(), 1, "{html}");
    assert!(html.contains("OMEGA_A<br>\nOMEGA_B"), "{html}");
}

#[test]
fn wikidot_spaced_inner_iftags_preserves_the_residual_quote_marker() {
    // Live sandbox provenance: iftags-spaced-inner-false.
    let (text, html) = render_text_and_html(
        "> > [[iftags +codex-never]]\n> > OMEGA_SPACED_FALSE\n> > [[/iftags]]\nOMEGA_AFTER",
    );

    assert!(!text.contains("OMEGA_SPACED_FALSE"), "{text}");
    assert!(text.contains('>'), "{text}");
    assert!(text.contains("OMEGA_AFTER"), "{text}");
    assert_eq!(html.matches("<blockquote>").count(), 1, "{html}");
    assert_eq!(html.matches("&gt;").count(), 1, "{html}");
    assert!(html.contains("<p>&gt;</p>"), "{html}");
}

#[test]
fn wikidot_nested_tight_iftags_pair_without_crossing() {
    // ftml #792: live Wikidot pairs iftags at the text level, so the
    // outer gate's span runs to the second closer and the inner gate is
    // literal body text, its condition never evaluated. The earlier LIFO
    // expectation encoded FTML's old nesting, which #792 showed is not
    // Wikidot's behavior (sandbox probes tq2/tq3, 2026-11-02: the plain
    // shape renders the body with the inner tags literal).
    let (text, html) = render_text_and_html(concat!(
        ">[[iftags -codex-never]]\n",
        ">[[iftags +codex-never]]\n",
        "OMEGA_NESTED_TIGHT_BODY\n",
        ">[[/iftags]]\n",
        ">[[/iftags]]\n",
        "OMEGA_NESTED_TIGHT_AFTER",
    ));

    assert!(text.contains("OMEGA_NESTED_TIGHT_BODY"), "{text}");
    assert!(text.contains("OMEGA_NESTED_TIGHT_AFTER"), "{text}");
    assert!(text.contains("[[iftags +codex-never]]"), "{text}");
    assert!(html.contains("[[iftags +codex-never]]"), "{html}");
    assert!(html.contains("[[/iftags]]"), "{html}");
}

#[test]
fn wikidot_true_spaced_inner_iftags_preserves_all_three_residual_quote_markers() {
    // Live sandbox provenance: run-iftags-spaced-inner-true, 2026-07-13.
    let (text, html) = render_text_and_html(
        "> > [[iftags -codex-never]]\n> > OMEGA_SPACED_TRUE\n> > [[/iftags]]\nOMEGA_AFTER",
    );

    assert!(text.contains("OMEGA_SPACED_TRUE"), "{text}");
    assert!(text.contains("OMEGA_AFTER"), "{text}");
    assert!(!html.contains("[[iftags"), "{html}");
    assert!(!html.contains("[[/iftags"), "{html}");
    assert_eq!(html.matches("<blockquote>").count(), 1, "{html}");
    assert_eq!(html.matches("&gt;").count(), 3, "{html}");
    assert!(
        html.contains("&gt;<br>\n&gt; OMEGA_SPACED_TRUE<br>\n&gt;</p>"),
        "{html}"
    );
}

#[test]
fn wikidot_unclosed_quoted_iftags_opener_remains_literal() {
    // Live sandbox provenance: iftags-unclosed-false.
    let (text, html) = render_text_and_html_with_layout_and_errors(
        "> [[iftags +codex-never]]\n> OMEGA_UNCLOSED_BODY\nOMEGA_AFTER",
        Layout::Wikidot,
        true,
    );

    assert!(text.contains("[[iftags +codex-never]]"), "{text}");
    assert!(text.contains("OMEGA_UNCLOSED_BODY"), "{text}");
    assert!(text.contains("OMEGA_AFTER"), "{text}");
    assert!(html.contains("<blockquote>"), "{html}");
}

#[test]
fn wikidot_unclosed_tight_iftags_line_is_consumed_without_gating() {
    // Live sandbox provenance: unclosed tight opener control.
    let (text, html) = render_text_and_html(
        ">[[iftags +codex-never]]\nOMEGA_TIGHT_UNCLOSED_BODY\nOMEGA_AFTER",
    );

    assert!(!text.contains("[[iftags"), "{text}");
    assert!(text.contains("OMEGA_TIGHT_UNCLOSED_BODY"), "{text}");
    assert!(text.contains("OMEGA_AFTER"), "{text}");
    assert!(!html.contains("[[iftags"), "{html}");
}

#[test]
fn ifcategory_is_wikidot_literal_but_remains_a_wikijump_conditional() {
    // Live sandbox provenance: ifcategory unquoted/depth-one cases.
    for (condition, wikijump_includes_body) in [("test", true), ("other", false)] {
        let input = format!(
            "[[ifcategory {condition}]]\nOMEGA_CATEGORY_BODY\n[[/ifcategory]]\nOMEGA_AFTER"
        );
        let (wikidot_text, wikidot_html) =
            render_text_and_html_with_layout_and_errors(&input, Layout::Wikidot, true);
        let (wikijump_text, wikijump_html) =
            render_text_and_html_with_layout(&input, Layout::Wikijump);

        assert!(wikidot_text.contains("[[ifcategory"), "{wikidot_text}");
        assert!(wikidot_text.contains("[[/ifcategory]]"), "{wikidot_text}");
        assert!(
            wikidot_text.contains("OMEGA_CATEGORY_BODY"),
            "{wikidot_text}"
        );
        assert!(wikidot_html.contains("[[ifcategory"), "{wikidot_html}");

        assert!(!wikijump_text.contains("[[ifcategory"), "{wikijump_text}");
        assert!(!wikijump_text.contains("[[/ifcategory"), "{wikijump_text}");
        assert_eq!(
            wikijump_text.contains("OMEGA_CATEGORY_BODY"),
            wikijump_includes_body,
            "{wikijump_text}"
        );
        assert!(!wikijump_html.contains("[[ifcategory"), "{wikijump_html}");
    }
}

#[test]
fn wikidot_quote_prefixed_ifcategory_stays_literal_across_closer_depths() {
    // Live sandbox provenance: ifcategory depth-one and deeper-close cases.
    for input in [
        "> [[ifcategory test]]\n> OMEGA_CATEGORY_D1\n> [[/ifcategory]]\nOMEGA_AFTER",
        "> [[ifcategory test]]\n> OMEGA_CATEGORY_DEEP\n>> [[/ifcategory]]\nOMEGA_AFTER",
    ] {
        let (text, html) =
            render_text_and_html_with_layout_and_errors(input, Layout::Wikidot, true);

        assert!(text.contains("[[ifcategory test]]"), "{input:?}: {text}");
        assert!(text.contains("[[/ifcategory]]"), "{input:?}: {text}");
        assert!(text.contains("OMEGA_CATEGORY_"), "{input:?}: {text}");
        assert!(text.contains("OMEGA_AFTER"), "{input:?}: {text}");
        assert!(html.contains("<blockquote>"), "{input:?}: {html}");
    }
}

#[test]
fn wikidot_content_section_marker_lines_do_not_render() {
    let (text, html) = render_text_and_html("before\n====\nmiddle\n=====\nafter");

    assert!(text.contains("before"));
    assert!(text.contains("middle"));
    assert!(text.contains("after"));
    assert!(!text.contains("===="), "{text}");
    assert!(!html.contains("===="), "{html}");
}

#[test]
fn wikidot_section_marker_rule_preserves_existing_equal_and_literal_contexts() {
    let (text, html) = render_text_and_html(
        "= centered\n++ Heading\n[[code]]\n====\n[[/code]]\n@@====@@",
    );

    assert!(text.contains("centered"));
    assert!(text.contains("Heading"));
    assert!(text.contains("===="), "{text}");
    assert!(html.contains("text-align: center"), "{html}");
    assert!(html.contains("<h2"), "{html}");
    assert!(html.contains("===="), "{html}");
}

#[test]
fn wikidot_canonical_unclosed_block_markers_do_not_render_as_text() {
    let (text, html) = render_text_and_html_with_layout_and_errors(
        r#"[[iftags +test]]
[[div_ class="authorlink-wrapper"]]
Calibold"#,
        Layout::Wikidot,
        true,
    );

    assert!(text.contains("[[iftags +test]]"), "{text}");
    assert!(text.contains("Calibold"), "{text}");
    assert!(html.contains("[[iftags +test]]"), "{html}");
    assert!(html.contains("[[div_"), "{html}");
    assert!(
        !html.contains(r#"<div class="authorlink-wrapper">"#),
        "{html}"
    );
    assert_eq!(
        html,
        "<p>[[iftags +test]]<br>\n[[div_ class=&quot;authorlink-wrapper&quot;]]<br>\nCalibold</p>",
    );
}

#[test]
fn wikidot_unclosed_false_iftags_marker_is_literal() {
    let (text, html) = render_text_and_html_with_layout_and_errors(
        r#"[[iftags +theme]]
Article body survives."#,
        Layout::Wikidot,
        true,
    );

    assert!(text.contains("[[iftags +theme]]"), "{text}");
    assert!(text.contains("Article body survives."), "{text}");
    assert!(html.contains("Article body survives."), "{html}");
    assert!(html.contains("[[iftags +theme]]"), "{html}");
}

#[test]
fn wikidot_unclosed_true_iftags_marker_is_literal() {
    let (text, html) = render_text_and_html_with_layout_and_errors(
        r#"[[iftags +test]]
Article body survives."#,
        Layout::Wikidot,
        true,
    );

    assert!(text.contains("[[iftags +test]]"), "{text}");
    assert!(text.contains("Article body survives."), "{text}");
    assert!(html.contains("Article body survives."), "{html}");
    assert!(html.contains("[[iftags +test]]"), "{html}");
}

#[test]
fn wikidot_unclosed_false_ifcategory_marker_is_literal() {
    let (text, html) = render_text_and_html_with_layout_and_errors(
        r#"[[ifcategory +other]]
Article body survives."#,
        Layout::Wikidot,
        true,
    );

    assert!(text.contains("[[ifcategory +other]]"), "{text}");
    assert!(text.contains("Article body survives."), "{text}");
    assert!(html.contains("Article body survives."), "{html}");
    assert!(html.contains("[[ifcategory +other]]"), "{html}");
}

#[test]
fn wikidot_unclosed_true_ifcategory_marker_is_literal() {
    let (text, html) = render_text_and_html_with_layout_and_errors(
        r#"[[ifcategory +test]]
Article body survives."#,
        Layout::Wikidot,
        true,
    );

    assert!(text.contains("[[ifcategory +test]]"), "{text}");
    assert!(text.contains("Article body survives."), "{text}");
    assert!(html.contains("Article body survives."), "{html}");
    assert!(html.contains("[[ifcategory +test]]"), "{html}");
}

#[test]
fn wikidot_licensebox_collapsible_expanded_source_renders() {
    let (text, html) = render_text_and_html(
        r#"[[div class="licensebox"]]
[[collapsible show="‡ Licensing / Citation" hide="‡ Hide Licensing / Citation"]]
Cite this page as:
[[div class="list-pages-box"]]
[[div class="list-pages-item"]]
> "SCP-2117" by Administrator.
[[/div]]
[[/div]]
For information on licensing, see the guide.
=====
> **Filename:** 2117.png
> **Author:** Cyantreuse
> **License:** CC BY-SA 3.0
> **Source Link:** [[[http://scp-wiki.wikidot.com/scp-2117/|SCP Wiki]]]
> **Derivative of:**
> ------
> **Author:** Dr Reach
> **License:** CC BY-SA 3.0
> **Source Link:** [[[http://scp-wiki.wikidot.com/scp-2117/|SCP Wiki]]]
=====
[[/collapsible]]
[[/div]]"#,
    );

    assert!(text.contains("Filename:"), "{text}");
    assert!(html.contains(r#"<div class="collapsible-block""#), "{html}");
    assert!(!html.contains("[[collapsible"), "{html}");
    assert!(!html.contains("[[/collapsible]]"), "{html}");
}
