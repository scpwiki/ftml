use ftml::data::{PageInfo, ScoreValue};
use ftml::layout::Layout;
use ftml::render::{Render, html::HtmlRender};
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::time::{Duration, Instant};

fn render(source: &str) -> String {
    let page_info = PageInfo {
        page: Cow::Borrowed("syntax-differential"),
        category: None,
        site: Cow::Borrowed("syntax-differential"),
        title: Cow::Borrowed("Automatic link differential"),
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
    HtmlRender.render(&tree, &page_info, &settings).body
}

#[test]
fn wikidot_automatic_link_matrix_matches_live_boundaries() {
    for (case_id, source, expected) in [
        (
            "scout-autolink-bare-plain",
            "BEGIN|http://example.com/path",
            "<p>BEGIN|<a href=\"http://example.com/path\">http://example.com/path</a></p>",
        ),
        (
            "scout-autolink-bare-paren",
            "BEGIN|(http://example.com/path)",
            "<p>BEGIN|(<a href=\"http://example.com/path\">http://example.com/path</a>)</p>",
        ),
        (
            "scout-autolink-bare-bracket",
            "BEGIN|[http://example.com/path]",
            "<p>BEGIN|[<a href=\"http://example.com/path\">http://example.com/path</a>]</p>",
        ),
        (
            "scout-autolink-bare-quote",
            "BEGIN|\"http://example.com/path\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path\">http://example.com/path</a>&quot;</p>",
        ),
        (
            "scout-autolink-pipe-plain",
            "BEGIN|http://example.com/path|END",
            "<p>BEGIN|<a href=\"http://example.com/path|END\">http://example.com/path|END</a></p>",
        ),
        (
            "scout-autolink-pipe-paren",
            "BEGIN|(http://example.com/path|END)",
            "<p>BEGIN|(<a href=\"http://example.com/path|END\">http://example.com/path|END</a>)</p>",
        ),
        (
            "scout-autolink-pipe-bracket",
            "BEGIN|[http://example.com/path|END]",
            "<p>BEGIN|[<a href=\"http://example.com/path|END\">http://example.com/path|END</a>]</p>",
        ),
        (
            "scout-autolink-pipe-quote",
            "BEGIN|\"http://example.com/path|END\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path|END\">http://example.com/path|END</a>&quot;</p>",
        ),
        (
            "scout-autolink-comma-plain",
            "BEGIN|http://example.com/path,END",
            "<p>BEGIN|<a href=\"http://example.com/path,END\">http://example.com/path,END</a></p>",
        ),
        (
            "scout-autolink-comma-paren",
            "BEGIN|(http://example.com/path,END)",
            "<p>BEGIN|(<a href=\"http://example.com/path,END\">http://example.com/path,END</a>)</p>",
        ),
        (
            "scout-autolink-comma-bracket",
            "BEGIN|[http://example.com/path,END]",
            "<p>BEGIN|[<a href=\"http://example.com/path,END\">http://example.com/path,END</a>]</p>",
        ),
        (
            "scout-autolink-comma-quote",
            "BEGIN|\"http://example.com/path,END\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path,END\">http://example.com/path,END</a>&quot;</p>",
        ),
        (
            "scout-autolink-period-plain",
            "BEGIN|http://example.com/path.END",
            "<p>BEGIN|<a href=\"http://example.com/path.END\">http://example.com/path.END</a></p>",
        ),
        (
            "scout-autolink-period-paren",
            "BEGIN|(http://example.com/path.END)",
            "<p>BEGIN|(<a href=\"http://example.com/path.END\">http://example.com/path.END</a>)</p>",
        ),
        (
            "scout-autolink-period-bracket",
            "BEGIN|[http://example.com/path.END]",
            "<p>BEGIN|[<a href=\"http://example.com/path.END\">http://example.com/path.END</a>]</p>",
        ),
        (
            "scout-autolink-period-quote",
            "BEGIN|\"http://example.com/path.END\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path.END\">http://example.com/path.END</a>&quot;</p>",
        ),
        (
            "scout-autolink-semicolon-plain",
            "BEGIN|http://example.com/path;END",
            "<p>BEGIN|<a href=\"http://example.com/path;END\">http://example.com/path;END</a></p>",
        ),
        (
            "scout-autolink-semicolon-paren",
            "BEGIN|(http://example.com/path;END)",
            "<p>BEGIN|(<a href=\"http://example.com/path;END\">http://example.com/path;END</a>)</p>",
        ),
        (
            "scout-autolink-semicolon-bracket",
            "BEGIN|[http://example.com/path;END]",
            "<p>BEGIN|[<a href=\"http://example.com/path;END\">http://example.com/path;END</a>]</p>",
        ),
        (
            "scout-autolink-semicolon-quote",
            "BEGIN|\"http://example.com/path;END\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path;END\">http://example.com/path;END</a>&quot;</p>",
        ),
        (
            "scout-autolink-colon-plain",
            "BEGIN|http://example.com/path:END",
            "<p>BEGIN|<a href=\"http://example.com/path:END\">http://example.com/path:END</a></p>",
        ),
        (
            "scout-autolink-colon-paren",
            "BEGIN|(http://example.com/path:END)",
            "<p>BEGIN|(<a href=\"http://example.com/path:END\">http://example.com/path:END</a>)</p>",
        ),
        (
            "scout-autolink-colon-bracket",
            "BEGIN|[http://example.com/path:END]",
            "<p>BEGIN|[<a href=\"http://example.com/path:END\">http://example.com/path:END</a>]</p>",
        ),
        (
            "scout-autolink-colon-quote",
            "BEGIN|\"http://example.com/path:END\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path:END\">http://example.com/path:END</a>&quot;</p>",
        ),
        (
            "scout-autolink-rparen-plain",
            "BEGIN|http://example.com/path)END",
            "<p>BEGIN|<a href=\"http://example.com/path)END\">http://example.com/path)END</a></p>",
        ),
        (
            "scout-autolink-rparen-paren",
            "BEGIN|(http://example.com/path)END)",
            "<p>BEGIN|(<a href=\"http://example.com/path)END\">http://example.com/path)END</a>)</p>",
        ),
        (
            "scout-autolink-rparen-bracket",
            "BEGIN|[http://example.com/path)END]",
            "<p>BEGIN|[<a href=\"http://example.com/path)END\">http://example.com/path)END</a>]</p>",
        ),
        (
            "scout-autolink-rparen-quote",
            "BEGIN|\"http://example.com/path)END\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path)END\">http://example.com/path)END</a>&quot;</p>",
        ),
        (
            "scout-autolink-rbracket-plain",
            "BEGIN|http://example.com/path]END",
            "<p>BEGIN|<a href=\"http://example.com/path]END\">http://example.com/path]END</a></p>",
        ),
        (
            "scout-autolink-rbracket-paren",
            "BEGIN|(http://example.com/path]END)",
            "<p>BEGIN|(<a href=\"http://example.com/path]END\">http://example.com/path]END</a>)</p>",
        ),
        (
            "scout-autolink-rbracket-bracket",
            "BEGIN|[http://example.com/path]END]",
            "<p>BEGIN|[<a href=\"http://example.com/path]END\">http://example.com/path]END</a>]</p>",
        ),
        (
            "scout-autolink-rbracket-quote",
            "BEGIN|\"http://example.com/path]END\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path]END\">http://example.com/path]END</a>&quot;</p>",
        ),
        (
            "scout-autolink-rbrace-plain",
            "BEGIN|http://example.com/path}END",
            "<p>BEGIN|<a href=\"http://example.com/path}END\">http://example.com/path}END</a></p>",
        ),
        (
            "scout-autolink-rbrace-paren",
            "BEGIN|(http://example.com/path}END)",
            "<p>BEGIN|(<a href=\"http://example.com/path}END\">http://example.com/path}END</a>)</p>",
        ),
        (
            "scout-autolink-rbrace-bracket",
            "BEGIN|[http://example.com/path}END]",
            "<p>BEGIN|[<a href=\"http://example.com/path}END\">http://example.com/path}END</a>]</p>",
        ),
        (
            "scout-autolink-rbrace-quote",
            "BEGIN|\"http://example.com/path}END\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path}END\">http://example.com/path}END</a>&quot;</p>",
        ),
        (
            "scout-autolink-quote-plain",
            "BEGIN|http://example.com/path\"END",
            "<p>BEGIN|<a href=\"http://example.com/path\">http://example.com/path</a>&quot;END</p>",
        ),
        (
            "scout-autolink-quote-paren",
            "BEGIN|(http://example.com/path\"END)",
            "<p>BEGIN|(<a href=\"http://example.com/path\">http://example.com/path</a>&quot;END)</p>",
        ),
        (
            "scout-autolink-quote-bracket",
            "BEGIN|[http://example.com/path\"END]",
            "<p>BEGIN|[<a href=\"http://example.com/path\">http://example.com/path</a>&quot;END]</p>",
        ),
        (
            "scout-autolink-quote-quote",
            "BEGIN|\"http://example.com/path\"END\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path\">http://example.com/path</a>&quot;END&quot;</p>",
        ),
        (
            "scout-autolink-apostrophe-plain",
            "BEGIN|http://example.com/path'END",
            "<p>BEGIN|<a href=\"http://example.com/path\">http://example.com/path</a>&#39;END</p>",
        ),
        (
            "scout-autolink-apostrophe-paren",
            "BEGIN|(http://example.com/path'END)",
            "<p>BEGIN|(<a href=\"http://example.com/path\">http://example.com/path</a>&#39;END)</p>",
        ),
        (
            "scout-autolink-apostrophe-bracket",
            "BEGIN|[http://example.com/path'END]",
            "<p>BEGIN|[<a href=\"http://example.com/path\">http://example.com/path</a>&#39;END]</p>",
        ),
        (
            "scout-autolink-apostrophe-quote",
            "BEGIN|\"http://example.com/path'END\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path\">http://example.com/path</a>&#39;END&quot;</p>",
        ),
        (
            "scout-autolink-angle-plain",
            "BEGIN|http://example.com/path<END",
            "<p>BEGIN|<a href=\"http://example.com/path&lt;END\">http://example.com/path&lt;END</a></p>",
        ),
        (
            "scout-autolink-angle-paren",
            "BEGIN|(http://example.com/path<END)",
            "<p>BEGIN|(<a href=\"http://example.com/path&lt;END\">http://example.com/path&lt;END</a>)</p>",
        ),
        (
            "scout-autolink-angle-bracket",
            "BEGIN|[http://example.com/path<END]",
            "<p>BEGIN|[<a href=\"http://example.com/path&lt;END\">http://example.com/path&lt;END</a>]</p>",
        ),
        (
            "scout-autolink-angle-quote",
            "BEGIN|\"http://example.com/path<END\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path&lt;END\">http://example.com/path&lt;END</a>&quot;</p>",
        ),
        (
            "scout-autolink-space-plain",
            "BEGIN|http://example.com/path END",
            "<p>BEGIN|<a href=\"http://example.com/path\">http://example.com/path</a> END</p>",
        ),
        (
            "scout-autolink-space-paren",
            "BEGIN|(http://example.com/path END)",
            "<p>BEGIN|(<a href=\"http://example.com/path\">http://example.com/path</a> END)</p>",
        ),
        (
            "scout-autolink-space-bracket",
            "BEGIN|[http://example.com/path END]",
            "<p>BEGIN|<a href=\"http://example.com/path\">END</a></p>",
        ),
        (
            "scout-autolink-space-quote",
            "BEGIN|\"http://example.com/path END\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path\">http://example.com/path</a> END&quot;</p>",
        ),
        (
            "scout-autolink-newline-plain",
            "BEGIN|http://example.com/path\nEND",
            "<p>BEGIN|<a href=\"http://example.com/path\">http://example.com/path</a><br>\nEND</p>",
        ),
        (
            "scout-autolink-newline-paren",
            "BEGIN|(http://example.com/path\nEND)",
            "<p>BEGIN|(<a href=\"http://example.com/path\">http://example.com/path</a><br>\nEND)</p>",
        ),
        (
            "scout-autolink-newline-bracket",
            "BEGIN|[http://example.com/path\nEND]",
            "<p>BEGIN|[<a href=\"http://example.com/path\">http://example.com/path</a><br>\nEND]</p>",
        ),
        (
            "scout-autolink-newline-quote",
            "BEGIN|\"http://example.com/path\nEND\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path\">http://example.com/path</a><br>\nEND&quot;</p>",
        ),
        (
            "scout-autolink-tab-plain",
            "BEGIN|http://example.com/path\tEND",
            "<p>BEGIN|<a href=\"http://example.com/path\">http://example.com/path</a> END</p>",
        ),
        (
            "scout-autolink-tab-paren",
            "BEGIN|(http://example.com/path\tEND)",
            "<p>BEGIN|(<a href=\"http://example.com/path\">http://example.com/path</a> END)</p>",
        ),
        (
            "scout-autolink-tab-bracket",
            "BEGIN|[http://example.com/path\tEND]",
            "<p>BEGIN| <a href=\"http://example.com/path\">END</a></p>",
        ),
        (
            "scout-autolink-tab-quote",
            "BEGIN|\"http://example.com/path\tEND\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path\">http://example.com/path</a> END&quot;</p>",
        ),
        (
            "scout-autolink-unicode-plain",
            "BEGIN|http://example.com/path雪END",
            "<p>BEGIN|<a href=\"http://example.com/path%E9%9B%AAEND\">http://example.com/path雪END</a></p>",
        ),
        (
            "scout-autolink-unicode-paren",
            "BEGIN|(http://example.com/path雪END)",
            "<p>BEGIN|(<a href=\"http://example.com/path%E9%9B%AAEND\">http://example.com/path雪END</a>)</p>",
        ),
        (
            "scout-autolink-unicode-bracket",
            "BEGIN|[http://example.com/path雪END]",
            "<p>BEGIN|[<a href=\"http://example.com/path%E9%9B%AAEND\">http://example.com/path雪END</a>]</p>",
        ),
        (
            "scout-autolink-unicode-quote",
            "BEGIN|\"http://example.com/path雪END\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path%E9%9B%AAEND\">http://example.com/path雪END</a>&quot;</p>",
        ),
        (
            "scout-autolink-hash-plain",
            "BEGIN|http://example.com/path#Frag|END",
            "<p>BEGIN|<a href=\"http://example.com/path#Frag|END\">http://example.com/path#Frag|END</a></p>",
        ),
        (
            "scout-autolink-hash-paren",
            "BEGIN|(http://example.com/path#Frag|END)",
            "<p>BEGIN|(<a href=\"http://example.com/path#Frag|END\">http://example.com/path#Frag|END</a>)</p>",
        ),
        (
            "scout-autolink-hash-bracket",
            "BEGIN|[http://example.com/path#Frag|END]",
            "<p>BEGIN|[<a href=\"http://example.com/path#Frag|END\">http://example.com/path#Frag|END</a>]</p>",
        ),
        (
            "scout-autolink-hash-quote",
            "BEGIN|\"http://example.com/path#Frag|END\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path#Frag|END\">http://example.com/path#Frag|END</a>&quot;</p>",
        ),
        (
            "scout-autolink-query-plain",
            "BEGIN|http://example.com/path?Q=X|END",
            "<p>BEGIN|<a href=\"http://example.com/path?Q=X|END\">http://example.com/path?Q=X|END</a></p>",
        ),
        (
            "scout-autolink-query-paren",
            "BEGIN|(http://example.com/path?Q=X|END)",
            "<p>BEGIN|(<a href=\"http://example.com/path?Q=X|END\">http://example.com/path?Q=X|END</a>)</p>",
        ),
        (
            "scout-autolink-query-bracket",
            "BEGIN|[http://example.com/path?Q=X|END]",
            "<p>BEGIN|[<a href=\"http://example.com/path?Q=X|END\">http://example.com/path?Q=X|END</a>]</p>",
        ),
        (
            "scout-autolink-query-quote",
            "BEGIN|\"http://example.com/path?Q=X|END\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path?Q=X|END\">http://example.com/path?Q=X|END</a>&quot;</p>",
        ),
        (
            "scout-autolink-amp-plain",
            "BEGIN|http://example.com/path&A=B|END",
            "<p>BEGIN|<a href=\"http://example.com/path&amp;A=B|END\">http://example.com/path&amp;A=B|END</a></p>",
        ),
        (
            "scout-autolink-amp-paren",
            "BEGIN|(http://example.com/path&A=B|END)",
            "<p>BEGIN|(<a href=\"http://example.com/path&amp;A=B|END\">http://example.com/path&amp;A=B|END</a>)</p>",
        ),
        (
            "scout-autolink-amp-bracket",
            "BEGIN|[http://example.com/path&A=B|END]",
            "<p>BEGIN|[<a href=\"http://example.com/path&amp;A=B|END\">http://example.com/path&amp;A=B|END</a>]</p>",
        ),
        (
            "scout-autolink-amp-quote",
            "BEGIN|\"http://example.com/path&A=B|END\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path&amp;A=B|END\">http://example.com/path&amp;A=B|END</a>&quot;</p>",
        ),
        (
            "scout-autolink-encoded-plain",
            "BEGIN|http://example.com/path%7CEND",
            "<p>BEGIN|<a href=\"http://example.com/path%7CEND\">http://example.com/path%7CEND</a></p>",
        ),
        (
            "scout-autolink-encoded-paren",
            "BEGIN|(http://example.com/path%7CEND)",
            "<p>BEGIN|(<a href=\"http://example.com/path%7CEND\">http://example.com/path%7CEND</a>)</p>",
        ),
        (
            "scout-autolink-encoded-bracket",
            "BEGIN|[http://example.com/path%7CEND]",
            "<p>BEGIN|[<a href=\"http://example.com/path%7CEND\">http://example.com/path%7CEND</a>]</p>",
        ),
        (
            "scout-autolink-encoded-quote",
            "BEGIN|\"http://example.com/path%7CEND\"",
            "<p>BEGIN|&quot;<a href=\"http://example.com/path%7CEND\">http://example.com/path%7CEND</a>&quot;</p>",
        ),
        (
            "scout-autolink-https-pipe-plain",
            "BEGIN|https://Example.COM/Path|END",
            "<p>BEGIN|<a href=\"https://Example.COM/Path|END\">https://Example.COM/Path|END</a></p>",
        ),
        (
            "scout-autolink-https-pipe-paren",
            "BEGIN|(https://Example.COM/Path|END)",
            "<p>BEGIN|(<a href=\"https://Example.COM/Path|END\">https://Example.COM/Path|END</a>)</p>",
        ),
        (
            "scout-autolink-https-pipe-bracket",
            "BEGIN|[https://Example.COM/Path|END]",
            "<p>BEGIN|[<a href=\"https://Example.COM/Path|END\">https://Example.COM/Path|END</a>]</p>",
        ),
        (
            "scout-autolink-https-pipe-quote",
            "BEGIN|\"https://Example.COM/Path|END\"",
            "<p>BEGIN|&quot;<a href=\"https://Example.COM/Path|END\">https://Example.COM/Path|END</a>&quot;</p>",
        ),
        (
            "scout-autolink-protocol-pipe-plain",
            "BEGIN|//example.com/path|END",
            "<p>BEGIN|//example.com/path|END</p>",
        ),
        (
            "scout-autolink-protocol-pipe-paren",
            "BEGIN|(//example.com/path|END)",
            "<p>BEGIN|(//example.com/path|END)</p>",
        ),
        (
            "scout-autolink-protocol-pipe-bracket",
            "BEGIN|[//example.com/path|END]",
            "<p>BEGIN|[//example.com/path|END]</p>",
        ),
        (
            "scout-autolink-protocol-pipe-quote",
            "BEGIN|\"//example.com/path|END\"",
            "<p>BEGIN|&quot;//example.com/path|END&quot;</p>",
        ),
        (
            "scout-autolink-mailto-pipe-plain",
            "BEGIN|mailto:User@example.com|END",
            "<p>BEGIN|<a href=\"mailto:User@example.com|END\">mailto:User@example.com|END</a></p>",
        ),
        (
            "scout-autolink-mailto-pipe-paren",
            "BEGIN|(mailto:User@example.com|END)",
            "<p>BEGIN|(<a href=\"mailto:User@example.com|END\">mailto:User@example.com|END</a>)</p>",
        ),
        (
            "scout-autolink-mailto-pipe-bracket",
            "BEGIN|[mailto:User@example.com|END]",
            "<p>BEGIN|[<a href=\"mailto:User@example.com|END\">mailto:User@example.com|END</a>]</p>",
        ),
        (
            "scout-autolink-mailto-pipe-quote",
            "BEGIN|\"mailto:User@example.com|END\"",
            "<p>BEGIN|&quot;<a href=\"mailto:User@example.com|END\">mailto:User@example.com|END</a>&quot;</p>",
        ),
        (
            "scout-autolink-www-pipe-plain",
            "BEGIN|www.example.com/path|END",
            "<p>BEGIN|www.example.com/path|END</p>",
        ),
        (
            "scout-autolink-www-pipe-paren",
            "BEGIN|(www.example.com/path|END)",
            "<p>BEGIN|(www.example.com/path|END)</p>",
        ),
        (
            "scout-autolink-www-pipe-bracket",
            "BEGIN|[www.example.com/path|END]",
            "<p>BEGIN|[www.example.com/path|END]</p>",
        ),
        (
            "scout-autolink-www-pipe-quote",
            "BEGIN|\"www.example.com/path|END\"",
            "<p>BEGIN|&quot;www.example.com/path|END&quot;</p>",
        ),
    ] {
        assert_eq!(render(source), expected, "{case_id}: {source:?}");
    }
}

#[test]
fn automatic_links_do_not_steal_structural_or_literal_owners() {
    for (source, expected) in [
        (
            "[[[https://example.com/a|Label]]]",
            "<p><a href=\"https://example.com/a\">Label</a></p>",
        ),
        (
            "[https://example.com/a|b Label]",
            "<p><a href=\"https://example.com/a|b\">Label</a></p>",
        ),
        (
            "[[code]]\nhttp://example.com/path|END\n[[/code]]",
            "<div class=\"code\"><pre><code>http://example.com/path|END</code></pre></div>",
        ),
        (
            "@@http://example.com/path|END@@",
            "<p><span style=\"white-space: pre-wrap;\">http://example.com/path|END</span></p>",
        ),
        ("A[!--http://example.com/path|END--]B", "<p>AB</p>"),
        (
            "|| http://example.com/path|| X ||",
            "<table class=\"wiki-content-table\">\n<tr>\n<td><a href=\"http://example.com/path\">http://example.com/path</a></td>\n<td>X</td>\n</tr>\n</table>",
        ),
    ] {
        assert_eq!(render(source), expected, "{source:?}");
    }
}

#[test]
fn wikidot_email_stops_at_its_last_domain_label() {
    let email = |local: &str, domain: &str| {
        let reversed = |part: &str| part.chars().rev().collect::<String>();
        let obfuscated = format!("{}|{}", reversed(domain), reversed(local));
        format!("<span class=\"wiki-email\">{obfuscated}#{obfuscated}</span>")
    };
    let cases = [
        (
            "email a@b.gov.",
            format!("<p>email {}.</p>", email("a", "b.gov")),
        ),
        (
            "x a@b.gov.. y",
            format!("<p>x {}.. y</p>", email("a", "b.gov")),
        ),
        (
            "x a@b.gov, y",
            format!("<p>x {}, y</p>", email("a", "b.gov")),
        ),
        (
            "x (a@b.gov) y",
            format!("<p>x ({}) y</p>", email("a", "b.gov")),
        ),
        (
            "x a@b.gov/y z",
            format!("<p>x {}/y z</p>", email("a", "b.gov")),
        ),
        (
            "x a@b.c.gov. y",
            format!("<p>x {}. y</p>", email("a", "b.c.gov")),
        ),
        (
            "x a@b.gov- y",
            format!("<p>x {} y</p>", email("a", "b.gov-")),
        ),
        (
            "x a_b@c.gov y",
            format!("<p>x {} y</p>", email("a_b", "c.gov")),
        ),
        (
            "x a.b@c.gov y",
            format!("<p>x {} y</p>", email("a.b", "c.gov")),
        ),
        (
            "x a+b@c.gov y",
            format!("<p>x a+{} y</p>", email("b", "c.gov")),
        ),
        (
            "x a..b@c.gov y",
            format!("<p>x a..{} y</p>", email("b", "c.gov")),
        ),
        (
            "x a%b@c.gov y",
            format!("<p>x a%{} y</p>", email("b", "c.gov")),
        ),
        ("x a@b_c.gov y", String::from("<p>x a@b_c.gov y</p>")),
        ("x a@b. y", String::from("<p>x a@b. y</p>")),
        ("x a@日本.gov y", String::from("<p>x a@日本.gov y</p>")),
    ];

    for (source, expected) in cases {
        assert_eq!(render(source), expected, "{source}");
    }
}

#[test]
fn wikidot_email_and_mailto_have_distinct_authority() {
    assert_eq!(
        render("support@wikidot.com"),
        "<p><span class=\"wiki-email\">moc.todikiw|troppus#moc.todikiw|troppus</span></p>",
    );
    assert_eq!(
        render("BEGIN|mailto:User@example.com|END"),
        "<p>BEGIN|<a href=\"mailto:User@example.com|END\">mailto:User@example.com|END</a></p>",
    );
}

#[test]
fn repeated_automatic_link_boundaries_stay_bounded() {
    let unit = concat!(
        "BEGIN|(http://example.com/path|END) ",
        "BEGIN|[https://example.com/path\tLABEL] ",
        "mailto:User@example.com|END ",
        "support@wikidot.com\n",
    );
    let source = unit.repeat(2_048);
    let started = Instant::now();
    let html = render(&source);
    let elapsed = started.elapsed();

    assert_eq!(html.matches("http://example.com/path|END").count(), 4_096);
    assert_eq!(html.matches("https://example.com/path").count(), 2_048);
    assert_eq!(html.matches("mailto:User@example.com|END").count(), 4_096);
    assert_eq!(
        html.matches("moc.todikiw|troppus#moc.todikiw|troppus")
            .count(),
        2_048,
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "automatic-link boundary scan took {elapsed:?}",
    );
}
