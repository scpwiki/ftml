use crate::data::{PageInfo, PageRef};
use crate::delayed::{
    DelayedInput, GeneratedInput, GeneratedKind, GeneratedValue, InputSegment,
    ResolvedTagRef, SlotBindings, SlotId, TextOrigin, parse_delayed_list,
};
use crate::layout::Layout;
use crate::render::PageExistenceResolver;
use crate::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn page_link_input(source: &str) -> DelayedInput<'_> {
    let marker = "%%title_linked%%";
    let start = source.find(marker).expect("fixture marker");
    let end = start + marker.len();
    DelayedInput::new(
        source,
        vec![
            InputSegment::text(0..start, TextOrigin::Authored),
            InputSegment::generated(GeneratedInput {
                source_range: start..end,
                id: SlotId::new(1),
                kind: GeneratedKind::PageLink,
                occurrence: 0,
            }),
            InputSegment::text(end..source.len(), TextOrigin::Authored),
        ],
    )
    .expect("valid delayed fixture")
}

fn page_bindings() -> SlotBindings<'static> {
    page_bindings_for(
        PageRef::page_only("component:image-block"),
        "Standard Image Block",
    )
}

fn page_bindings_for(page: PageRef, label: &'static str) -> SlotBindings<'static> {
    SlotBindings::new(vec![(
        SlotId::new(1),
        GeneratedValue::PageLink {
            page,
            label: Cow::Borrowed(label),
        },
    )])
    .expect("unique bindings")
}

fn render(source: &str) -> String {
    render_with_layout(source, Layout::Wikidot)
}

fn render_with_layout(source: &str, layout: Layout) -> String {
    let input = page_link_input(source);
    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::List, layout);
    let delayed = parse_delayed_list(&input, &page_info, &settings)
        .expect("supported delayed input");
    let bound = delayed.bind(&page_bindings()).expect("matching bindings");
    bound.render_html(&page_info, &settings).body().to_owned()
}

fn render_inline_list(source: &str) -> String {
    let input = DelayedInput::new(
        source,
        vec![InputSegment::text(0..source.len(), TextOrigin::Authored)],
    )
    .expect("valid inline authored input");
    let page_info = PageInfo::dummy();
    let mut settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
    settings.list_pages_inline = true;
    let delayed = parse_delayed_list(&input, &page_info, &settings)
        .expect("supported inline delayed input");
    let bound = delayed
        .bind(&SlotBindings::empty())
        .expect("empty inline bindings");
    bound.render_html(&page_info, &settings).body().to_owned()
}

fn render_input(input: &DelayedInput<'_>) -> String {
    render_input_with_bindings(input, &page_bindings())
}

fn render_input_with_bindings(
    input: &DelayedInput<'_>,
    bindings: &SlotBindings<'_>,
) -> String {
    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
    let delayed = parse_delayed_list(input, &page_info, &settings)
        .expect("supported delayed input");
    let bound = delayed.bind(bindings).expect("matching bindings");
    bound.render_html(&page_info, &settings).body().to_owned()
}

fn render_authored(source: &str) -> String {
    let input = DelayedInput::new(
        source,
        vec![InputSegment::text(0..source.len(), TextOrigin::Authored)],
    )
    .expect("valid authored delayed fixture");
    render_input_with_bindings(&input, &SlotBindings::empty())
}

fn page_link_input_with_runtime_prefix<'t>(
    source: &'t str,
    runtime_prefix: &str,
) -> DelayedInput<'t> {
    let marker = "%%title_linked%%";
    let marker_start = source.find(marker).expect("fixture marker");
    let marker_end = marker_start + marker.len();
    DelayedInput::new(
        source,
        vec![
            InputSegment::text(0..runtime_prefix.len(), TextOrigin::RuntimeScalar),
            InputSegment::text(runtime_prefix.len()..marker_start, TextOrigin::Authored),
            InputSegment::generated(GeneratedInput {
                source_range: marker_start..marker_end,
                id: SlotId::new(1),
                kind: GeneratedKind::PageLink,
                occurrence: 0,
            }),
            InputSegment::text(marker_end..source.len(), TextOrigin::Authored),
        ],
    )
    .expect("valid runtime-prefix fixture")
}

fn page_link_input_with_runtime_suffix<'t>(
    source: &'t str,
    runtime_suffix: &str,
) -> DelayedInput<'t> {
    let marker = "%%title_linked%%";
    let marker_start = source.find(marker).expect("fixture marker");
    let marker_end = marker_start + marker.len();
    let runtime_start = source.len() - runtime_suffix.len();
    DelayedInput::new(
        source,
        vec![
            InputSegment::text(0..marker_start, TextOrigin::Authored),
            InputSegment::generated(GeneratedInput {
                source_range: marker_start..marker_end,
                id: SlotId::new(1),
                kind: GeneratedKind::PageLink,
                occurrence: 0,
            }),
            InputSegment::text(marker_end..runtime_start, TextOrigin::Authored),
            InputSegment::text(runtime_start..source.len(), TextOrigin::RuntimeScalar),
        ],
    )
    .expect("valid runtime-suffix fixture")
}

#[test]
fn generated_values_cannot_become_color_authority() {
    for source in ["##%%title_linked%%|A##", "##|%%title_linked%%|A##"] {
        let input = page_link_input(source);
        let html = render_input(&input);

        assert!(html.contains("##"), "{html}");
        assert!(html.contains("Standard Image Block"), "{html}");
        assert!(!html.contains("style=\"color:"), "{html}");
        assert!(!html.contains("background-color:"), "{html}");
    }
}

#[test]
fn runtime_scalar_cannot_create_social_resource_authority() {
    let html = render_with_runtime_scalar("[[social reddit]]", "reddit");

    assert!(html.contains("[[social reddit]]"), "{html}");
    assert!(!html.contains("wj-social"), "{html}");
}

#[test]
fn generated_page_link_cannot_become_a_social_provider() {
    let source = "[[social %%title_linked%%]]";
    let input = page_link_input(source);
    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
    let delayed = parse_delayed_list(&input, &page_info, &settings)
        .expect("generated Social-head fixture parses");
    let bound = delayed
        .bind(&page_bindings())
        .expect("generated Social-head value remains bindable");
    let sealed = bound.render_html(&page_info, &settings);

    assert!(sealed.body().contains("[[social "), "{}", sealed.body());
    assert!(
        sealed.body().contains("Standard Image Block"),
        "{}",
        sealed.body()
    );
    assert!(sealed.body().contains("]]"), "{}", sealed.body());
    assert!(sealed.resource_requirements().is_empty());
}

#[test]
fn delayed_html_blocks_remain_observable_after_binding() {
    let source = "[[html]]<strong>delayed payload</strong>[[/html]]";
    let input = DelayedInput::new(
        source,
        vec![InputSegment::text(0..source.len(), TextOrigin::Authored)],
    )
    .expect("valid authored delayed fixture");
    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
    let delayed = parse_delayed_list(&input, &page_info, &settings)
        .expect("supported delayed input");
    let bound = delayed
        .bind(&SlotBindings::empty())
        .expect("empty delayed schema should bind");
    let sealed = bound.render_html(&page_info, &settings);

    assert!(sealed.body().contains(r#"src="https://example.com/""#));
    assert_eq!(sealed.html_blocks(), ["<strong>delayed payload</strong>"],);
}

#[test]
fn inline_list_delayed_anchor_is_not_wrapped_in_a_paragraph() {
    let source = "[[a_ href=\"/component:image-block\"]]inline row[[/a]]";
    assert_eq!(
        render_inline_list(source),
        "<a href=\"/component:image-block\">inline row</a>",
    );
    assert_eq!(
        render_authored(source),
        "<p><a href=\"/component:image-block\">inline row</a></p>",
    );
}

fn render_with_runtime_scalar(source: &str, scalar: &str) -> String {
    let start = source.find(scalar).expect("runtime scalar fixture");
    let end = start + scalar.len();
    let input = DelayedInput::new(
        source,
        vec![
            InputSegment::text(0..start, TextOrigin::Authored),
            InputSegment::text(start..end, TextOrigin::RuntimeScalar),
            InputSegment::text(end..source.len(), TextOrigin::Authored),
        ],
    )
    .expect("valid runtime scalar fixture");
    render_input_with_bindings(&input, &SlotBindings::empty())
}

fn render_with_runtime_literal(source: &str, literal: &str) -> String {
    let start = source.find(literal).expect("runtime literal fixture");
    let end = start + literal.len();
    let input = DelayedInput::new(
        source,
        vec![
            InputSegment::text(0..start, TextOrigin::Authored),
            InputSegment::text(start..end, TextOrigin::RuntimeLiteral),
            InputSegment::text(end..source.len(), TextOrigin::Authored),
        ],
    )
    .expect("valid runtime literal fixture");
    render_input_with_bindings(&input, &SlotBindings::empty())
}

fn render_tag(source: &str) -> String {
    render_tag_values(source, &["component"], " ")
}

fn render_tag_values(source: &str, tags: &[&str], separator: &str) -> String {
    let marker = "%%tags_linked%%";
    let start = source.find(marker).expect("fixture marker");
    let end = start + marker.len();
    let input = DelayedInput::new(
        source,
        vec![
            InputSegment::text(0..start, TextOrigin::Authored),
            InputSegment::generated(GeneratedInput {
                source_range: start..end,
                id: SlotId::new(2),
                kind: GeneratedKind::TagLinks,
                occurrence: 0,
            }),
            InputSegment::text(end..source.len(), TextOrigin::Authored),
        ],
    )
    .expect("valid delayed fixture");
    let bindings = SlotBindings::new(vec![(
        SlotId::new(2),
        GeneratedValue::TagLinks {
            tags: Cow::Owned(
                tags.iter()
                    .map(|tag| ResolvedTagRef {
                        tag: Cow::Owned((*tag).to_owned()),
                    })
                    .collect(),
            ),
            separator: Cow::Owned(separator.to_owned()),
        },
    )])
    .expect("unique bindings");
    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
    let delayed = parse_delayed_list(&input, &page_info, &settings)
        .expect("supported delayed input");
    let bound = delayed.bind(&bindings).expect("matching bindings");
    bound.render_html(&page_info, &settings).body().to_owned()
}

#[test]
fn delayed_page_link_is_an_active_inline_leaf_without_textual_substitution() {
    assert_eq!(
        render("BEGIN|**%%title_linked%%**|END"),
        concat!(
            "<p>BEGIN|<strong>",
            "<a href=\"/component:image-block\">Standard Image Block</a>",
            "</strong>|END</p>",
        ),
    );
}

#[test]
fn delayed_binding_preserves_rejected_size_closer_source() {
    assert_eq!(
        render("[[SIZE]]before %%title_linked%% after[[/SiZe]]"),
        concat!(
            "<p>[[SIZE]]before ",
            "<a href=\"/component:image-block\">Standard Image Block</a>",
            " after[[/SiZe]]</p>",
        ),
    );
}

#[test]
fn delayed_list_collapses_spaces_around_suppressed_monospace_owners() {
    assert_eq!(
        render("%%title_linked%%|A {{0}} {{****}} {{****}} B"),
        concat!(
            "<p><a href=\"/component:image-block\">",
            "Standard Image Block</a>|A B</p>",
        ),
    );
}

#[test]
fn wikidot_delayed_list_runtime_scalar_does_not_activate_a_spaces_only_line() {
    let mut source = "ROW RUNTIME\n     \n[[=]]\nCENTER\n[[/=]]".to_owned();
    crate::preproc::whitespace::normalize_wikidot_whitespace_only_lines(&mut source);
    assert_eq!(
        render_with_runtime_scalar(&source, "RUNTIME"),
        concat!(
            "<p>ROW RUNTIME</p>",
            "<div style=\"text-align: center;\"><p>CENTER</p></div>",
        ),
    );
}

#[test]
fn wikidot_delayed_list_quote_alignment_marker_is_not_visible_text() {
    let expected = concat!(
        "<blockquote><p style=\"text-align: center;\">",
        "<span style=\"color: brown\"><strong>DATE</strong></span>",
        "</p></blockquote>",
    );
    assert_eq!(
        render_with_runtime_scalar("> = ##brown|**DATE**##", "DATE"),
        expected,
    );
    assert_eq!(render_authored("> = ##brown|**DATE**##"), expected);
    let wrapped = render_authored(concat!(
        "WIKIJUMPWIKIDOTCOMPATHTML00000000000000000000000000000000I0X\n\n",
        "> = ##brown|**DATE**##\n\n",
        "WIKIJUMPWIKIDOTCOMPATHTML00000000000000000000000000000000I1X",
    ));
    assert!(
        wrapped.contains(expected),
        "generated container boundaries must not alter quote alignment: {wrapped}",
    );
}

#[test]
fn runtime_literal_preserves_owner_recovery_without_disabling_link_grammar() {
    let literal = "%%unknown%%";

    assert_eq!(
        render_with_runtime_literal(
            "BEGIN|[[code]]\n%%unknown%%\n[[/code]]|END",
            literal,
        ),
        "<p>BEGIN|[[code]]<br>\n%%unknown%%<br>\n[[/code]]|END</p>",
    );
    assert_eq!(
        render_with_runtime_literal(
            "BEGIN|[[div class=\"%%unknown%%\"]]X[[/div]]|END",
            literal,
        ),
        "<p>BEGIN|[[div class=&quot;%%unknown%%&quot;]]X[[/div]]|END</p>",
    );
    assert_eq!(
        render_with_runtime_literal("BEGIN|[[[scp-003|%%unknown%%]]]|END", literal),
        "<p>BEGIN|<a href=\"/scp-003\">%%unknown%%</a>|END</p>",
    );
    assert_eq!(
        render_with_runtime_literal("BEGIN|[[[%%unknown%%|LABEL]]]|END", literal),
        "<p>BEGIN|<a href=\"/unknown\">LABEL</a>|END</p>",
    );
}

#[test]
fn delayed_runtime_literal_links_use_page_existence_resolver() {
    struct FixtureExistence;

    impl PageExistenceResolver for FixtureExistence {
        fn page_exists(&self, _site: &str, page: &str) -> bool {
            page != "unknown"
        }
    }

    let source = "BEGIN|[[[%%unknown%%|LABEL]]]|END";
    let literal = "%%unknown%%";
    let start = source.find(literal).expect("runtime literal fixture");
    let end = start + literal.len();
    let input = DelayedInput::new(
        source,
        vec![
            InputSegment::text(0..start, TextOrigin::Authored),
            InputSegment::text(start..end, TextOrigin::RuntimeLiteral),
            InputSegment::text(end..source.len(), TextOrigin::Authored),
        ],
    )
    .expect("valid runtime literal fixture");
    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
    let delayed = parse_delayed_list(&input, &page_info, &settings)
        .expect("supported delayed input");
    let bound = delayed
        .bind(&SlotBindings::empty())
        .expect("empty bindings");

    assert_eq!(
        bound
            .render_html_with_page_existence(&page_info, &settings, &FixtureExistence,)
            .body(),
        "<p>BEGIN|<a class=\"newpage\" href=\"/unknown\">LABEL</a>|END</p>",
    );
}

#[test]
fn runtime_scalar_still_allows_authored_code_ownership() {
    assert_eq!(
        render_with_runtime_scalar("BEGIN|[[code]]\nRUNTIME\n[[/code]]|END", "RUNTIME",),
        "<p>BEGIN|[[code]]<br>\nRUNTIME<br>\n[[/code]]|END</p>",
    );
}

#[test]
fn wikidot_delayed_list_preserves_trailing_span_content_space() {
    assert_eq!(
        render_authored(
            "En date: [[span style=\"background-color: gold;\"]] JAUNE [[/span]].",
        ),
        concat!(
            "<p>En date: ",
            "<span style=\"background-color: gold;\">JAUNE</span>",
            " .</p>",
        ),
    );
}

#[test]
fn wikidot_delayed_list_moves_size_boundary_spaces_outside_spans() {
    assert_eq!(
        render_authored(concat!(
            "[[size 240%]]I[[/size]]",
            "[[size 190%]]N [[/size]]H",
            "[[size 130%]] P[[/size]]",
        )),
        concat!(
            "<p>",
            r#"<span style="font-size:240%;">I</span>"#,
            r#"<span style="font-size:190%;">N</span> H "#,
            r#"<span style="font-size:130%;">P</span>"#,
            "</p>",
        ),
    );
}

#[test]
fn delayed_list_keeps_content_around_complete_empty_raw_lines_and_nested_blocks() {
    let html = render_authored(concat!(
        "ALPHA\n",
        "@@@@\n",
        "@@@@\n",
        "BRAVO\n",
        "@@@@\n",
        "@@@@\n",
        "CHARLIE\n",
        "[[=]]\n",
        "[[div class=\"addendum\"]]\n",
        "------\n",
        "[[collapsible show=\"OPEN\" hide=\"CLOSE\"]]\n",
        "[[<]]\n",
        "DELTA\n",
        "[[/<]]\n",
        "[[/collapsible]]\n",
        "------\n",
        "[[/div]]\n",
        "@@@@\n",
        "@@@@\n",
        "@@@@\n",
        "@@@@\n",
        "@@@@\n",
        "@@@@\n",
        "@@@@\n",
        "@@@@\n",
        "@@@@\n",
        "\n",
        "[[div class=\"finlog\"]]\n",
        "ECHO\n",
        "[[/div]]\n",
        "[[/=]]",
    ));

    for text in ["ALPHA", "BRAVO", "CHARLIE", "DELTA", "ECHO"] {
        assert!(html.contains(text), "{text} was lost from {html}");
    }
    assert!(
        !html.contains("[[/div]]") && !html.contains("[[/=]]"),
        "{html}"
    );
}

#[test]
fn delayed_runtime_scalar_keeps_content_across_repeated_empty_raw_lines() {
    let html = render_with_runtime_scalar(
        concat!(
            "ALPHA 2026/08/02",
            "[[footnote]]ALPHA NOTE[[/footnote]]\n",
            "@@@@\n",
            "@@@@\n",
            "BRAVO\n",
            "@@@@\n",
            "@@@@\n",
            "[[=]]\n",
            "[[div class=\"addendum\"]]\n",
            "CHARLIE\n",
            "[[/div]]\n",
            "[[/=]]",
        ),
        "2026/08/02",
    );

    for text in ["ALPHA", "2026/08/02", "ALPHA NOTE", "BRAVO", "CHARLIE"] {
        assert!(html.contains(text), "{text} was lost from {html}");
    }
    assert!(
        !html.contains("[[/div]]") && !html.contains("[[/=]]"),
        "{html}",
    );
}

#[test]
fn delayed_page_link_keeps_authored_block_container_active() {
    for (source, expected) in [
        (
            concat!(
                "[[div class=\"card-block\"]]\n",
                "%%title_linked%%\n",
                "[[/div]]",
            ),
            concat!(
                "<div class=\"card-block\"><p>",
                "<a href=\"/component:image-block\">Standard Image Block</a>",
                "</p></div>",
            ),
        ),
        (
            concat!(
                "[[div_ class=\"card-block\"]]\n",
                "%%title_linked%%\n",
                "[[/div]]",
            ),
            concat!(
                "<div class=\"card-block\">",
                "<a href=\"/component:image-block\">Standard Image Block</a>",
                "</div>",
            ),
        ),
    ] {
        assert_eq!(render(source), expected, "source: {source}");
    }
}

#[test]
fn delayed_page_link_keeps_inline_structural_div_continuation_active() {
    let html = render(concat!(
        "[[div_ class=\"outer\"]]\n",
        "[[ul]][[li class=\"folded\"]][[ul]]_[[/ul]]",
        "[[div class=\"linked-row\"]]\n",
        "%%title_linked%%\n",
        "[[/div]][[/li]][[/ul]][[/div]]",
    ));

    assert!(
        html.contains("<div class=\"linked-row\"><p>")
            && html.contains(concat!(
                "<a href=\"/component:image-block\">",
                "Standard Image Block</a>",
            ))
            && !html.contains("[[div"),
        "{html}",
    );
}

#[test]
fn delayed_page_link_uses_legacy_source_projection_inside_inline_raw() {
    assert_eq!(
        render("BEGIN|@@%%title_linked%%@@|END"),
        concat!(
            "<p>BEGIN|<span style=\"white-space: pre-wrap;\">",
            "[[[component:image-block | Standard Image Block]]]",
            "</span>|END</p>",
        ),
    );
}

#[test]
fn delayed_page_link_is_omitted_with_its_comment_owner() {
    assert_eq!(
        render("BEGIN|[!-- %%title_linked%% --]|END"),
        "<p>BEGIN||END</p>",
    );
}

#[test]
fn comment_elided_consumers_do_not_flatten_generated_provenance() {
    let link = render("BEGIN|[[[start|A[!-- %%title_linked%% --]B]]]|END");
    assert!(!link.contains("Standard Image Block"), "{link}");
    assert!(!link.contains("%%title_linked%%"), "{link}");

    let attribute =
        render("BEGIN|[[span class=\"A[!-- %%title_linked%% --]B\"]]X[[/span]]|END");
    assert!(!attribute.contains("Standard Image Block"), "{attribute}");
    assert!(!attribute.contains("%%title_linked%%"), "{attribute}");
    assert!(!attribute.contains("class=\"AB\""), "{attribute}");
}

#[test]
fn runtime_comment_shaped_text_cannot_enter_an_authored_field_view() {
    let html = render_with_runtime_scalar("[[[start|A[!--x--]B]]]", "[!--x--]");
    assert!(html.contains("[!--x--]"), "{html}");
    assert!(!html.contains("href=\"/start\""), "{html}");
}

#[test]
fn runtime_scalar_comment_closer_cannot_validate_an_authored_opener() {
    let source = "[!--x--]";
    let scalar_start = source.find("--]").expect("runtime closer fixture");
    let input = DelayedInput::new(
        source,
        vec![
            InputSegment::text(0..scalar_start, TextOrigin::Authored),
            InputSegment::text(scalar_start..source.len(), TextOrigin::RuntimeScalar),
        ],
    )
    .expect("valid mixed-provenance comment fixture");

    let html = render_input_with_bindings(&input, &SlotBindings::empty());
    assert!(html.contains('x'), "{html}");
    assert!(html.contains("--]"), "{html}");
}

#[test]
fn runtime_literal_comment_closer_retains_source_grammar() {
    let source = "[!--x--]";
    let literal_start = source.find("--]").expect("runtime closer fixture");
    let input = DelayedInput::new(
        source,
        vec![
            InputSegment::text(0..literal_start, TextOrigin::Authored),
            InputSegment::text(literal_start..source.len(), TextOrigin::RuntimeLiteral),
        ],
    )
    .expect("valid mixed-provenance comment fixture");

    assert_eq!(
        render_input_with_bindings(&input, &SlotBindings::empty()),
        "",
    );
}

#[test]
fn runtime_scalar_dashes_cannot_close_a_valid_authored_comment_early() {
    let source = "[!--a--]b--]";
    let scalar_start = source.find("a--]").expect("runtime dash fixture") + 1;
    let scalar_end = scalar_start + 2;
    let input = DelayedInput::new(
        source,
        vec![
            InputSegment::text(0..scalar_start, TextOrigin::Authored),
            InputSegment::text(scalar_start..scalar_end, TextOrigin::RuntimeScalar),
            InputSegment::text(scalar_end..source.len(), TextOrigin::Authored),
        ],
    )
    .expect("valid mixed-provenance comment fixture");

    assert_eq!(
        render_input_with_bindings(&input, &SlotBindings::empty()),
        "",
    );
}

#[test]
fn anchor_links_preserve_delayed_owner_and_runtime_scalar_provenance() {
    assert_eq!(
        render("BEGIN|[#toc1 %%title_linked%%]|END"),
        concat!(
            "<p>BEGIN|[#toc1 ",
            "<a href=\"/component:image-block\">Standard Image Block</a>",
            "]|END</p>",
        ),
    );
    assert_eq!(
        render("BEGIN|[#%%title_linked%% Label]|END"),
        concat!(
            "<p>BEGIN|[#",
            "<a href=\"/component:image-block\">Standard Image Block</a>",
            " Label]|END</p>",
        ),
    );
    assert_eq!(
        render("BEGIN|[*#toc1 %%title_linked%%]|END"),
        concat!(
            "<p>BEGIN|[*#toc1 ",
            "<a href=\"/component:image-block\">Standard Image Block</a>",
            "]|END</p>",
        ),
    );
    assert_eq!(
        render("BEGIN|[#toc1 A[!--%%title_linked%%--]B]|END"),
        r##"<p>BEGIN|<a href="#toc1">AB</a>|END</p>"##,
    );
    assert_eq!(
        render_with_runtime_scalar("[#toc1 Label]", "toc1"),
        r##"<p><a href="#toc1">Label</a></p>"##,
    );
    assert_eq!(
        render_with_runtime_scalar("[#toc1 Label]", "toc1 Label"),
        "<p>[#toc1 Label]</p>",
    );
    assert_eq!(
        render_with_runtime_scalar("[*#toc1 Label]", "#"),
        "<p>[*#toc1 Label]</p>",
    );
}

#[test]
fn named_anchor_markers_preserve_delayed_and_runtime_provenance() {
    assert_eq!(
        render_authored("BEGIN|[[# alpha]]|END"),
        r#"<p>BEGIN|<a name="alpha"></a>|END</p>"#,
    );

    let generated = render("BEGIN|[[# %%title_linked%%]]|END");
    assert!(generated.contains("BEGIN|[[# "), "{generated}");
    assert!(generated.contains(concat!(
        r#"<a href="/component:image-block">"#,
        "Standard Image Block</a>",
    )));
    assert!(generated.contains("]]|END"), "{generated}");
    assert!(!generated.contains("<a name"), "{generated}");

    let runtime_name = render_with_runtime_scalar("BEGIN|[[# alpha]]|END", "alpha");
    assert_eq!(runtime_name, "<p>BEGIN|[[# alpha]]|END</p>");
    assert!(!runtime_name.contains("<a name"), "{runtime_name}");

    let runtime_marker =
        render_with_runtime_scalar("BEGIN|[[# alpha]]|END", "[[# alpha]]");
    assert_eq!(runtime_marker, "<p>BEGIN|[[# alpha]]|END</p>");
    assert!(!runtime_marker.contains("<a name"), "{runtime_marker}");
}

#[test]
fn slot_occupancy_recovers_outer_links_as_authored_source() {
    assert_eq!(
        render("BEGIN|[[[scp-003|%%title_linked%%]]]|END"),
        concat!(
            "<p>BEGIN|[[[scp-003|",
            "<a href=\"/component:image-block\">Standard Image Block</a>",
            "]]]|END</p>",
        ),
    );
    assert_eq!(
        render("BEGIN|[https://example.com %%title_linked%%]|END"),
        concat!(
            "<p>BEGIN|[<a href=\"https://example.com\">https://example.com</a> ",
            "<a href=\"/component:image-block\">Standard Image Block</a>",
            "]|END</p>",
        ),
    );
}

#[test]
fn page_and_tag_slots_have_distinct_span_attribute_recovery() {
    assert_eq!(
        render("BEGIN|[[span title=\"%%title_linked%%\"]]X[[/span]]|END"),
        "<p>BEGIN|<span>X</span>|END</p>",
    );
    assert_eq!(
        render("BEGIN|[[span class=\"%%title_linked%%\"]]X[[/span]]|END"),
        "<p>BEGIN|<span class>X</span>|END</p>",
    );
    assert_eq!(
        render_tag("BEGIN|[[span title=\"%%tags_linked%%\"]]X[[/span]]|END"),
        concat!(
            "<p>BEGIN|[[span title=&quot;",
            "<a href=\"/system:page-tags/tag/component\">component</a>",
            "&quot;]]X[[/span]]|END</p>",
        ),
    );
}

#[test]
fn malformed_empty_key_blocks_cannot_promote_generated_values_to_structure() {
    for (open, close) in [
        ("[[div =\"%%title_linked%%\"]]", "[[/div]]"),
        ("[[span =\"%%title_linked%%\"]]", "[[/span]]"),
    ] {
        let source = format!("{open}**literal**{close}");
        let html = render(&source);

        assert!(html.contains("[["), "{html}");
        assert!(html.contains(close), "{html}");
        assert!(
            html.contains(concat!(
                r#"<a href="/component:image-block">"#,
                "Standard Image Block</a>",
            )),
            "{html}",
        );
        assert!(!html.contains("<div "), "{html}");
        assert!(!html.contains("<div>"), "{html}");
        assert!(!html.contains("<span "), "{html}");
        assert!(!html.contains("<span>"), "{html}");
        assert!(html.contains("<strong>literal</strong>"), "{html}");
        assert!(!html.contains("%%title_linked%%"), "{html}");
    }
}

#[test]
fn page_slot_recovers_image_owner_while_tag_slot_stays_attribute_text() {
    assert_eq!(
        render("BEGIN|[[image https://example.com/x.png alt=\"%%title_linked%%\"]]|END",),
        concat!(
            "<p>BEGIN|[[image ",
            "<a href=\"https://example.com/x.png\">https://example.com/x.png</a>",
            " alt=&quot;",
            "<a href=\"/component:image-block\">Standard Image Block</a>",
            "&quot;]]|END</p>",
        ),
    );
    assert_eq!(
        render_tag(
            "BEGIN|[[image https://example.com/x.png alt=\"%%tags_linked%%\"]]|END",
        ),
        concat!(
            "\n\nBEGIN|<img src=\"https://example.com/x.png\" class=\"image\" ",
            "alt=\"[/system:page-tags/tag/component component]\">|END",
        ),
    );
}

#[test]
fn generated_image_attributes_preserve_implicit_attachment_disposition() {
    assert_eq!(
        render_tag(r#"[[=image photo.jpeg alt="%%tags_linked%%"]]"#),
        concat!(
            "<div class=\"image-container aligncenter\">",
            "<a href=\"https://sandbox.wjfiles.com/local--files/some-page/photo.jpeg\">",
            "<img src=\"https://sandbox.wjfiles.com/local--resized-images/some-page/photo.jpeg/medium.jpg\" ",
            "class=\"image\" alt=\"[/system:page-tags/tag/component component]\">",
            "</a></div>",
        ),
    );
    assert_eq!(
        render_tag(r#"[[image photo.jpeg link="%%tags_linked%%"]]"#),
        concat!(
            "<a href=\"/[/system:page-tags/tag/component%20component]\">",
            "<img src=\"https://sandbox.wjfiles.com/local--resized-images/some-page/photo.jpeg/medium.jpg\" ",
            "class=\"image\" alt=\"photo.jpeg\">",
            "</a>",
        ),
    );
    assert_eq!(
        render_tag(r#"[[=image photo.png alt="%%tags_linked%%"]]"#),
        concat!(
            "<div class=\"image-container aligncenter\">",
            "<img src=\"https://sandbox.wjfiles.com/local--files/some-page/photo.png\" ",
            "class=\"image\" alt=\"[/system:page-tags/tag/component component]\">",
            "</div>",
        ),
    );
    assert_eq!(
        render_tag(r#"[[image photo.png link="%%tags_linked%%"]]"#),
        concat!(
            "<a href=\"/[/system:page-tags/tag/component%20component]\">",
            "<img src=\"https://sandbox.wjfiles.com/local--files/some-page/photo.png\" ",
            "class=\"image\" alt=\"photo.png\">",
            "</a>",
        ),
    );
}

#[test]
fn slot_schema_rejects_missing_and_wrong_kind_bindings_atomically() {
    let source = "%%title_linked%%";
    let input = page_link_input(source);
    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
    let delayed = parse_delayed_list(&input, &page_info, &settings)
        .expect("supported delayed input");

    assert!(delayed.bind(&SlotBindings::empty()).is_err());
    let wrong = SlotBindings::new(vec![(
        SlotId::new(1),
        GeneratedValue::TagLinks {
            tags: Cow::Borrowed(&[]),
            separator: Cow::Borrowed(" "),
        },
    )])
    .expect("unique bindings");
    assert!(delayed.bind(&wrong).is_err());
}

#[test]
fn authored_marker_shaped_text_cannot_steal_a_generated_slot() {
    let source = "%%title_linked%%|%%title_linked%%|%%title_linked%%";
    let marker = "%%title_linked%%";
    let generated_start = marker.len() + 1;
    let generated_end = generated_start + marker.len();
    let input = DelayedInput::new(
        source,
        vec![
            InputSegment::text(0..generated_start, TextOrigin::RuntimeScalar),
            InputSegment::generated(GeneratedInput {
                source_range: generated_start..generated_end,
                id: SlotId::new(1),
                kind: GeneratedKind::PageLink,
                occurrence: 0,
            }),
            InputSegment::text(generated_end..source.len(), TextOrigin::RuntimeScalar),
        ],
    )
    .expect("valid collision fixture");
    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
    let delayed = parse_delayed_list(&input, &page_info, &settings)
        .expect("supported delayed input");
    let bound = delayed
        .bind(&page_bindings())
        .expect("out-of-band slot binds atomically");

    assert_eq!(
        bound.render_html(&page_info, &settings).body(),
        concat!(
            "<p>%%title_linked%%|",
            "<a href=\"/component:image-block\">Standard Image Block</a>",
            "|%%title_linked%%</p>",
        ),
    );
}

#[test]
fn nested_line_start_owners_bind_without_retaining_delayed_leaves() {
    for source in [
        "|| %%title_linked%% ||\n",
        "* %%title_linked%%\n",
        "term:: %%title_linked%%\n",
        concat!(
            "[[tabview]]\n",
            "[[tab One]]\n",
            "%%title_linked%%\n",
            "[[/tab]]\n",
            "[[/tabview]]",
        ),
    ] {
        let input = page_link_input(source);
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
        let delayed = parse_delayed_list(&input, &page_info, &settings)
            .expect("supported delayed input");
        let bound = delayed
            .bind(&page_bindings())
            .expect("every nested owner is traversed during atomic binding");
        let html = bound.render_html(&page_info, &settings);
        assert!(
            html.body()
                .contains("<a href=\"/component:image-block\">Standard Image Block</a>",)
                && !html.body().contains("%%title_linked%%"),
            "a renderable nested owner must be completely bound: {source}",
        );
    }
}

#[test]
fn deep_delayed_binding_does_not_inherit_a_small_caller_stack() {
    const DEPTH: usize = 384;
    let mut source = "[[div]]\n".repeat(DEPTH);
    source.push_str("%%title_linked%%\n");
    source.push_str(&"[[/div]]\n".repeat(DEPTH));

    let worker = std::thread::Builder::new()
        .name("delayed-small-stack".to_owned())
        .stack_size(512 * 1024)
        .spawn(move || render(&source))
        .expect("start delayed small-stack worker");
    let html = worker
        .join()
        .expect("deep delayed binding must not overflow the caller stack");

    assert!(
        html.contains(r#"<a href="/component:image-block">Standard Image Block</a>"#,),
        "{html}"
    );
    assert!(!html.contains("%%title_linked%%"), "{html}");
}

#[test]
fn delayed_underclosed_div_suffix_parses_within_a_bounded_budget() {
    let source = concat!(
        "[[mo }\n[]\n",
        "[[div]]\n[[div]]\n[[div]]\n[[div]]\n[[div]]\n",
        "[[[div]]\n",
        "[[div]]\n[[div]]\n[[div]]\n[[div]]\n[[div]]\n[[div]]\n",
        "SS]red{color:   odule]]\n",
    );
    assert_eq!(source.len(), 131);
    let input = DelayedInput::new(
        source,
        vec![
            InputSegment::text(0..129, TextOrigin::Authored),
            InputSegment::generated(GeneratedInput {
                source_range: 129..130,
                id: SlotId::new(1),
                kind: GeneratedKind::TagLinks,
                occurrence: 0,
            }),
            InputSegment::text(130..130, TextOrigin::Authored),
            InputSegment::generated(GeneratedInput {
                source_range: 130..131,
                id: SlotId::new(1),
                kind: GeneratedKind::TagLinks,
                occurrence: 1,
            }),
            InputSegment::text(131..131, TextOrigin::RuntimeLiteral),
        ],
    )
    .expect("valid delayed timeout regression fixture");
    let page_info = PageInfo::dummy();
    let started = Instant::now();
    for layout in [Layout::Wikidot, Layout::Wikijump] {
        for inline in [false, true] {
            let mut settings = WikitextSettings::from_mode(WikitextMode::List, layout);
            settings.list_pages_inline = inline;
            let _ = parse_delayed_list(&input, &page_info, &settings);
        }
    }
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "delayed underclosed div parsing exceeded budget: {:?}",
        started.elapsed(),
    );
}

#[test]
fn delayed_underclosed_span_suffix_parses_within_a_bounded_budget() {
    let source = format!("{}tail", "[[span]]".repeat(40));
    let input = DelayedInput::new(
        &source,
        vec![InputSegment::text(0..source.len(), TextOrigin::Authored)],
    )
    .expect("valid delayed underclosed span fixture");
    let page_info = PageInfo::dummy();
    let started = Instant::now();
    for layout in [Layout::Wikidot, Layout::Wikijump] {
        for inline in [false, true] {
            let mut settings = WikitextSettings::from_mode(WikitextMode::List, layout);
            settings.list_pages_inline = inline;
            let _ = parse_delayed_list(&input, &page_info, &settings);
        }
    }
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "delayed underclosed span parsing exceeded budget: {:?}",
        started.elapsed(),
    );
}

#[test]
fn orphan_tab_fallback_keeps_generated_fragment_provenance() {
    let source = concat!("[[tab]]\n", "%%title_linked%%\n", "[[/tab]]",);
    let html = render(source);

    assert!(html.contains("[[tab]]"), "{html}");
    assert!(html.contains("[[/tab]]"), "{html}");
    assert!(
        html.contains(concat!(
            r#"<a href="/component:image-block">"#,
            "Standard Image Block</a>",
        )),
        "{html}",
    );
    assert!(!html.contains("%%title_linked%%"), "{html}");
}

#[test]
fn bibliography_definition_values_bind_before_rendering() {
    let html = render(concat!(
        "[[bibliography title=\"Works\"]]\n",
        ": alpha : %%title_linked%%\n",
        "[[/bibliography]]",
    ));

    assert!(
        html.contains(concat!(
            r#"<a href="/component:image-block">"#,
            "Standard Image Block</a>",
        )),
        "bibliography values are out-of-band syntax-tree owners: {html}",
    );
    assert!(
        !html.contains("%%title_linked%%"),
        "a bound bibliography must not retain a delayed leaf: {html}",
    );
}

#[test]
fn adjacent_authored_text_segments_do_not_create_synthetic_token_boundaries() {
    let source = "**bold** %%title_linked%%";
    let marker_start = source.find("%%title_linked%%").expect("fixture marker");
    let marker_end = source.len();

    let input = DelayedInput::new(
        source,
        vec![
            InputSegment::text(0..1, TextOrigin::Authored),
            InputSegment::text(1..marker_start, TextOrigin::Authored),
            InputSegment::generated(GeneratedInput {
                source_range: marker_start..marker_end,
                id: SlotId::new(1),
                kind: GeneratedKind::PageLink,
                occurrence: 0,
            }),
        ],
    )
    .expect("adjacent authored text remains a valid provenance split");

    assert_eq!(
        render_input(&input),
        concat!(
            "<p><strong>bold</strong> ",
            "<a href=\"/component:image-block\">Standard Image Block</a>",
            "</p>",
        ),
    );
}

#[test]
fn runtime_scalar_text_renders_without_markup_activation() {
    let source = "**bold** [[html]]X[[/html]] https://tracker.example/";
    let input = DelayedInput::new(
        source,
        vec![InputSegment::text(
            0..source.len(),
            TextOrigin::RuntimeScalar,
        )],
    )
    .expect("runtime scalar input is valid");
    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
    let delayed = parse_delayed_list(&input, &page_info, &settings)
        .expect("runtime scalar text is supported");
    let bound = delayed
        .bind(&SlotBindings::empty())
        .expect("runtime scalar text has no generated bindings");
    let html = bound.render_html(&page_info, &settings);

    assert_eq!(
        html.body(),
        "<p>**bold** [[html]]X[[/html]] https://tracker.example/</p>",
    );
    assert!(html.html_blocks().is_empty());
    assert!(!html.body().contains("<strong>"));
    assert!(!html.body().contains("<a "));
}

#[test]
fn runtime_scalar_advanced_table_markers_remain_inert() {
    let source = "[[table]]\n[[row]]\n[[cell]]A[[/cell]]\n[[/row]]\n[[/table]]";
    let input = DelayedInput::new(
        source,
        vec![InputSegment::text(
            0..source.len(),
            TextOrigin::RuntimeScalar,
        )],
    )
    .expect("runtime scalar input is valid");

    let html = render_input_with_bindings(&input, &SlotBindings::empty());
    assert_eq!(html, format!("<p>{source}</p>"));
    assert!(!html.contains("<table>"));
}

#[test]
fn generated_values_do_not_become_advanced_table_attributes() {
    for source in [
        concat!(
            "[[table class=\"%%title_linked%%\"]]\n",
            "[[row]]\n[[cell]]A[[/cell]]\n[[/row]]\n[[/table]]",
        ),
        concat!(
            "[[table]]\n[[row]]\n",
            "[[cell colspan=\"%%title_linked%%\"]]A[[/cell]]\n",
            "[[/row]]\n[[/table]]",
        ),
    ] {
        let html = render(source);
        assert!(!html.contains("<table>"), "{html}");
        assert!(
            html.contains(concat!(
                r#"<a href="/component:image-block">"#,
                "Standard Image Block</a>",
            )),
            "{html}",
        );
        assert!(!html.contains("%%title_linked%%"), "{html}");
    }
}

#[test]
fn runtime_scalar_values_do_not_become_advanced_table_attributes() {
    let source = concat!(
        "[[table class=\"runtime-value\"]]\n",
        "[[row]]\n[[cell]]A[[/cell]]\n[[/row]]\n[[/table]]",
    );
    let html = render_with_runtime_scalar(source, "runtime-value");

    assert!(!html.contains("<table>"), "{html}");
    assert!(html.contains("runtime-value"), "{html}");
    assert!(html.contains('A'), "{html}");
}

#[test]
fn delayed_advanced_table_paragraphs_keep_generated_links() {
    let html = render(concat!(
        "[[table]]\n[[row]]\n[[cell]]\n",
        "A %%title_linked%%\n\nB\n",
        "[[/cell]]\n[[/row]]\n[[/table]]",
    ));

    assert!(
        html.contains(concat!(
            "<td><p>A ",
            "<a href=\"/component:image-block\">Standard Image Block</a>",
            "</p><p>B</p></td>",
        )),
        "{html}",
    );
    assert!(!html.contains("%%title_linked%%"), "{html}");
}

#[test]
fn runtime_scalar_nested_table_markers_stay_text_in_authored_cell_paragraphs() {
    let nested = "[[table]][[row]][[cell]]I[[/cell]][[/row]][[/table]]";
    let source = format!(
        "[[table]]\n[[row]]\n[[cell]]\nA\n\n{nested}\n\nB\n[[/cell]]\n[[/row]]\n[[/table]]"
    );
    let start = source.find(nested).expect("nested marker fixture");
    let end = start + nested.len();
    let input = DelayedInput::new(
        &source,
        vec![
            InputSegment::text(0..start, TextOrigin::Authored),
            InputSegment::text(start..end, TextOrigin::RuntimeScalar),
            InputSegment::text(end..source.len(), TextOrigin::Authored),
        ],
    )
    .expect("mixed table provenance is valid");

    let html = render_input_with_bindings(&input, &SlotBindings::empty());
    assert_eq!(html.matches("<table>").count(), 1, "{html}");
    assert!(html.contains(nested), "{html}");
    assert!(html.contains("<p>A</p>"), "{html}");
    assert!(html.contains("<p>B</p>"), "{html}");
}

#[test]
fn runtime_scalar_text_does_not_complete_authored_delimiters() {
    let source = "**injected**";
    let input = DelayedInput::new(
        source,
        vec![
            InputSegment::text(0..1, TextOrigin::Authored),
            InputSegment::text(1..source.len() - 1, TextOrigin::RuntimeScalar),
            InputSegment::text(source.len() - 1..source.len(), TextOrigin::Authored),
        ],
    )
    .expect("mixed provenance input is valid");

    assert_eq!(
        render_input_with_bindings(&input, &SlotBindings::empty()),
        "<p>**injected**</p>",
    );
}

#[test]
fn delayed_heading_binds_before_building_its_table_of_contents_label() {
    let html = render("[[toc]]\n+ %%title_linked%%");
    assert!(
        html.contains(concat!(
            r#"<div style="margin-left: 1em;">"#,
            r#"<a href="javascript:;">Standard Image Block</a></div>"#,
        ),),
        "the TOC label should use the bound heading text: {html}",
    );
    assert!(
        html.contains(concat!(
            r#"<h1><span><a href="/component:image-block">"#,
            "Standard Image Block</a></span></h1>",
        ),),
        "the heading should retain the bound page link: {html}",
    );
}

#[test]
fn tag_external_label_recovery_matches_zero_one_and_many_textual_tags() {
    let source = "BEGIN|[https://example.com %%tags_linked%%]|END";
    for (tags, expected) in [
        (
            &[][..],
            r#"<p>BEGIN|<a href="https://example.com"></a>|END</p>"#,
        ),
        (
            &["component"][..],
            concat!(
                r#"<p>BEGIN|<a href="https://example.com">"#,
                "[/system:page-tags/tag/component component</a>]|END</p>",
            ),
        ),
        (
            &["component", "featured"][..],
            concat!(
                r#"<p>BEGIN|<a href="https://example.com">"#,
                "[/system:page-tags/tag/component component</a> ",
                "[/system:page-tags/tag/featured featured]]|END</p>",
            ),
        ),
    ] {
        assert_eq!(
            render_tag_values(source, tags, " "),
            expected,
            "tag cardinality: {}",
            tags.len(),
        );
    }
}

#[test]
fn empty_generated_tag_links_trim_their_preceding_line_end_space() {
    assert_eq!(
        render_tag_values("Tags: %%tags_linked%%", &[], " "),
        "<p>Tags:</p>",
    );
    assert_eq!(
        render_tag_values("Tags: %%tags_linked%%", &["component"], " "),
        concat!(
            r#"<p>Tags: <a href="/system:page-tags/tag/component">"#,
            "component</a></p>",
        ),
    );
}

#[test]
fn empty_generated_tag_links_remove_their_empty_list_mode_paragraph() {
    assert_eq!(
        render_tag_values(
            "[[div class=\"unbold\"]]\n%%tags_linked%%\n[[/div]]",
            &[],
            " ",
        ),
        "<div class=\"unbold\"></div>",
    );
}

#[test]
fn delayed_image_attributes_preserve_authored_links_and_suffix() {
    let linked = render_tag(r#"[[image x.png alt="%%tags_linked%%" link="SCP-002"]]"#);
    assert!(
        linked.contains(r#"<a href="/SCP-002"><img "#),
        "authored image link must survive delayed alt binding: {linked}",
    );

    let external =
        render_tag(r#"[[image x.png alt="%%tags_linked%%" link="https://example.com"]]"#);
    assert!(
        external.contains(r#"<a href="https://example.com"><img "#),
        "{external}",
    );

    let suffixed = render_tag(r#"[[image x.png alt="%%tags_linked%% suffix"]]"#);
    assert!(
        suffixed.contains(r#"alt="[/system:page-tags/tag/component component] suffix""#,),
        "authored attribute suffix must survive delayed binding: {suffixed}",
    );
}

#[test]
fn delayed_raw_decodes_entities_and_div_shell_keeps_the_parsed_opener() {
    assert_eq!(
        render("@@&amp; %%title_linked%%@@"),
        concat!(
            r#"<p><span style="white-space: pre-wrap;">&amp; "#,
            "[[[component:image-block | Standard Image Block]]]",
            "</span></p>",
        ),
    );

    let div = render(r#"[[div title="[[probe"]]%%title_linked%%[[/div]]"#);
    assert!(
        div.contains(r#"[[div title=&quot;[[probe&quot;]]"#),
        "delayed div shell must start at its parsed opener: {div}",
    );

    for source in [
        "[[ raw]]%%title_linked%%[[/raw]]",
        "[[\u{2003}raw]]%%title_linked%%[[/raw]]",
    ] {
        let html = render(source);
        let opener = source.split("%%title_linked%%").next().unwrap();
        assert!(
            html.contains(opener),
            "delayed raw shell must preserve the complete opener {opener:?}: {html}",
        );
    }

    for source in [
        "[[   div]]%%title_linked%%[[/div]]",
        "[[\u{2003}div]]%%title_linked%%[[/div]]",
    ] {
        let html = render(source);
        let opener = source.split("%%title_linked%%").next().unwrap();
        assert!(
            html.contains(opener),
            "delayed div shell must preserve the complete opener {opener:?}: {html}",
        );
    }
}

#[test]
fn delayed_raw_adjacency_does_not_flatten_generated_or_runtime_values() {
    let generated = render("@@A@@@@%%title_linked%%@@");
    assert_eq!(
        generated.matches("white-space: pre-wrap;").count(),
        2,
        "{generated}",
    );
    assert!(
        generated.contains("[[[component:image-block | Standard Image Block]]]"),
        "{generated}",
    );

    let runtime = render_with_runtime_scalar("@@A@@@@B@@", "@@B@@");
    assert_eq!(
        runtime.matches("white-space: pre-wrap;").count(),
        1,
        "{runtime}",
    );
    assert!(runtime.contains("@@B@@"), "{runtime}");
}

#[test]
fn delayed_raw_inside_math_rolls_back_without_erasing_values() {
    let generated = render("[[$OUTER @@%%title_linked%%@@ TAIL$]]");
    assert!(!generated.contains("math-inline"), "{generated}");
    assert!(
        generated.contains("[[[component:image-block | Standard Image Block]]]"),
        "{generated}",
    );
    assert!(generated.contains("[[$OUTER "), "{generated}");
    assert!(generated.contains(" TAIL$]]"), "{generated}");

    let runtime =
        render_with_runtime_scalar("[[$OUTER @@2026/08/02@@ TAIL$]]", "2026/08/02");
    assert!(!runtime.contains("math-inline"), "{runtime}");
    assert!(runtime.contains("2026/08/02"), "{runtime}");
    assert!(runtime.contains("[[$OUTER "), "{runtime}");
    assert!(runtime.contains(" TAIL$]]"), "{runtime}");
}

#[test]
fn delayed_code_shell_keeps_the_parsed_opener() {
    assert_eq!(
        render(concat!(
            "BEGIN|[[code type=\"[[probe\"]]\n",
            "%%title_linked%%\n",
            "[[/code]]|END",
        )),
        concat!(
            "<p>BEGIN|[[code type=&quot;[[probe&quot;]]<br>\n",
            "<a href=\"/component:image-block\">Standard Image Block</a>",
            "<br>\n[[/code]]|END</p>",
        ),
    );
}

#[test]
fn delayed_code_shell_handles_unicode_space_before_name() {
    let html = render_with_layout(
        "[[\u{2003}code]]\n%%title_linked%%\n[[/code]]",
        Layout::Wikijump,
    );
    assert!(
        html.contains("[[\u{2003}code]]"),
        "delayed code shell must start at its parsed opener: {html}",
    );
}

#[test]
fn delayed_raw_recovery_owns_uninterpreted_heads() {
    let source = concat!(
        "BEGIN|[[raw probe=\"%%title_linked%%\"]]\n",
        "**AUTHORED**\n",
        "[[/raw]]|END",
    );
    assert_eq!(
        render(source),
        concat!(
            "<p>BEGIN|[[raw probe=&quot;",
            "<a href=\"/component:image-block\">Standard Image Block</a>",
            "&quot;]]<br>\n**AUTHORED**<br>\n[[/raw]]|END</p>",
        ),
    );

    let source = concat!(
        "BEGIN|[[raw malformed=\"unterminated]]\n",
        "**AUTHORED** %%title_linked%%\n",
        "[[/raw]]|END",
    );
    assert_eq!(
        render(source),
        concat!(
            "<p>BEGIN|[[raw malformed=&quot;unterminated]]<br>\n",
            "**AUTHORED** ",
            "<a href=\"/component:image-block\">Standard Image Block</a>",
            "<br>\n[[/raw]]|END</p>",
        ),
    );
}

#[test]
fn recovery_projections_preserve_complete_page_references() {
    let bindings = page_bindings_for(
        PageRef::page_and_site("Other Wiki", "Target Page#toc2"),
        "Remote Target",
    );

    for (source, expected) in [
        (
            "@@%%title_linked%%@@",
            concat!(
                r#"<p><span style="white-space: pre-wrap;">"#,
                "[[[:other-wiki:target-page#toc2 | Remote Target]]]",
                "</span></p>",
            ),
        ),
        (
            "[[#if true | %%title_linked%% | FALLBACK]]",
            "<p>[[[:other-wiki:target-page#toc2] | FALLBACK]]</p>",
        ),
    ] {
        let input = page_link_input(source);
        assert_eq!(
            render_input_with_bindings(&input, &bindings),
            expected,
            "source: {source}",
        );
    }
}

#[test]
fn recovery_source_decodes_semicolon_entities_once() {
    assert_eq!(
        render(concat!(
            "[[code]]\n",
            "&amp; %%title_linked%%\n",
            "[[/code]]",
        )),
        concat!(
            "<p>[[code]]<br>\n&amp; ",
            "<a href=\"/component:image-block\">Standard Image Block</a>",
            "<br>\n[[/code]]</p>",
        ),
    );
    assert_eq!(
        render("[[#if true | %%title_linked%% | A &amp; B]]"),
        "<p>[[[component:image-block] | A &amp; B]]</p>",
    );
}

#[test]
fn parser_recovery_diagnostics_do_not_bypass_atomic_slot_binding() {
    let mut observed_recovered_parse = false;
    for source in [
        "BEGIN|[[[scp-003|%%title_linked%%]]]|END",
        "BEGIN|[https://example.com %%title_linked%%]|END",
        "BEGIN|[[image https://example.com/x.png alt=\"%%title_linked%%\"]]|END",
        "BEGIN|[[#if true | %%title_linked%% | NO]]|END",
    ] {
        let input = page_link_input(source);
        let page_info = PageInfo::dummy();
        let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
        let delayed = parse_delayed_list(&input, &page_info, &settings)
            .expect("supported delayed input");
        if !delayed.errors().is_empty() {
            observed_recovered_parse = true;
            delayed
                .bind(&page_bindings())
                .expect("closed recovery owner still binds atomically");
        }
    }
    assert!(
        observed_recovered_parse,
        "the policy fixture should exercise an observable parser recovery",
    );
}

#[test]
fn delayed_slots_preserve_the_frozen_listpages_owner_boundaries() {
    for (source, expected) in [
        (
            "BEGIN|{{{%%title_linked%%}}}|END",
            concat!(
                "<p>BEGIN|<tt>{",
                "<a href=\"/component:image-block\">Standard Image Block</a>",
                "}</tt>|END</p>",
            ),
        ),
        (
            "BEGIN|[[code]]\n%%title_linked%%\n[[/code]]|END",
            concat!(
                "<p>BEGIN|[[code]]<br>\n",
                "<a href=\"/component:image-block\">Standard Image Block</a>",
                "<br>\n[[/code]]|END</p>",
            ),
        ),
        (
            "BEGIN|[[raw]]\n%%title_linked%%\n[[/raw]]|END",
            concat!(
                "<p>BEGIN|[[raw]]<br>\n",
                "<a href=\"/component:image-block\">Standard Image Block</a>",
                "<br>\n[[/raw]]|END</p>",
            ),
        ),
        (
            "BEGIN|[[div class=\"probe\"]]%%title_linked%%[[/div]]|END",
            concat!(
                "<p>BEGIN|[[div class=&quot;probe&quot;]]",
                "<a href=\"/component:image-block\">Standard Image Block</a>",
                "[[/div]]|END</p>",
            ),
        ),
        (
            "BEGIN|[[[%%title_linked%%|LABEL]]]|END",
            concat!(
                "<p>BEGIN|[[[",
                "<a href=\"/component:image-block\">Standard Image Block</a>",
                "|LABEL]]]|END</p>",
            ),
        ),
        (
            "BEGIN|[%%title_linked%% LABEL]|END",
            concat!(
                "<p>BEGIN|[",
                "<a href=\"/component:image-block\">Standard Image Block</a>",
                " LABEL]|END</p>",
            ),
        ),
        (
            "BEGIN|[[#if true | %%title_linked%% | NO]]|END",
            "<p>BEGIN|[[[component:image-block] | NO]]|END</p>",
        ),
    ] {
        assert_eq!(render(source), expected, "source: {source}");
    }

    for (source, expected) in [
        (
            "BEGIN|[[raw]]\n%%tags_linked%%\n[[/raw]]|END",
            concat!(
                "<p>BEGIN|[[raw]]<br>\n",
                "<a href=\"/system:page-tags/tag/component\">component</a>",
                "<br>\n[[/raw]]|END</p>",
            ),
        ),
        (
            "BEGIN|[https://example.com %%tags_linked%%]|END",
            concat!(
                "<p>BEGIN|<a href=\"https://example.com\">",
                "[/system:page-tags/tag/component component",
                "</a>]|END</p>",
            ),
        ),
        (
            "BEGIN|[[image https://example.com/x.png link=\"%%tags_linked%%\"]]|END",
            concat!(
                "\n\nBEGIN|<a href=\"/[/system:page-tags/tag/component%20component]\">",
                "<img src=\"https://example.com/x.png\" class=\"image\" alt=\"x.png\">",
                "</a>|END",
            ),
        ),
        (
            "BEGIN|[[#if true | %%tags_linked%% | NO]]|END",
            concat!(
                "<p>BEGIN|",
                "<a href=\"/system:page-tags/tag/component\">component</a>",
                "|END</p>",
            ),
        ),
    ] {
        assert_eq!(render_tag(source), expected, "source: {source}");
    }
}

#[test]
fn right_angle_run_touching_a_suppressed_seam_converts_every_pair() {
    assert_eq!(render("a[!-- %%title_linked%% --]>>>>"), "<p>a»»</p>");
    assert_eq!(render("a>[!-- %%title_linked%% --]>>>"), "<p>a»»</p>");
    assert_eq!(render("a>>[!-- %%title_linked%% --]>>>>"), "<p>a»»»</p>");
    assert_eq!(render("\"[!-- %%title_linked%% --]>>>>"), "<p>&quot;»»</p>");
    assert_eq!(render("a>[!-- %%title_linked%% --]>>>>"), "<p>a»»&gt;</p>");
}

#[test]
fn left_angle_and_ellipsis_runs_touching_a_suppressed_seam_convert_with_joined_grouping()
{
    assert_eq!(render("<[!-- %%title_linked%% --]<<<"), "<p>««</p>");
    assert_eq!(render(".[!-- %%title_linked%% --]..."), "<p>….</p>");
    assert_eq!(render(".[!-- %%title_linked%% --]....."), "<p>……</p>");
    assert_eq!(render("a[!-- %%title_linked%% --]..."), "<p>a…</p>");
    assert_eq!(
        render("<[!-- %%title_linked%% --]\u{ab}<"),
        "<p>&lt;«&lt;</p>",
    );
}

#[test]
fn runtime_origin_text_stays_a_typography_barrier_at_a_seam() {
    let source = "runtime-<[!-- %%title_linked%% --]<<";
    let input = page_link_input_with_runtime_prefix(source, "runtime-<");
    assert_eq!(render_input(&input), "<p>runtime-&lt;«</p>");
}

#[test]
fn suppressed_delayed_owners_apply_typography_only_across_authored_text() {
    assert_eq!(
        render("left-[[iftags +missing]]\n%%title_linked%%\n[[/iftags]]-right"),
        "<p>left—right</p>",
    );
    assert_eq!(
        render("left.[!-- %%title_linked%% --]..right"),
        "<p>left…right</p>",
    );

    let source = "`[!-- %%title_linked%% --]`quoted'[!-- %%title_linked%% --]'";
    let marker = "%%title_linked%%";
    let ranges = source
        .match_indices(marker)
        .map(|(start, marker)| start..start + marker.len())
        .collect::<Vec<_>>();
    let mut segments = Vec::new();
    let mut cursor = 0;
    for (occurrence, range) in ranges.into_iter().enumerate() {
        segments.push(InputSegment::text(
            cursor..range.start,
            TextOrigin::Authored,
        ));
        segments.push(InputSegment::generated(GeneratedInput {
            source_range: range.clone(),
            id: SlotId::new(1),
            kind: GeneratedKind::PageLink,
            occurrence: occurrence as u32,
        }));
        cursor = range.end;
    }
    segments.push(InputSegment::text(
        cursor..source.len(),
        TextOrigin::Authored,
    ));
    let input =
        DelayedInput::new(source, segments).expect("valid repeated suppression input");
    assert_eq!(render_input(&input), "<p>“quoted”</p>");

    let source = "runtime-[!-- %%title_linked%% --]-authored";
    let input = page_link_input_with_runtime_prefix(source, "runtime-");
    assert_eq!(
        render_input(&input),
        "<p>runtime--authored</p>",
        "runtime-origin text must remain a seam barrier",
    );

    let source = "authored-[!-- %%title_linked%% --]-runtime";
    let input = page_link_input_with_runtime_suffix(source, "-runtime");
    assert_eq!(
        render_input(&input),
        "<p>authored--runtime</p>",
        "runtime-origin text must remain a seam barrier on either side",
    );
}

#[test]
fn generated_values_do_not_gain_typography_or_general_syntax_authority() {
    let input = page_link_input("left-%%title_linked%%-right");
    let bindings = page_bindings_for(PageRef::page_only("target"), "--[[html]]");
    assert_eq!(
        render_input_with_bindings(&input, &bindings),
        concat!("<p>left-<a href=\"/target\">", "--[[html]]</a>-right</p>",),
    );
}

#[test]
fn delayed_suppression_binding_is_atomic_and_rolls_back_hidden_metadata() {
    let source = concat!(
        "start-[[iftags +missing]]\n",
        "[[html]]<b>hidden</b>[[/html]]\n",
        "%%title_linked%%\n",
        "[[/iftags]]-middle",
    );
    let input = page_link_input(source);
    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
    let delayed = parse_delayed_list(&input, &page_info, &settings)
        .expect("supported delayed input");

    assert_eq!(
        delayed.bind(&SlotBindings::empty()).unwrap_err(),
        crate::delayed::DelayedError::BindingSchemaMismatch,
    );

    let bound = delayed
        .bind(&page_bindings())
        .expect("the same parsed tree must bind after a rejected candidate");
    let sealed = bound.render_html(&page_info, &settings);
    assert_eq!(sealed.body(), "<p>start—middle</p>");
    assert!(sealed.html_blocks().is_empty());
}

#[test]
fn many_adjacent_suppressed_delayed_nodes_resolve_within_a_bounded_budget() {
    const COUNT: usize = 2_048;
    let mut source = String::from("left-");
    let mut generated_ranges = Vec::with_capacity(COUNT);
    for index in 0..COUNT {
        source.push_str("[!-- ");
        let start = source.len();
        source.push_str(&format!("%%slot-{index}%%"));
        generated_ranges.push(start..source.len());
        source.push_str(" --]");
    }
    source.push_str("-right");

    let mut segments = Vec::with_capacity(COUNT * 2 + 1);
    let mut bindings = Vec::with_capacity(COUNT);
    let mut cursor = 0;
    for (index, range) in generated_ranges.into_iter().enumerate() {
        segments.push(InputSegment::text(
            cursor..range.start,
            TextOrigin::Authored,
        ));
        let id = SlotId::new(index as u32);
        segments.push(InputSegment::generated(GeneratedInput {
            source_range: range.clone(),
            id,
            kind: GeneratedKind::PageLink,
            occurrence: 0,
        }));
        bindings.push((
            id,
            GeneratedValue::PageLink {
                page: PageRef::page_only("suppressed"),
                label: Cow::Borrowed("suppressed"),
            },
        ));
        cursor = range.end;
    }
    segments.push(InputSegment::text(
        cursor..source.len(),
        TextOrigin::Authored,
    ));

    let input = DelayedInput::new(&source, segments).expect("valid dense delayed input");
    let bindings = SlotBindings::new(bindings).expect("unique dense bindings");
    let page_info = PageInfo::dummy();
    let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
    let started = Instant::now();
    let delayed = parse_delayed_list(&input, &page_info, &settings)
        .expect("dense delayed input parses");
    let bound = delayed.bind(&bindings).expect("dense delayed input binds");
    let body = bound.render_html(&page_info, &settings);

    assert_eq!(body.body(), "<p>left—right</p>");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "dense delayed suppression exceeded its bounded budget: {:?}",
        started.elapsed(),
    );
}
