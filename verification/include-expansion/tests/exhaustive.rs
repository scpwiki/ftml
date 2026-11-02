
use ftml::data::PageRef;
use ftml::includes::{FetchedPage, IncludeRef, Includer, include};
use ftml::layout::Layout;
use ftml::settings::{WikitextMode, WikitextSettings};
use std::borrow::Cow;
use std::convert::Infallible;

const MARKER: &str = "\u{a7}";

#[derive(Clone, Copy, Debug)]
enum Kind {
    Missing,
    Empty,
    Marker,
    Variables,
}

struct Probe {
    kind: Kind,
    asked: usize,
}

impl<'t> Includer<'t> for Probe {
    type Error = Infallible;

    fn include_pages(
        &mut self,
        includes: &[IncludeRef<'t>],
    ) -> Result<Vec<FetchedPage<'t>>, Infallible> {
        self.asked = includes.len();
        Ok(includes
            .iter()
            .map(|include| FetchedPage {
                page_ref: include.page_ref().clone(),
                content: match self.kind {
                    Kind::Missing => None,
                    Kind::Empty => Some(Cow::Borrowed("")),
                    Kind::Marker => Some(Cow::Borrowed(MARKER)),
                    Kind::Variables => Some(Cow::Owned("{$a}|{$b}|{$a".to_string())),
                },
            })
            .collect())
    }

    fn no_such_include(&mut self, _page_ref: &PageRef) -> Result<Cow<'t, str>, Infallible> {
        Ok(Cow::Borrowed(""))
    }
}

const TOKENS: &[&str] = &[
    "[[include", "[[", "]]", " ", "page", ":", "|", "a=b", "=", ">", "> ", "--", "\u{e9}",
    "\u{1f980}", "\n", "\r\n", "[[/include", "~",
];

fn run(input: &str, kind: Kind) -> Result<(String, Vec<PageRef>, usize), String> {
    let settings = WikitextSettings::from_mode(WikitextMode::Page, Layout::Wikidot);
    let probe = Probe { kind, asked: 0 };
    let asked = std::cell::Cell::new(0usize);
    struct Counting<'a>(Probe, &'a std::cell::Cell<usize>);
    impl<'t, 'a> Includer<'t> for Counting<'a> {
        type Error = Infallible;
        fn include_pages(
            &mut self,
            includes: &[IncludeRef<'t>],
        ) -> Result<Vec<FetchedPage<'t>>, Infallible> {
            let pages = self.0.include_pages(includes)?;
            self.1.set(includes.len());
            Ok(pages)
        }
        fn no_such_include(&mut self, page_ref: &PageRef) -> Result<Cow<'t, str>, Infallible> {
            self.0.no_such_include(page_ref)
        }
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        include(input, &settings, Counting(probe, &asked), || unreachable!())
    }));
    match result {
        Err(_) => Err("panicked".to_string()),
        Ok(Err(never)) => match never {},
        Ok(Ok((out, pages))) => Ok((out, pages, asked.get())),
    }
}

fn describe(input: &str) -> String {
    format!("{input:?}")
}

fn check(input: &str) -> Option<String> {
    let missing = match run(input, Kind::Missing) {
        Ok(v) => v,
        Err(e) => return Some(format!("{}: {e} (missing-page includer)", describe(input))),
    };
    let (miss_out, miss_pages, asked) = &missing;
    if miss_pages.len() != *asked {
        return Some(format!(
            "{}: asked for {asked} includes but returned {} page refs",
            describe(input),
            miss_pages.len()
        ));
    }
    if *asked == 0 && !input.contains('>') && miss_out != input {
        return Some(format!("{}: no include block asked for, yet text changed to {}", describe(input), describe(miss_out)));
    }
    {
        let mut rest = miss_out.chars().peekable();
        for c in input.chars() {
            if rest.peek() == Some(&c) {
                rest.next();
            }
        }
        if rest.peek().is_some() {
            return Some(format!("{}: missing-page output {} is not input text with parts deleted", describe(input), describe(miss_out)));
        }
    }
    for kind in [Kind::Empty, Kind::Marker, Kind::Variables] {
        let (out, pages, n) = match run(input, kind) {
            Ok(v) => v,
            Err(e) => return Some(format!("{}: {e} ({kind:?} includer)", describe(input))),
        };
        if n != *asked || pages != *miss_pages {
            return Some(format!("{}: {kind:?} includer saw different includes than the missing-page includer", describe(input)));
        }
        match kind {
            Kind::Empty if &out != miss_out => {
                return Some(format!("{}: empty page gives {} but missing page gives {}", describe(input), describe(&out), describe(miss_out)));
            }
            Kind::Marker if !input.contains('\n') && !input.contains('\r') => {
                let stripped: String = out.chars().filter(|c| *c != '\n' && *c != '\r' && *c != '\u{a7}').collect();
                let base: String = miss_out.chars().filter(|c| *c != '\n' && *c != '\r').collect();
                if stripped != base {
                    return Some(format!("{}: marker output {} disagrees with missing-page output {} about consumed text", describe(input), describe(&out), describe(miss_out)));
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn every_input_inside_the_bounds_expands_consistently() {
    let max_tokens: usize = std::env::var("FTML_INCLUDE_TOKENS").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
    std::panic::set_hook(Box::new(|_| {}));
    let mut checked = 0u64;
    let mut failures = Vec::new();
    for len in 0..=max_tokens {
        let mut pick = vec![0usize; len];
        loop {
            let input: String = pick.iter().map(|&i| TOKENS[i]).collect();
            checked += 1;
            if let Some(message) = check(&input) {
                if failures.len() < 8 {
                    failures.push(message);
                }
            }
            let mut pos = 0;
            while pos < len {
                pick[pos] += 1;
                if pick[pos] < TOKENS.len() {
                    break;
                }
                pick[pos] = 0;
                pos += 1;
            }
            if pos == len {
                break;
            }
        }
    }
    let _ = std::panic::take_hook();
    assert!(failures.is_empty(), "{checked} inputs checked; first failures:\n{}", failures.join("\n"));
    eprintln!("checked {checked} inputs, all properties held");
}

#[test]
fn include_padding_follows_surrounding_line_endings() {
    let includes = ["[[include page]]", "[[include page a=1]]", "[[include page | a=1]]"];
    let prefixes = ["", "x\n", "x\r\n", "x\n\n", "x\r\n\r\n", "\u{e9}\n", "\n", "\r\n"];
    let suffixes = ["", "\ny", "\r\ny", "\n\r\ny", "\r\n\ny", "\n", "\r\n", "\n\u{e9}"];
    let mut checked = 0;
    let mut failures = Vec::new();
    for include_text in includes {
        for prefix in prefixes {
            for suffix in suffixes {
                let input = format!("{prefix}{include_text}{suffix}");
                let (out, pages, asked) = match run(&input, Kind::Marker) {
                    Ok(v) => v,
                    Err(e) => {
                        failures.push(format!("{}: {e}", describe(&input)));
                        continue;
                    }
                };
                checked += 1;
                let before = if prefix.is_empty() {
                    ""
                } else if prefix.ends_with("\r\n") {
                    "\r\n"
                } else {
                    "\n"
                };
                let after = if suffix.is_empty() {
                    ""
                } else if suffix.starts_with("\r\n") {
                    "\r\n"
                } else {
                    "\n"
                };
                let expected = format!("{prefix}{before}{MARKER}{after}{suffix}");
                if asked != 1 || pages.len() != 1 || out != expected {
                    failures.push(format!("{}: got {} expected {}", describe(&input), describe(&out), describe(&expected)));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{checked} inputs checked; failures:\n{}", failures.join("\n"));
}
