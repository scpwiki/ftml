use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use ftml::tree::{Element, ListItem, PartialElement};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn render(source: &str) -> String {
    let page_info = PageInfo {
        page: Cow::Borrowed("footnote-first"),
        category: None,
        site: Cow::Borrowed("syntax-differential"),
        title: Cow::Borrowed("Footnote-first line ownership"),
        alt_title: None,
        score: ScoreValue::Integer(0),
        tags: Vec::new(),
        language: Cow::Borrowed("en"),
    };
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let mut source = source.to_owned();
    ftml::preprocess_for_layout(&mut source, settings.layout);
    let tokenization = ftml::tokenize(&source);
    let (tree, _errors) = ftml::parse(&tokenization, &page_info, &settings).into();
    assert!(!contains_empty_owner(&tree.elements), "{tree:#?}");
    HtmlRender.render(&tree, &page_info, &settings).body
}

fn contains_empty_owner(elements: &[Element<'_>]) -> bool {
    elements.iter().any(|element| match element {
        Element::Partial(PartialElement::WikidotEmptyInlineOwner) => true,
        Element::Container(container) => contains_empty_owner(container.elements()),
        Element::Table(table) => table.rows.iter().any(|row| {
            row.cells
                .iter()
                .any(|cell| contains_empty_owner(&cell.elements))
        }),
        Element::TabView(tabs) => tabs
            .iter()
            .any(|item| contains_empty_owner(item.elements())),
        Element::List { items, .. } => items.iter().any(|item| match item {
            ListItem::Elements { elements, .. } => contains_empty_owner(elements),
            ListItem::SubList { element } => {
                contains_empty_owner(std::slice::from_ref(element))
            }
        }),
        Element::DefinitionList(items) => items.iter().any(|item| {
            contains_empty_owner(&item.key_elements)
                || contains_empty_owner(&item.value_elements)
        }),
        Element::Anchor { elements, .. }
        | Element::Collapsible { elements, .. }
        | Element::Color { elements, .. }
        | Element::Include { elements, .. } => contains_empty_owner(elements),
        _ => false,
    })
}

#[test]
fn wikidot_footnote_first_matrix_matches_live_dom() {
    for (case_id, source, expected) in [
        (
            r#"scout-line-owner-footnote-quote-direct"#,
            r#"> [[footnote]]N[[/footnote]]"#,
            r#"<div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote-comment"#,
            r#"> [!--x--][[footnote]]N[[/footnote]]"#,
            r#"<div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote-raw-empty"#,
            r#"> @@@@[[footnote]]N[[/footnote]]"#,
            r#"<blockquote><p><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote-bold-empty"#,
            r#"> ****[[footnote]]N[[/footnote]]"#,
            r#"<blockquote><p><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote-span-empty"#,
            r#"> [[span]][[/span]][[footnote]]N[[/footnote]]"#,
            r#"<blockquote><p><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote-space"#,
            r#">  [[footnote]]N[[/footnote]]"#,
            r#"<div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote-text"#,
            r#"> A[[footnote]]N[[/footnote]]"#,
            r#"<blockquote><p>A<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote-raw-text"#,
            r#"> @@A@@[[footnote]]N[[/footnote]]"#,
            r#"<blockquote><p><span style="white-space: pre-wrap;">A</span><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote-bold-text"#,
            r#"> **A**[[footnote]]N[[/footnote]]"#,
            r#"<blockquote><p><strong>A</strong><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote-span-text"#,
            r#"> [[span]]A[[/span]][[footnote]]N[[/footnote]]"#,
            r#"<blockquote><p><span>A</span><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote-nextline-empty"#,
            r#"> 
[[footnote]]N[[/footnote]]"#,
            r#"<div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote-nextline-marked"#,
            r#"> A
> [[footnote]]N[[/footnote]]"#,
            r#"<blockquote><p>A</p></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote-nextline-unmarked"#,
            r#"> A
[[footnote]]N[[/footnote]]"#,
            r#"<blockquote><p>A<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote2-direct"#,
            r#">> [[footnote]]N[[/footnote]]"#,
            r#"<div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote2-comment"#,
            r#">> [!--x--][[footnote]]N[[/footnote]]"#,
            r#"<div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote2-raw-empty"#,
            r#">> @@@@[[footnote]]N[[/footnote]]"#,
            r#"<blockquote><blockquote><p><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p></blockquote></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote2-bold-empty"#,
            r#">> ****[[footnote]]N[[/footnote]]"#,
            r#"<blockquote><blockquote><p><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p></blockquote></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote2-span-empty"#,
            r#">> [[span]][[/span]][[footnote]]N[[/footnote]]"#,
            r#"<blockquote><blockquote><p><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p></blockquote></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote2-space"#,
            r#">>  [[footnote]]N[[/footnote]]"#,
            r#"<div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote2-text"#,
            r#">> A[[footnote]]N[[/footnote]]"#,
            r#"<blockquote><blockquote><p>A<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p></blockquote></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote2-raw-text"#,
            r#">> @@A@@[[footnote]]N[[/footnote]]"#,
            r#"<blockquote><blockquote><p><span style="white-space: pre-wrap;">A</span><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p></blockquote></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote2-bold-text"#,
            r#">> **A**[[footnote]]N[[/footnote]]"#,
            r#"<blockquote><blockquote><p><strong>A</strong><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p></blockquote></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote2-span-text"#,
            r#">> [[span]]A[[/span]][[footnote]]N[[/footnote]]"#,
            r#"<blockquote><blockquote><p><span>A</span><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p></blockquote></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote2-nextline-empty"#,
            r#">> 
[[footnote]]N[[/footnote]]"#,
            r#"<div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote2-nextline-marked"#,
            r#">> A
>> [[footnote]]N[[/footnote]]"#,
            r#"<blockquote><blockquote><p>A</p></blockquote></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-quote2-nextline-unmarked"#,
            r#">> A
[[footnote]]N[[/footnote]]"#,
            r#"<blockquote><blockquote><p>A<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p></blockquote></blockquote><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul-direct"#,
            r#"* [[footnote]]N[[/footnote]]"#,
            r#"<p>*<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul-comment"#,
            r#"* [!--x--][[footnote]]N[[/footnote]]"#,
            r#"<p>*<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul-raw-empty"#,
            r#"* @@@@[[footnote]]N[[/footnote]]"#,
            r#"<ul>
<li><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></li>
</ul><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul-bold-empty"#,
            r#"* ****[[footnote]]N[[/footnote]]"#,
            r#"<ul>
<li><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></li>
</ul><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul-span-empty"#,
            r#"* [[span]][[/span]][[footnote]]N[[/footnote]]"#,
            r#"<ul>
<li><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></li>
</ul><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul-space"#,
            r#"*  [[footnote]]N[[/footnote]]"#,
            r#"<p>*<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul-text"#,
            r#"* A[[footnote]]N[[/footnote]]"#,
            r#"<ul>
<li>A<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></li>
</ul><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul-raw-text"#,
            r#"* @@A@@[[footnote]]N[[/footnote]]"#,
            r#"<ul>
<li><span style="white-space: pre-wrap;">A</span><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></li>
</ul><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul-bold-text"#,
            r#"* **A**[[footnote]]N[[/footnote]]"#,
            r#"<ul>
<li><strong>A</strong><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></li>
</ul><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul-span-text"#,
            r#"* [[span]]A[[/span]][[footnote]]N[[/footnote]]"#,
            r#"<ul>
<li><span>A</span><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></li>
</ul><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul-nextline-empty"#,
            r#"* 
[[footnote]]N[[/footnote]]"#,
            r#"<p>*<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul-nextline-marked"#,
            r#"* A
* [[footnote]]N[[/footnote]]"#,
            r#"<ul>
<li>A</li>
</ul><p>*<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul-nextline-unmarked"#,
            r#"* A
[[footnote]]N[[/footnote]]"#,
            r#"<ul>
<li>A<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></li>
</ul><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul2-direct"#,
            r#"** [[footnote]]N[[/footnote]]"#,
            r#"<p>**<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul2-comment"#,
            r#"** [!--x--][[footnote]]N[[/footnote]]"#,
            r#"<p>**<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul2-raw-empty"#,
            r#"** @@@@[[footnote]]N[[/footnote]]"#,
            r#"<p>** <sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul2-bold-empty"#,
            r#"** ****[[footnote]]N[[/footnote]]"#,
            r#"<p>** <sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul2-span-empty"#,
            r#"** [[span]][[/span]][[footnote]]N[[/footnote]]"#,
            r#"<p>** <sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul2-space"#,
            r#"**  [[footnote]]N[[/footnote]]"#,
            r#"<p>**<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul2-text"#,
            r#"** A[[footnote]]N[[/footnote]]"#,
            r#"<p>** A<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul2-raw-text"#,
            r#"** @@A@@[[footnote]]N[[/footnote]]"#,
            r#"<p>** <span style="white-space: pre-wrap;">A</span><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul2-bold-text"#,
            r#"** **A**[[footnote]]N[[/footnote]]"#,
            r#"<p>** <strong>A</strong><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul2-span-text"#,
            r#"** [[span]]A[[/span]][[footnote]]N[[/footnote]]"#,
            r#"<p>** <span>A</span><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul2-nextline-empty"#,
            r#"** 
[[footnote]]N[[/footnote]]"#,
            r#"<p>**<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul2-nextline-marked"#,
            r#"** A
** [[footnote]]N[[/footnote]]"#,
            r#"<p>** A<br>
**<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ul2-nextline-unmarked"#,
            r#"** A
[[footnote]]N[[/footnote]]"#,
            r#"<p>** A<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol-direct"#,
            r#"# [[footnote]]N[[/footnote]]"#,
            r#"<p>#<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol-comment"#,
            r#"# [!--x--][[footnote]]N[[/footnote]]"#,
            r#"<p>#<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol-raw-empty"#,
            r#"# @@@@[[footnote]]N[[/footnote]]"#,
            r#"<ol>
<li><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></li>
</ol><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol-bold-empty"#,
            r#"# ****[[footnote]]N[[/footnote]]"#,
            r#"<ol>
<li><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></li>
</ol><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol-span-empty"#,
            r#"# [[span]][[/span]][[footnote]]N[[/footnote]]"#,
            r#"<ol>
<li><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></li>
</ol><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol-space"#,
            r#"#  [[footnote]]N[[/footnote]]"#,
            r#"<p>#<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol-text"#,
            r#"# A[[footnote]]N[[/footnote]]"#,
            r#"<ol>
<li>A<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></li>
</ol><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol-raw-text"#,
            r#"# @@A@@[[footnote]]N[[/footnote]]"#,
            r#"<ol>
<li><span style="white-space: pre-wrap;">A</span><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></li>
</ol><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol-bold-text"#,
            r#"# **A**[[footnote]]N[[/footnote]]"#,
            r#"<ol>
<li><strong>A</strong><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></li>
</ol><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol-span-text"#,
            r#"# [[span]]A[[/span]][[footnote]]N[[/footnote]]"#,
            r#"<ol>
<li><span>A</span><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></li>
</ol><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol-nextline-empty"#,
            r#"# 
[[footnote]]N[[/footnote]]"#,
            r#"<p>#<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol-nextline-marked"#,
            r#"# A
# [[footnote]]N[[/footnote]]"#,
            r#"<ol>
<li>A</li>
</ol><p>#<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol-nextline-unmarked"#,
            r#"# A
[[footnote]]N[[/footnote]]"#,
            r#"<ol>
<li>A<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></li>
</ol><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol2-direct"#,
            r#"## [[footnote]]N[[/footnote]]"#,
            r#"<p>##<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol2-comment"#,
            r#"## [!--x--][[footnote]]N[[/footnote]]"#,
            r#"<p>##<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol2-raw-empty"#,
            r#"## @@@@[[footnote]]N[[/footnote]]"#,
            r#"<p>## <sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol2-bold-empty"#,
            r#"## ****[[footnote]]N[[/footnote]]"#,
            r#"<p>## <sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol2-span-empty"#,
            r#"## [[span]][[/span]][[footnote]]N[[/footnote]]"#,
            r#"<p>## <sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol2-space"#,
            r#"##  [[footnote]]N[[/footnote]]"#,
            r#"<p>##<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol2-text"#,
            r#"## A[[footnote]]N[[/footnote]]"#,
            r#"<p>## A<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol2-raw-text"#,
            r#"## @@A@@[[footnote]]N[[/footnote]]"#,
            r#"<p>## <span style="white-space: pre-wrap;">A</span><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol2-bold-text"#,
            r#"## **A**[[footnote]]N[[/footnote]]"#,
            r#"<p>## <strong>A</strong><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol2-span-text"#,
            r#"## [[span]]A[[/span]][[footnote]]N[[/footnote]]"#,
            r#"<p>## <span>A</span><sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol2-nextline-empty"#,
            r#"## 
[[footnote]]N[[/footnote]]"#,
            r#"<p>##<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol2-nextline-marked"#,
            r#"## A
## [[footnote]]N[[/footnote]]"#,
            r#"<p>## A<br>
##<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
        (
            r#"scout-line-owner-footnote-ol2-nextline-unmarked"#,
            r#"## A
[[footnote]]N[[/footnote]]"#,
            r#"<p>## A<sup class="footnoteref"><a id="footnoteref-1" href="javascript:;" class="footnoteref" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnote-1&#39;)">1</a></sup></p><div class="footnotes-footer"><div class="title">Footnotes</div><div class="footnote-footer" id="footnote-1"><a href="javascript:;" onclick="WIKIDOT.page.utils.scrollToReference(&#39;footnoteref-1&#39;)">1</a>. N</div></div>"#,
        ),
    ] {
        assert_eq!(render(source), expected, "{case_id}: {source:?}");
    }
}

#[test]
fn fallback_references_keep_global_numbering_and_backlinks() {
    let html = render(concat!(
        "> [[footnote]]quoted[[/footnote]]\n",
        "* [[footnote]]listed[[/footnote]]\n",
        "plain[[footnote]]ordinary[[/footnote]]",
    ));

    assert!(!html.contains("<blockquote>"), "{html}");
    assert!(html.contains("<p>*<sup class=\"footnoteref\">"), "{html}");
    assert!(!html.contains("id=\"footnoteref-1\""), "{html}");
    for index in 2..=3 {
        assert!(
            html.contains(&format!("id=\"footnoteref-{index}\"")),
            "{html}"
        );
    }
    for index in 1..=3 {
        assert!(html.contains(&format!("id=\"footnote-{index}\"")), "{html}");
        assert!(
            html.contains(&format!("scrollToReference(&#39;footnoteref-{index}&#39;)")),
            "{html}"
        );
    }
    assert!(html.contains("1</a>. quoted"), "{html}");
    assert!(html.contains("2</a>. listed"), "{html}");
    assert!(html.contains("3</a>. ordinary"), "{html}");
}

#[test]
fn explicit_footnote_block_is_not_duplicated_after_fallback() {
    let html = render(concat!(
        "> [[footnote]]quoted[[/footnote]]\n",
        "* [[footnote]]listed[[/footnote]]\n",
        "[[footnoteblock title=\"Notes\"]]",
    ));

    assert_eq!(
        html.matches("class=\"footnotes-footer\"").count(),
        1,
        "{html}"
    );
    assert!(html.contains("<div class=\"title\">Notes</div>"), "{html}");
    assert_eq!(
        html.matches("class=\"footnote-footer\"").count(),
        2,
        "{html}"
    );
}

#[test]
fn long_invisible_prefixes_before_footnotes_stay_bounded() {
    let comments = "[!--x--]".repeat(4_096);
    let source = format!(
        "> {comments}[[footnote]]quoted[[/footnote]]\n* {comments}[[footnote]]listed[[/footnote]]",
    );
    let started = Instant::now();
    let html = render(&source);
    let elapsed = started.elapsed();

    assert!(!html.contains("<blockquote>"), "{html}");
    assert!(!html.contains("<ul>"), "{html}");
    assert_eq!(
        html.matches("class=\"footnote-footer\"").count(),
        2,
        "{html}"
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "footnote-first prefix scan took {elapsed:?}",
    );
}
