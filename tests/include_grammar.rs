use ftml::data::PageRef;
use ftml::includes::{FetchedPage, IncludeRef, Includer};
use ftml::layout::Layout;
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

const PAGE: &str = "run-owned:include-grammar-20260730123701-464";
type VariableSnapshot = BTreeMap<String, String>;
type IncludeSnapshot = (PageRef, VariableSnapshot);
type MatrixCase<'a> = (&'a str, &'a str, Option<Target>, &'a [(&'a str, &'a str)]);

#[derive(Clone, Copy, Debug)]
enum Target {
    Local,
    CrossSite,
    LeadingColon,
}

impl Target {
    fn page_ref(self) -> PageRef {
        match self {
            Self::Local => PageRef::page_only(PAGE),
            Self::CrossSite => PageRef::page_and_site("sandbox-for-codex", PAGE),
            Self::LeadingColon => {
                PageRef::page_and_site("run-owned", "include-grammar-20260730123701-464")
            }
        }
    }
}

#[derive(Debug, Default)]
struct CapturedIncludes(Rc<RefCell<Vec<IncludeSnapshot>>>);

impl CapturedIncludes {
    fn snapshot(&self) -> Vec<IncludeSnapshot> {
        self.0.borrow().clone()
    }
}

impl Clone for CapturedIncludes {
    fn clone(&self) -> Self {
        Self(Rc::clone(&self.0))
    }
}

impl<'t> Includer<'t> for CapturedIncludes {
    type Error = String;

    fn include_pages(
        &mut self,
        includes: &[IncludeRef<'t>],
    ) -> Result<Vec<FetchedPage<'t>>, Self::Error> {
        let mut captured = self.0.borrow_mut();
        for include in includes {
            captured.push((
                include.page_ref().clone(),
                include
                    .variables()
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect(),
            ));
        }

        Ok(includes
            .iter()
            .map(|include| FetchedPage {
                page_ref: include.page_ref().clone(),
                content: Some(Cow::Borrowed("")),
            })
            .collect())
    }

    fn no_such_include(
        &mut self,
        page_ref: &PageRef,
    ) -> Result<Cow<'t, str>, Self::Error> {
        Err(format!("unexpected missing include: {page_ref}"))
    }
}

#[test]
fn live_include_grammar_matrix() {
    let cases: &[MatrixCase<'_>] = &[
        (
            "canonical-no-args",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464]]\nEND",
            Some(Target::Local),
            &[],
        ),
        (
            "canonical-args",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=1|n=2|z=3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "1"), ("z", "3")],
        ),
        (
            "first-arg-no-leading-pipe",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 x=1|n=2|z=3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "1"), ("z", "3")],
        ),
        (
            "target-tight-pipe",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464|x=1|n=2|z=3]]\nEND",
            None,
            &[],
        ),
        (
            "target-tab-pipe",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464\t|x=1|n=2|z=3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "1"), ("z", "3")],
        ),
        (
            "target-newline-pipe",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464\n|x=1\n|n=2\n|z=3\n]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "1"), ("z", "3")],
        ),
        (
            "cross-site",
            "BEGIN\n[[include :sandbox-for-codex:run-owned:include-grammar-20260730123701-464 |x=1|n=2|z=3]]\nEND",
            Some(Target::CrossSite),
            &[("n", "2"), ("x", "1"), ("z", "3")],
        ),
        (
            "target-leading-colon",
            "BEGIN\n[[include :run-owned:include-grammar-20260730123701-464 |x=1|n=2|z=3]]\nEND",
            Some(Target::LeadingColon),
            &[("n", "2"), ("x", "1"), ("z", "3")],
        ),
        (
            "quoted-target",
            "BEGIN\n[[include \"run-owned:include-grammar-20260730123701-464\" |x=1|n=2|z=3]]\nEND",
            None,
            &[],
        ),
        (
            "bare-flag-before",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |FLAG|x=1|n=2|z=3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "1"), ("z", "3")],
        ),
        (
            "bare-flag-middle",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=1|FLAG|n=2|z=3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "1"), ("z", "3")],
        ),
        (
            "bare-flag-after",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=1|n=2|z=3|FLAG]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "1"), ("z", "3")],
        ),
        (
            "multiple-bare-flags",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |A|x=1|B|n=2|C|z=3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "1"), ("z", "3")],
        ),
        (
            "empty-x",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=|n=2|z=3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("z", "3")],
        ),
        (
            "empty-middle",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=1|n=|z=3]]\nEND",
            Some(Target::Local),
            &[("x", "1"), ("z", "3")],
        ),
        (
            "empty-last",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=1|n=2|z=]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "1")],
        ),
        (
            "key-only-x",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x|n=2|z=3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("z", "3")],
        ),
        (
            "key-only-middle",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=1|n|z=3]]\nEND",
            Some(Target::Local),
            &[("x", "1"), ("z", "3")],
        ),
        (
            "unknown-empty",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |unknown=|x=1|n=2|z=3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "1"), ("z", "3")],
        ),
        (
            "unknown-bare",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |unknown|x=1|n=2|z=3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "1"), ("z", "3")],
        ),
        (
            "spaces-around-equals",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x = 1|n = 2|z = 3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "1"), ("z", "3")],
        ),
        (
            "spaces-around-pipe",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 | x=1 | n=2 | z=3 ]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "1"), ("z", "3")],
        ),
        (
            "quoted-values-double",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=\"1\"|n=\"2\"|z=\"3\"]]\nEND",
            Some(Target::Local),
            &[("n", "\"2\""), ("x", "\"1\""), ("z", "\"3\"")],
        ),
        (
            "quoted-values-single",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x='1'|n='2'|z='3']]\nEND",
            Some(Target::Local),
            &[("n", "'2'"), ("x", "'1'"), ("z", "'3'")],
        ),
        (
            "value-with-space",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=A B|n=2|z=3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "A B"), ("z", "3")],
        ),
        (
            "value-leading-space",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x= A|n=2|z=3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "A"), ("z", "3")],
        ),
        (
            "value-trailing-space",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=A |n=2|z=3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "A"), ("z", "3")],
        ),
        (
            "value-double-space",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=A  B|n=2|z=3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "A  B"), ("z", "3")],
        ),
        (
            "value-pipe-quoted",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=\"A|B\"|n=2|z=3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "\"A"), ("z", "3")],
        ),
        (
            "value-brackets",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=[[span]]A[[/span]]|n=2|z=3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "[[span]]A[[/span]]"), ("z", "3")],
        ),
        (
            "value-comment",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=[!--A--]|n=2|z=3]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "[!--A--]"), ("z", "3")],
        ),
        (
            "uppercase-keys",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |X=1|N=2|Z=3]]\nEND",
            Some(Target::Local),
            &[("N", "2"), ("X", "1"), ("Z", "3")],
        ),
        (
            "mixed-keys",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=1|N=2|z=3]]\nEND",
            Some(Target::Local),
            &[("N", "2"), ("x", "1"), ("z", "3")],
        ),
        (
            "hyphen-key",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x-y=1|x=2|n=3|z=4]]\nEND",
            Some(Target::Local),
            &[("n", "3"), ("x", "2"), ("x-y", "1"), ("z", "4")],
        ),
        (
            "underscore-key",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |_x=1|x=2|n=3|z=4]]\nEND",
            Some(Target::Local),
            &[("_x", "1"), ("n", "3"), ("x", "2"), ("z", "4")],
        ),
        (
            "unicode-key",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |日本語=1|x=2|n=3|z=4]]\nEND",
            Some(Target::Local),
            &[("n", "3"), ("x", "2"), ("z", "4")],
        ),
        (
            "duplicate-x-1-2",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=1|x=2|n=3|z=4]]\nEND",
            Some(Target::Local),
            &[("n", "3"), ("x", "1"), ("z", "4")],
        ),
        (
            "duplicate-x-2-1",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=2|x=1|n=3|z=4]]\nEND",
            Some(Target::Local),
            &[("n", "3"), ("x", "2"), ("z", "4")],
        ),
        (
            "empty-then-value",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=|x=2|n=3|z=4]]\nEND",
            Some(Target::Local),
            &[("n", "3"), ("x", "2"), ("z", "4")],
        ),
        (
            "value-then-empty",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=2|x=|n=3|z=4]]\nEND",
            Some(Target::Local),
            &[("n", "3"), ("x", "2"), ("z", "4")],
        ),
        (
            "key-only-then-value",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x|x=2|n=3|z=4]]\nEND",
            Some(Target::Local),
            &[("n", "3"), ("x", "2"), ("z", "4")],
        ),
        (
            "value-then-key-only",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=2|x|n=3|z=4]]\nEND",
            Some(Target::Local),
            &[("n", "3"), ("x", "2"), ("z", "4")],
        ),
        (
            "extra-bracket",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=1|n=2|z=3]]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "1"), ("z", "3]")],
        ),
        (
            "two-extra-brackets",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=1|n=2|z=3]]]]\nEND",
            Some(Target::Local),
            &[("n", "2"), ("x", "1"), ("z", "3]]")],
        ),
        (
            "unclosed",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=1|n=2|z=3\nEND",
            None,
            &[],
        ),
        (
            "leading-space-ownline",
            "BEGIN\n [[include run-owned:include-grammar-20260730123701-464 |x=1|n=2|z=3]]\nEND",
            None,
            &[],
        ),
        (
            "inline-left",
            "BEGIN[[include run-owned:include-grammar-20260730123701-464 |x=1|n=2|z=3]]\nEND",
            None,
            &[],
        ),
        (
            "inline-right",
            "BEGIN\n[[include run-owned:include-grammar-20260730123701-464 |x=1|n=2|z=3]]END",
            None,
            &[],
        ),
        (
            "inline-both",
            "BEGIN[[include run-owned:include-grammar-20260730123701-464 |x=1|n=2|z=3]]END",
            None,
            &[],
        ),
        (
            "quote-prefix",
            "> [[include run-owned:include-grammar-20260730123701-464 |x=1|n=2|z=3]]",
            None,
            &[],
        ),
        (
            "list-prefix",
            "* [[include run-owned:include-grammar-20260730123701-464 |x=1|n=2|z=3]]",
            None,
            &[],
        ),
        (
            "inside-code",
            "[[code]]\n[[include run-owned:include-grammar-20260730123701-464 |x=1|n=2|z=3]]\n[[/code]]",
            Some(Target::Local),
            &[("n", "2"), ("x", "1"), ("z", "3")],
        ),
        (
            "inside-comment",
            "[!-- [[include run-owned:include-grammar-20260730123701-464 |x=1|n=2|z=3]] --]",
            None,
            &[],
        ),
        (
            "inside-raw-inline",
            "@@[[include run-owned:include-grammar-20260730123701-464 |x=1|n=2|z=3]]@@",
            None,
            &[],
        ),
    ];

    assert_eq!(
        cases.len(),
        54,
        "the live evidence matrix must stay complete"
    );

    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    for (case_id, source, target, variables) in cases {
        let captured = CapturedIncludes::default();
        ftml::include(source, &settings, captured.clone(), || {
            format!("invalid include result for {case_id}")
        })
        .unwrap_or_else(|error| panic!("{case_id}: {error}"));

        let expected = target.map(|target| {
            (
                target.page_ref(),
                variables
                    .iter()
                    .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                    .collect(),
            )
        });
        let actual = captured.snapshot();
        assert_eq!(actual.as_slice(), expected.as_slice(), "{case_id}");
    }
}

#[test]
fn live_include_with_nested_span_caption_is_consumed_whole() {
    let source = concat!(
        "[[include component:image-block name=scary.jpg",
        "|caption=[[span style=\"font-family: 'Schoolbell';  font-size: 20px;\"]]",
        "where it hides[[/span]]]]\n",
    );
    let expected_caption = concat!(
        "[[span style=\"font-family: 'Schoolbell';  font-size: 20px;\"]]",
        "where it hides[[/span]]",
    );

    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let captured = CapturedIncludes::default();
    let (output, _) = ftml::include(source, &settings, captured.clone(), || {
        "invalid include result for nested-span-caption".to_owned()
    })
    .expect("nested-span caption include should parse");

    let actual = captured.snapshot();
    assert_eq!(actual.len(), 1, "the whole line is one include: {actual:?}");
    assert_eq!(
        actual[0].0,
        PageRef::page_only("component:image-block"),
        "include target: {actual:?}",
    );
    assert_eq!(
        actual[0].1.get("caption").map(String::as_str),
        Some(expected_caption),
        "caption keeps the nested block intact: {actual:?}",
    );
    assert_eq!(
        actual[0].1.get("name").map(String::as_str),
        Some("scary.jpg"),
        "other arguments still parse: {actual:?}",
    );
    assert!(
        !output.contains("[[") && !output.contains("]]"),
        "no leftover literal include text: {output:?}",
    );
}
