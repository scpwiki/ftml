use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, text::TextRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;

const T: &str = "-zz-no-such-tag";
const F: &str = "+zz-no-such-tag";

fn page_info(tags: Vec<Cow<'static, str>>) -> PageInfo<'static> {
    PageInfo {
        page: Cow::Borrowed("iftags-pairing"),
        category: None,
        site: Cow::Borrowed("scp-wiki"),
        title: Cow::Borrowed("Iftags Pairing"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags,
        language: Cow::Borrowed("en"),
    }
}

fn render_layout(source: &str, layout: Layout, tags: Vec<Cow<'static, str>>) -> String {
    let page_info = page_info(tags);
    let settings = WikitextSettings::from_mode(WikitextMode::Page, layout);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, _errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    TextRender
        .render(&tree, &page_info, &settings)
        .trim()
        .to_owned()
}

fn render(source: &str) -> String {
    render_layout(source, Layout::Wikidot, Vec::new())
}

#[test]
fn single_pairs_unchanged() {
    assert_eq!(render(&format!("a[[iftags {T}]]b[[/iftags]]c")), "abc");
    assert_eq!(render(&format!("a[[iftags {F}]]b[[/iftags]]c")), "ac");
}

#[test]
fn two_openers_one_closer_pairs_first_opener() {
    assert_eq!(
        render(&format!("x[[iftags {T}]]a[[iftags {T}]]b[[/iftags]]c")),
        format!("xa[[iftags {T}]]bc"),
    );
}

#[test]
fn nested_pair_is_jumped_and_stays_literal() {
    assert_eq!(
        render(&format!(
            "x[[iftags {T}]]a[[iftags {T}]]b[[/iftags]]c[[/iftags]]y"
        )),
        format!("xa[[iftags {T}]]b[[/iftags]]cy"),
    );
}

#[test]
fn nested_pair_inner_condition_is_not_evaluated() {
    assert_eq!(
        render(&format!(
            "x[[iftags {T}]]a[[iftags {F}]]b[[/iftags]]c[[/iftags]]y"
        )),
        format!("xa[[iftags {F}]]b[[/iftags]]cy"),
    );
}

#[test]
fn false_outer_hides_the_whole_jumped_span() {
    assert_eq!(
        render(&format!(
            "x[[iftags {F}]]a[[iftags {T}]]b[[/iftags]]c[[/iftags]]y"
        )),
        "xy",
    );
}

#[test]
fn pairing_backtracks_when_jumping_dead_ends() {
    assert_eq!(
        render(&format!(
            "x[[iftags {T}]]a[[iftags {T}]]b[[iftags {T}]]c[[/iftags]]d[[/iftags]]"
        )),
        format!("xa[[iftags {T}]]b[[iftags {T}]]c[[/iftags]]d"),
    );
    assert_eq!(
        render(&format!(
            "x[[iftags {F}]]a[[iftags {T}]]b[[iftags {T}]]c[[/iftags]]d[[/iftags]]"
        )),
        "x",
    );
}

#[test]
fn cross_paragraph_pairing() {
    assert_eq!(
        render(&format!(
            "x[[iftags {T}]]abc\n\n[[iftags {T}]]def[[/iftags]]"
        )),
        format!("xabc\n\n[[iftags {T}]]def"),
    );
    assert_eq!(
        render(&format!(
            "x[[iftags {T}]]abc\n\n[[iftags {F}]]def[[/iftags]]ghi"
        )),
        format!("xabc\n\n[[iftags {F}]]defghi"),
    );
}

#[test]
fn cross_paragraph_nested_pair_is_jumped() {
    let text = render(&format!(
        "[[iftags {T}]]a\n\n[[iftags {T}]]b\n\n[[/iftags]]c[[/iftags]]"
    ));
    assert_eq!(
        text.replace("\n\n", "\n"),
        format!("a\n[[iftags {T}]]b\n[[/iftags]]c"),
    );
}

#[test]
fn false_first_opener_hides_span_across_paragraphs() {
    assert_eq!(
        render(&format!(
            "x[[iftags {F}]]abc\n\n[[iftags {T}]]def[[/iftags]]"
        )),
        "x",
    );
}

#[test]
fn stray_closer_satisfies_unclosed_opener() {
    let text = render(&format!(
        "x[[iftags {T}]]abc\n\nmid[[/iftags]]end\n\n[[iftags {T}]]def[[/iftags]]"
    ));
    assert_eq!(text.replace("\n\n", "\n"), "xabc\nmidend\ndef");
}

#[test]
fn sequential_pairs_pair_independently() {
    assert_eq!(
        render(&format!(
            "[[iftags {T}]]a[[/iftags]]m[[iftags {F}]]b[[/iftags]]"
        )),
        "am",
    );
    assert_eq!(
        render(&format!(
            "[[iftags {T}]]a[[iftags {T}]]b[[/iftags]]c[[iftags {T}]]d[[/iftags]]e"
        )),
        format!("a[[iftags {T}]]b[[/iftags]]c[[iftags {T}]]de"),
    );
}

#[test]
fn triple_opener_one_closer() {
    assert_eq!(
        render(&format!(
            "[[iftags {T}]]a[[iftags {T}]]b[[iftags {T}]]c[[/iftags]]d"
        )),
        format!("a[[iftags {T}]]b[[iftags {T}]]cd"),
    );
}

#[test]
fn unclosed_opener_and_stray_closer_stay_literal() {
    assert_eq!(
        render(&format!("x[[iftags {T}]]abc")),
        format!("x[[iftags {T}]]abc"),
    );
    assert_eq!(
        render(&format!("[[iftags {T}]]a[[/iftags]]t[[iftags {T}]]u")),
        format!("at[[iftags {T}]]u"),
    );
    assert_eq!(render("b[[/iftags]]c"), "b[[/iftags]]c");
    assert_eq!(
        render(&format!("[[/iftags]]x[[iftags {T}]]a[[/iftags]]")),
        "[[/iftags]]xa",
    );
}

#[test]
fn closer_inside_code_block_does_not_pair() {
    let text = render(&format!(
        "[[iftags {T}]]a\n[[code]]\n[[/iftags]]\n[[/code]]\nb[[/iftags]]"
    ));
    assert_eq!(text.matches("[[/iftags]]").count(), 1, "text: {text}");
    assert!(text.contains('a'), "text: {text}");
    assert!(text.contains('b'), "text: {text}");
}

#[test]
fn iftags_inside_code_block_is_literal() {
    let text = render(&format!("[[code]]\n[[iftags {T}]]x[[/iftags]]\n[[/code]]"));
    assert!(
        text.contains(&format!("[[iftags {T}]]x[[/iftags]]")),
        "text: {text}",
    );
}

#[test]
fn closer_inside_raw_span_does_not_pair() {
    assert_eq!(
        render(&format!("[[iftags {T}]]a@@[[/iftags]]@@b[[/iftags]]")),
        "a[[/iftags]]b",
    );
}

#[test]
fn bare_iftags_never_keeps() {
    assert_eq!(render("[[iftags]]body[[/iftags]]"), "");
    assert_eq!(
        render_layout(
            "[[iftags]]body[[/iftags]]",
            Layout::Wikidot,
            vec![Cow::Borrowed("essay")],
        ),
        "",
    );
}

#[test]
fn false_pair_keeps_trailing_token_literal() {
    let fruit = vec![Cow::Borrowed("fruit")];
    assert_eq!(
        render_layout(
            "[[iftags -fruit]]\n[[iftags -fruit]]\n[[/iftags]]\ntext4",
            Layout::Wikidot,
            fruit.clone(),
        ),
        "text4",
    );
    assert_eq!(
        render_layout(
            "[[iftags -fruit]]\n[[iftags -fruit]]\n[[/iftags]]\n[[iftags -fruit]]",
            Layout::Wikidot,
            fruit.clone(),
        ),
        "[[iftags -fruit]]",
    );
    assert_eq!(
        render_layout(
            "[[iftags -fruit]]\n[[iftags +other]]\n[[/iftags]]\n[[iftags -fruit]]",
            Layout::Wikidot,
            fruit.clone(),
        ),
        "[[iftags -fruit]]",
    );
    assert_eq!(
        render_layout(
            "[[iftags +other]]\n[[iftags +other]]\n[[/iftags]]\ntext4",
            Layout::Wikidot,
            fruit,
        ),
        "text4",
    );
}

#[test]
fn native_layout_pairs_the_same_way() {
    assert_eq!(
        render_layout(
            &format!("x[[iftags {T}]]a[[iftags {T}]]b[[/iftags]]c[[/iftags]]y"),
            Layout::Wikijump,
            Vec::new(),
        ),
        format!("xa[[iftags {T}]]b[[/iftags]]cy"),
    );
    assert_eq!(
        render_layout(
            &format!("x[[iftags {F}]]a[[iftags {T}]]b[[/iftags]]c[[/iftags]]y"),
            Layout::Wikijump,
            Vec::new(),
        ),
        "xy",
    );
    assert_eq!(
        render_layout(
            &format!("[[iftags {F}]]a[[iftags {T}]]b[[/iftags]]d"),
            Layout::Wikijump,
            Vec::new(),
        ),
        "d",
    );
}
