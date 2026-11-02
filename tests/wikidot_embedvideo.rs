use ftml::data::{PageInfo, ScoreValue};
use ftml::delayed::{
    DelayedInput, GeneratedInput, GeneratedKind, GeneratedValue, InputSegment,
    SlotBindings, SlotId, TextOrigin, parse_delayed_list,
};
use ftml::layout::Layout;
use ftml::parsing::ParseErrorKind;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::{Element, SyntaxTree};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::HashSet;
use std::time::{Duration, Instant};

const LIVE_EVIDENCE: &str =
    include_str!("fixtures/wikidot-embedvideo-live-20260807.json");

fn page_info() -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("embedvideo-evidence"),
        category: Some(Cow::Borrowed("test")),
        site: Cow::Borrowed("scp-wiki"),
        title: Cow::Borrowed("EmbedVideo evidence"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    }
}

fn render(source: &str, layout: Layout) -> ftml::render::html::HtmlOutput {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let tokenization = ftml::tokenize(source);
    let (tree, _errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    HtmlRender.render(&tree, &page_info, &settings)
}

fn embed_requirements(
    output: &ftml::render::html::HtmlOutput,
) -> Vec<&ftml::render::html::EmbedVideoRequirement> {
    output
        .resource_requirements
        .iter()
        .filter_map(|requirement| requirement.embed_video_requirement())
        .collect()
}

fn sha256_hex(source: &str) -> String {
    use std::fmt::Write;

    Sha256::digest(source.as_bytes()).iter().fold(
        String::with_capacity(64),
        |mut output, byte| {
            write!(&mut output, "{byte:02x}").expect("write to String");
            output
        },
    )
}

#[test]
fn embedvideo_is_a_static_typed_render_boundary() {
    let source = "[[embedvideo]]https://youtu.be/dQw4w9WgXcQ[[/embedvideo]]";
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = ftml::tokenize(source);
    let (tree, errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    assert!(errors.is_empty(), "{errors:#?}");

    let serialized = serde_json::to_string(&tree).expect("serializable syntax tree");
    assert!(serialized.contains("embed-video"), "{serialized}");

    let output = HtmlRender.render(&tree, &page_info, &settings);
    assert!(output.body.contains("wj-embed-video"), "{}", output.body);
    assert!(!output.body.contains("youtu.be"), "{}", output.body);
    let requirement = embed_requirements(&output)[0];
    let serialized_requirement =
        serde_json::to_value(requirement).expect("serializable requirement");
    assert_eq!(serialized_requirement["embed_video"]["source"], source);
    assert_eq!(
        serialized_requirement["embed_video"]["source-sha256"],
        "3be49772375abf702a321e021373d8bbd79bc872ca301a9326218e97a6cccd73",
    );
}

#[test]
fn all_34_live_backed_cases_preserve_the_opaque_owner_boundary() {
    let fixture: Value = serde_json::from_str(LIVE_EVIDENCE).expect("valid evidence");
    assert_eq!(fixture["schema"], "ftml.wikidot-embedvideo-evidence.v1");
    assert_eq!(
        fixture["provenance"]["archived_capture"]["authenticated"],
        false
    );
    assert_eq!(fixture["provenance"]["archived_capture"]["mutated"], false);
    assert_eq!(
        fixture["provenance"]["supplemental_capture"]["authenticated"],
        false
    );
    assert_eq!(
        fixture["provenance"]["supplemental_capture"]["mutated"],
        false
    );

    let cases = fixture["cases"].as_array().expect("case array");
    assert_eq!(cases.len(), 34);
    let inactive: HashSet<&str> = [
        "focused-embedvideo-code",
        "focused-embedvideo-corpus-1-doc-wiki-syntax:embedding",
        "focused-embedvideo-corpus-2-fam-radio-season-02",
        "focused-embedvideo-escaped",
        "focused-embedvideo-head",
        "focused-embedvideo-unclosed",
        "focused-embedvideo-unknown-head",
    ]
    .into_iter()
    .collect();

    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let source = case["source"].as_str().expect("source");
        let expected_source_hash = case["source_sha256"].as_str().expect("source hash");
        let actual_source_hash = sha256_hex(source);
        assert_eq!(
            actual_source_hash, expected_source_hash,
            "{id}: source provenance"
        );
        assert_eq!(case["raw_html_sha256"].as_str().unwrap().len(), 64, "{id}");

        let output = render(source, Layout::Wikidot);
        let requirements = embed_requirements(&output);
        let expected_count = if id == "focused-embedvideo-adjacent" {
            2
        } else {
            usize::from(!inactive.contains(id))
        };
        assert_eq!(requirements.len(), expected_count, "{id}: {}", output.body);

        for requirement in requirements {
            let embed_video = requirement.embed_video();
            assert!(source.contains(embed_video.source()), "{id}");
            assert_eq!(
                embed_video.source_sha256().to_hex(),
                sha256_hex(embed_video.source()),
                "{id}: owner identity",
            );
            assert!(requirement.id().starts_with("wj-embed-video-"), "{id}");
        }
        if expected_count > 0 {
            assert!(!output.body.contains("<iframe"), "{id}: {}", output.body);
            assert!(!output.body.contains("<script"), "{id}: {}", output.body);
            assert!(
                !output.body.contains("javascript:"),
                "{id}: {}",
                output.body
            );
            assert!(
                !output.body.contains("data:text/html"),
                "{id}: {}",
                output.body
            );
        }
    }
}

#[test]
fn exact_activation_payload_and_recovery_are_typed() {
    for source in [
        "[[embedvideo]]X[[/embedvideo]]",
        "[[EMBEDVIDEO]]X[[/EmBeDvIdEo]]",
        "before [[embedvideo]]X[[/wrong]] after",
        "[[embedvideo]]\r\n雪🦀\r\n[[/EMBEDVIDEO]]",
    ] {
        assert_eq!(
            embed_requirements(&render(source, Layout::Wikidot)).len(),
            1,
            "{source:?}"
        );
    }

    let wikijump = render("[[embedvideo]]X[[/embedvideo]]", Layout::Wikijump);
    assert!(embed_requirements(&wikijump).is_empty());
    assert_eq!(wikijump.body, "<p>[[embedvideo]]X[[/embedvideo]]</p>",);
    for source in [
        "[[embedvideos]]X[[/embedvideos]]",
        "[[ embedvideo]]X[[/embedvideo]]",
        "[[embedvideo ]]X[[/embedvideo]]",
        "[[ｅｍｂｅｄｖｉｄｅｏ]]X[[/ｅｍｂｅｄｖｉｄｅｏ]]",
        "[[embedvideo]]X",
    ] {
        assert!(
            embed_requirements(&render(source, Layout::Wikidot)).is_empty(),
            "{source:?}"
        );
    }

    for (source, expected_payload) in [
        ("[[embedvideo]][[/embedvideo]]", ""),
        ("[[embedvideo]]\n\n[[/embedvideo]]", "\n\n"),
        ("[[embedvideo]]]X[[/embedvideo]]", "]X"),
    ] {
        let output = render(source, Layout::Wikidot);
        let requirement = embed_requirements(&output)[0];
        assert_eq!(requirement.embed_video().source(), source);
        assert_eq!(requirement.embed_video().payload(), expected_payload);
    }

    let nested = "[[embedvideo]]A[[embedvideo]]B[[/inner]]C[[/outer]]";
    let output = render(nested, Layout::Wikidot);
    let requirement = embed_requirements(&output)[0];
    assert_eq!(
        requirement.embed_video().source(),
        "[[embedvideo]]A[[embedvideo]]B[[/inner]]"
    );
    assert_eq!(requirement.embed_video().payload(), "A[[embedvideo]]B");
    assert!(output.body.contains("C[[/outer]]"), "{}", output.body);

    let adjacent = render(
        "[[embedvideo]]A[[/x]][[EMBEDVIDEO]]B[[/y]]",
        Layout::Wikidot,
    );
    assert_eq!(embed_requirements(&adjacent).len(), 2);

    let extra_close = render("[[embedvideo]]X[[/embedvideo]]]Y", Layout::Wikidot);
    assert_eq!(embed_requirements(&extra_close).len(), 1);
    assert!(
        extra_close.body.contains("</div>]<p>Y</p>"),
        "{}",
        extra_close.body
    );
}

#[test]
fn head_trailing_whitespace_before_line_break_opens_the_block() {
    let iframe = "<iframe src=\"https://www.youtube.com/embed/dQw4w9WgXcQ\"></iframe>";
    for (source, expected_payload) in [
        (
            format!("[[embedvideo]] \n{iframe}\n[[/embedvideo]]"),
            format!(" \n{iframe}\n"),
        ),
        (
            "[[embedvideo]]\t\nX\n[[/embedvideo]]".to_string(),
            "\t\nX\n".to_string(),
        ),
        (
            "[[embedvideo]]  \nX\n[[/embedvideo]]".to_string(),
            "  \nX\n".to_string(),
        ),
        (
            "[[embedvideo]]] \nX\n[[/embedvideo]]".to_string(),
            "] \nX\n".to_string(),
        ),
    ] {
        let output = render(&source, Layout::Wikidot);
        let requirements = embed_requirements(&output);
        assert_eq!(requirements.len(), 1, "{source:?}: {}", output.body);
        assert_eq!(requirements[0].embed_video().source(), source);
        assert_eq!(requirements[0].embed_video().payload(), expected_payload);
    }

    for source in [
        "[[embedvideo]] X[[/embedvideo]]".to_string(),
        "[[embedvideo]] ... [[/embedvideo]]".to_string(),
        format!("[[embedvideo]] {iframe}[[/embedvideo]]"),
    ] {
        let output = render(&source, Layout::Wikidot);
        assert!(
            embed_requirements(&output).is_empty(),
            "{source:?}: {}",
            output.body
        );
    }
}

#[test]
fn literal_owners_exclude_embedvideo_syntax() {
    for source in [
        "[[code]]\n[[embedvideo]]X[[/embedvideo]]\n[[/code]]",
        "@@[[embedvideo]]X[[/embedvideo]]@@",
        "[!-- [[embedvideo]]X[[/embedvideo]] --]",
    ] {
        assert!(
            embed_requirements(&render(source, Layout::Wikidot)).is_empty(),
            "{source:?}"
        );
    }

    let source = "[[html]]<div>[[embedvideo]]X[[/embedvideo]]</div>[[/html]]";
    assert!(
        embed_requirements(&render(source, Layout::Wikidot)).is_empty(),
        "disabled hosted HTML must retain literal ownership",
    );
    for layout in [Layout::Wikidot, Layout::Wikijump] {
        let page_info = page_info();
        let mut settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
        settings.enable_html_blocks = true;
        let tokenization = ftml::tokenize(source);
        let (tree, _errors) = ftml::parse(&tokenization, &page_info, &settings).into();
        let output = HtmlRender.render(&tree, &page_info, &settings);
        assert!(embed_requirements(&output).is_empty(), "{}", output.body);
    }
}

#[test]
fn typed_owner_serializes_and_deep_owns_exact_identity() {
    let source = "[[embedvideo]]\r\n雪🦀\r\n[[/embedvideo]]";
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let tokenization = ftml::tokenize(source);
    let (tree, errors): (SyntaxTree<'_>, _) =
        ftml::parse(&tokenization, &page_info, &settings).into();
    assert!(errors.is_empty(), "{errors:#?}");
    let owned = tree.to_owned();
    drop(tokenization);

    let Element::EmbedVideo(embed_video) = &owned.elements[0] else {
        panic!("expected EmbedVideo: {:#?}", owned.elements);
    };
    assert_eq!(embed_video.source(), source);
    assert_eq!(embed_video.payload(), "\r\n雪🦀\r\n");
    assert_eq!(embed_video.source_sha256().to_hex().len(), 64);

    let serialized = serde_json::to_string(&owned).expect("serialize owned tree");
    let round_trip: SyntaxTree<'_> =
        serde_json::from_str(&serialized).expect("deserialize tree");
    assert_eq!(round_trip.elements, owned.elements);

    let mut forged = serde_json::to_value(&owned).expect("tree value");
    forged["elements"][0]["data"]["source-sha256"] = Value::String("0".repeat(64));
    assert!(
        serde_json::from_value::<SyntaxTree<'_>>(forged).is_err(),
        "deserialization must reject a forged source identity",
    );

    let mut non_ascii = serde_json::to_value(&owned).expect("tree value");
    non_ascii["elements"][0]["data"]["source-sha256"] = Value::String("é".repeat(32));
    assert!(
        serde_json::from_value::<SyntaxTree<'_>>(non_ascii).is_err(),
        "non-ASCII digest text must be rejected without panicking",
    );
}

#[test]
fn authored_payload_is_inert_across_delayed_provenance_boundaries() {
    let authored =
        "[[embedvideo]]<iframe src=\"https://example.com\"></iframe>[[/embedvideo]]";
    let input = DelayedInput::new(
        authored,
        vec![InputSegment::text(0..authored.len(), TextOrigin::Authored)],
    )
    .expect("valid authored input");
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::List, Layout::Wikidot);
    let delayed =
        parse_delayed_list(&input, &page_info, &settings).expect("parse authored owner");
    let bound = delayed
        .bind(&SlotBindings::empty())
        .expect("bind authored owner");
    let fragment = bound.render_html(&page_info, &settings);
    assert_eq!(
        fragment
            .resource_requirements()
            .iter()
            .filter(|item| item.embed_video_requirement().is_some())
            .count(),
        1,
    );
    assert!(!fragment.body().contains("iframe"), "{}", fragment.body());

    for (source, runtime) in [
        (
            "[[embedvideo]]javascript:alert(1)[[/embedvideo]]",
            "javascript:alert(1)",
        ),
        ("[[embedvideo]]X[[/embedvideo]]", "embedvideo"),
        ("[[embedvideo]]X[[/embedvideo]]", "[[embedvideo]]"),
        ("[[embedvideo]]X[[/embedvideo]]", "[[/embedvideo]]"),
    ] {
        let start = source.find(runtime).unwrap();
        let end = start + runtime.len();
        let input = DelayedInput::new(
            source,
            vec![
                InputSegment::text(0..start, TextOrigin::Authored),
                InputSegment::text(start..end, TextOrigin::RuntimeScalar),
                InputSegment::text(end..source.len(), TextOrigin::Authored),
            ],
        )
        .expect("runtime segments");
        let delayed = parse_delayed_list(&input, &page_info, &settings)
            .expect("parse runtime input");
        let bound = delayed
            .bind(&SlotBindings::empty())
            .expect("bind runtime input");
        assert!(
            bound
                .render_html(&page_info, &settings)
                .resource_requirements()
                .iter()
                .all(|item| item.embed_video_requirement().is_none()),
            "runtime fragment {runtime:?} became authority",
        );
    }

    let source = "[[embedvideo]]%%title_linked%%[[/embedvideo]]";
    let marker = "%%title_linked%%";
    let start = source.find(marker).unwrap();
    let end = start + marker.len();
    let input = DelayedInput::new(
        source,
        vec![
            InputSegment::text(0..start, TextOrigin::Authored),
            InputSegment::generated(GeneratedInput {
                source_range: start..end,
                id: SlotId::new(332),
                kind: GeneratedKind::PageLink,
                occurrence: 0,
            }),
            InputSegment::text(end..source.len(), TextOrigin::Authored),
        ],
    )
    .expect("generated segments");
    let delayed =
        parse_delayed_list(&input, &page_info, &settings).expect("parse generated input");
    let bindings = SlotBindings::new(vec![(
        SlotId::new(332),
        GeneratedValue::PageLink {
            page: ftml::data::PageRef::page_only("provider"),
            label: Cow::Borrowed("<iframe src=javascript:alert(1)></iframe>"),
        },
    )])
    .expect("bindings");
    let bound = delayed.bind(&bindings).expect("bind generated input");
    assert!(
        bound
            .render_html(&page_info, &settings)
            .resource_requirements()
            .iter()
            .all(|item| item.embed_video_requirement().is_none()),
    );
}

#[test]
fn malformed_recovery_reports_stable_error_kinds_and_linear_work() {
    let page_info = page_info();
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    for (source, expected, expected_span) in [
        ("[[embedvideo]]X", ParseErrorKind::BlockExpectedEnd, 14..15),
        (
            "[[embedvideo value=x]]X[[/embedvideo]]",
            ParseErrorKind::BlockMalformedArguments,
            13..18,
        ),
    ] {
        let tokenization = ftml::tokenize(source);
        let outcome = ftml::parse(&tokenization, &page_info, &settings);
        let error = outcome
            .errors()
            .iter()
            .find(|error| error.kind() == expected)
            .unwrap_or_else(|| panic!("{source:?}: {:#?}", outcome.errors()));
        assert_eq!(error.rule(), "block-embedvideo");
        assert_eq!(error.span(), expected_span);
        assert!(
            embed_requirements(&render(source, Layout::Wikidot)).is_empty(),
            "{source:?}: {:#?}",
            outcome.errors(),
        );
    }
    assert_eq!(
        render("[[embedvideo]]X", Layout::Wikidot).body,
        "<p>[[embedvideo]]X</p>",
    );

    let source = "[[embedvideo]]".repeat(512);
    let started = Instant::now();
    let output = render(&source, Layout::Wikidot);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert!(embed_requirements(&output).is_empty());
}
