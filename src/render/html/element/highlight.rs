/*
 * render/html/element/highlight.rs
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

//! Syntax highlighting for `[[code]]` blocks in the Wikidot layout.
//!
//! This is a faithful port of the PEAR `Text_Highlighter` engine as deployed
//! on Wikidot, driven by the generated language tables in
//! [`highlight_tables`]. Table contents transcribe the upstream definitions
//! one-to-one, with exactly two Wikidot-era deltas applied at generation
//! time: the pre-2007 `javascript` regular-expression-literal region (removed
//! upstream in 2007) is present, and the `css` `/* */` comment rule (added
//! upstream in 2008) is absent. Both are pinned by live Wikidot output.
//!
//! A handful of upstream match rules use PCRE features the `regex` crate
//! cannot express (look-around, backreferences, conditionals). Those rules
//! are replaced in the tables by never-matching placeholders (group counts
//! preserved) and reimplemented as [`HlCustom`] matchers below, mirroring the
//! upstream match semantics exactly.

use regex::bytes::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

#[allow(dead_code, unused_imports)]
mod tables {
    include!("highlight_tables.rs");
}

/// Table types shared with the generated [`tables`] module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HlCustom {
    None,
    PyFuncname,
    CssSelector,
    DtdTagname,
    HtmlTagname,
    PerlQqw,
    PerlQ,
    PerlSubst2,
    PerlSubst1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HlCustomEnd {
    CppLineEnd,
    PhpCodeEnd,
    DiffNormEnd,
    CssPropEnd,
}

#[derive(Debug, Clone, Copy)]
pub enum HlEnd {
    Pattern(&'static str),
    Custom(HlCustomEnd),
}

#[derive(Debug, Clone, Copy)]
pub struct HlRule {
    pub pattern: &'static str,
    pub groups: usize,
    pub custom: HlCustom,
    pub target: i32,
    pub remember: bool,
    pub delim: &'static str,
    pub inner: &'static str,
    pub parts: Option<&'static [(usize, &'static str)]>,
    pub keywords: Option<&'static [(&'static str, &'static str)]>,
}

#[derive(Debug, Clone, Copy)]
pub struct HlLang {
    pub def_class: &'static str,
    pub kwmap: &'static [(&'static str, &'static str)],
    pub disabled: &'static [&'static str],
    pub ends: &'static [(i32, HlEnd)],
    pub states: &'static [(i32, &'static [HlRule])],
}

/// Preprocesses a trimmed code body exactly like the upstream renderer:
/// CRLF/CR normalization, empty lines become a single space (some browsers
/// refuse to display empty lines), tab expansion, and a trailing trim.
fn preprocess(body: &str, tabsize: usize) -> String {
    let step = body.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(step.len());
    for (index, line) in step.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        if line.is_empty() {
            out.push(' ');
        } else {
            out.push_str(line);
        }
    }
    let expanded = out.replace('\t', &" ".repeat(tabsize));
    trim_end(&expanded).to_string()
}

fn trim_end(text: &str) -> &str {
    let bytes = text.as_bytes();
    let mut end = bytes.len();
    while end > 0 {
        // Matches PHP `rtrim` without arguments: space, tab, newline,
        // carriage return, NUL, and vertical tab.
        match bytes[end - 1] {
            b' ' | b'\t' | b'\n' | b'\r' | b'\0' | 0x0B => end -= 1,
            _ => break,
        }
    }
    &text[..end]
}

/// Escapes highlight text the way the upstream renderer does with
/// `htmlspecialchars` under its historical flags: `&`, `<`, `>`, and `"`
/// are escaped, while `'` is left as-is.
fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

const CLASS_MAP: &[(&str, &str)] = &[
    ("code", "hl-code"),
    ("identifier", "hl-identifier"),
    ("reserved", "hl-reserved"),
    ("number", "hl-number"),
    ("brackets", "hl-brackets"),
    ("quotes", "hl-quotes"),
    ("string", "hl-string"),
    ("special", "hl-special"),
    ("comment", "hl-comment"),
    ("mlcomment", "hl-mlcomment"),
    ("inlinetags", "hl-inlinetags"),
    ("var", "hl-var"),
    ("types", "hl-types"),
    ("prepro", "hl-prepro"),
    ("url", "hl-url"),
    ("inlinedoc", "hl-inlinedoc"),
    ("default", "hl-default"),
    ("builtin", "hl-builtin"),
];

fn full_class_name(class: &str) -> String {
    for (key, full) in CLASS_MAP {
        if *key == class {
            return full.to_string();
        }
    }
    format!("hl-{class}")
}

struct CompiledState {
    regex: Option<Regex>,
}

struct CompiledLang {
    states: HashMap<i32, CompiledState>,
    keywords: HashMap<(&'static str, &'static str), Regex>,
    def_class: &'static str,
    kwmap: HashMap<&'static str, &'static str>,
    disabled: Vec<&'static str>,
    tabsize: usize,
}

/// Strips the PCRE `/.../modifiers` delimiters from an upstream keyword
/// pattern. Upstream stores keywords as `preg_match` literals such as
/// `/^(if|else)$/`; the `regex` crate has no delimiters, so the slashes are
/// removed here (keeping the tables byte-faithful to upstream) and any
/// trailing modifiers are re-applied as an inline flag group.
fn strip_preg_delimiters(pattern: &str) -> String {
    let Some(without_open) = pattern.strip_prefix('/') else {
        return pattern.to_string();
    };
    let Some(end) = without_open.rfind('/') else {
        return pattern.to_string();
    };
    let (body, modifiers) = without_open.split_at(end);
    let modifiers = &modifiers[1..];
    if modifiers.is_empty() {
        return body.to_string();
    }
    let mut flags = String::from("(?");
    for ch in modifiers.chars() {
        if matches!(ch, 'i' | 'm' | 's' | 'x') {
            flags.push(ch);
        }
    }
    flags.push(')');
    format!("{flags}{body}")
}

fn compile_keywords(
    lang: &'static HlLang,
    map: &mut HashMap<(&'static str, &'static str), Regex>,
) {
    for (_, rules) in lang.states {
        for rule in *rules {
            if let Some(groups) = rule.keywords {
                for (group, pattern) in groups {
                    map.entry((*group, *pattern)).or_insert_with(|| {
                        let stripped = strip_preg_delimiters(pattern);
                        Regex::new(&format!("(?:{stripped})"))
                            .expect("invalid highlight keyword pattern")
                    });
                }
            }
        }
    }
}

fn compile_lang(lang: &'static HlLang) -> CompiledLang {
    let mut states = HashMap::new();
    for (state_id, rules) in lang.states {
        let mut alts = Vec::with_capacity(rules.len());
        for rule in rules.iter() {
            alts.push(format!("(?:{})", rule.pattern));
        }
        let regex = if alts.is_empty() {
            None
        } else {
            Some(
                Regex::new(&format!("(?:{})", alts.join("|")))
                    .expect("invalid highlight alternation"),
            )
        };
        states.insert(*state_id, CompiledState { regex });
    }
    let mut keywords = HashMap::new();
    compile_keywords(lang, &mut keywords);
    CompiledLang {
        states,
        keywords,
        def_class: lang.def_class,
        kwmap: lang.kwmap.iter().copied().collect(),
        disabled: lang.disabled.to_vec(),
        tabsize: 4,
    }
}

// ---------- custom (non-regex) matchers ----------

/// A match produced by a [`HlCustom`] rule: byte range plus capture-group
/// ranges (group 0 is the whole match) for remember-substitution and parts.
struct CustomHit {
    start: usize,
    end: usize,
    groups: Vec<Option<(usize, usize)>>,
}

fn next_boundary(text: &str, mut pos: usize) -> usize {
    let len = text.len();
    while pos < len && !text.is_char_boundary(pos) {
        pos += 1;
    }
    pos
}

fn prev_byte_is(text: &str, pos: usize, byte: u8) -> bool {
    pos > 0 && text.as_bytes()[pos - 1] == byte
}

fn static_regex(pattern: &'static str) -> Regex {
    Regex::new(pattern).expect("invalid static highlight pattern")
}

fn py_ident_re() -> Regex {
    static_regex("(?i)[a-z_]\\w*")
}

fn dtd_tag_re() -> Regex {
    static_regex("!(ENTITY|ATTLIST|ELEMENT|NOTATION)\\b")
}

fn html_tag_re() -> Regex {
    static_regex("(?i)[\\w\\-\\:]+")
}

fn css_selector_re() -> Regex {
    static_regex("(?i)((\\.|#)?[a-z]+[a-z\\d\\-]*|(\\*))")
}

fn diff_norm_re() -> Regex {
    static_regex("(?m)^[ad]\\d+\\s\\d+")
}

fn is_perl_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
}

fn find_py_funcname(text: &str, pos: usize) -> Option<CustomHit> {
    let re = py_ident_re();
    let bytes = text.as_bytes();
    let mut search = pos;
    loop {
        let found = re.find_at(text.as_bytes(), search)?;
        let mut next = found.end();
        while next < bytes.len() && is_perl_space(bytes[next]) {
            next += 1;
        }
        if next < bytes.len() && bytes[next] == b'(' {
            return Some(CustomHit {
                start: found.start(),
                end: found.end(),
                groups: vec![Some((found.start(), found.end()))],
            });
        }
        search = next_boundary(text, found.start() + 1);
        if search >= bytes.len() {
            return None;
        }
    }
}

fn find_dtd_tagname(text: &str, pos: usize) -> Option<CustomHit> {
    let re = dtd_tag_re();
    let mut search = pos;
    loop {
        let caps = re.captures_at(text.as_bytes(), search)?;
        let whole = caps.get(0).expect("regex whole match");
        if prev_byte_is(text, whole.start(), b'<') {
            let inner = caps.get(1).map(|m| (m.start(), m.end()));
            return Some(CustomHit {
                start: whole.start(),
                end: whole.end(),
                groups: vec![Some((whole.start(), whole.end())), inner],
            });
        }
        search = next_boundary(text, whole.start() + 1);
        if search >= text.len() {
            return None;
        }
    }
}

fn find_html_tagname(text: &str, pos: usize) -> Option<CustomHit> {
    let re = html_tag_re();
    let bytes = text.as_bytes();
    let mut search = pos;
    loop {
        let found = re.find_at(text.as_bytes(), search)?;
        if found.start() > 0 && matches!(bytes[found.start() - 1], b'<' | b'/' | b'?') {
            return Some(CustomHit {
                start: found.start(),
                end: found.end(),
                groups: vec![Some((found.start(), found.end()))],
            });
        }
        search = next_boundary(text, found.start() + 1);
        if search >= text.len() {
            return None;
        }
    }
}

fn is_css_name_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-'
}

fn find_css_selector(text: &str, pos: usize) -> Option<CustomHit> {
    let re = css_selector_re();
    let bytes = text.as_bytes();
    let mut search = pos;
    loop {
        let found = re.find_at(text.as_bytes(), search)?;
        let end = found.end();
        // Guard 1 (from `(?![a-z\d\-])`): the base match is already minimal
        // in the regex sense, so verify the next byte directly.
        let guard1 = end >= bytes.len() || !is_css_name_continue(bytes[end]);
        // Guard 2 (from `(?!\s*:\s*[\s\{])`): after skipping whitespace, a
        // colon must NOT be followed by another whitespace-or-`{` character.
        // The second `\s*` may match empty, so the very next byte after the
        // colon already decides — it must not be skipped over.
        let mut guard2 = true;
        if guard1 {
            let mut cursor = end;
            while cursor < bytes.len() && is_perl_space(bytes[cursor]) {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b':' {
                cursor += 1;
                if cursor < bytes.len()
                    && (is_perl_space(bytes[cursor]) || bytes[cursor] == b'{')
                {
                    guard2 = false;
                }
            }
        }
        if guard1 && guard2 {
            return Some(CustomHit {
                start: found.start(),
                end: found.end(),
                groups: vec![Some((found.start(), found.end()))],
            });
        }
        search = next_boundary(text, found.start() + 1);
        if search >= text.len() {
            return None;
        }
    }
}

// `s`, `tr`, `m` delimiters: `|` `#` `~` backtick `!` `@` `$` `%` `^` `&`
// `*` `+` `=` `\` `;` `:` `'` `"` `,` `.` `/` `?`. (`-`, space, and brackets
// are not delimiters; `-` forms a range inside the upstream class.)
fn is_perl_subst_delim(byte: u8) -> bool {
    matches!(
        byte,
        b'|' | b'#'
            | b'~'
            | b'`'
            | b'!'
            | b'@'
            | b'$'
            | b'%'
            | b'^'
            | b'&'
            | b'*'
            | b'+'
            | b'='
            | b'\\'
            | b';'
            | b':'
            | b'\''
            | b'"'
            | b','
            | b'.'
            | b'/'
            | b'?'
    )
}

fn is_perl_subst_modifier(byte: u8) -> bool {
    matches!(byte, b'e' | b'c' | b'g' | b'i' | b'm' | b'o' | b's' | b'x')
}

/// Scans one `((\\.|[^\x5c])*?)` content run ending at an unescaped `delim`.
/// Returns the end of the content (the delimiter position).
fn scan_perl_content(bytes: &[u8], mut pos: usize, delim: u8) -> Option<usize> {
    while pos < bytes.len() {
        let byte = bytes[pos];
        if byte == b'\\' {
            if pos + 1 >= bytes.len() || bytes[pos + 1] == b'\n' {
                return None;
            }
            pos += 2;
        } else if byte == delim {
            return Some(pos);
        } else {
            pos += 1;
        }
    }
    None
}

fn find_perl_subst(text: &str, pos: usize, with_two_parts: bool) -> Option<CustomHit> {
    let bytes = text.as_bytes();
    let mut search = pos;
    while search < bytes.len() {
        let rest = &bytes[search..];
        let op_len = if rest.starts_with(b"tr") {
            2
        } else if rest.starts_with(b"s") || rest.starts_with(b"m") {
            let is_m = rest.starts_with(b"m");
            if with_two_parts == is_m {
                search = next_boundary(text, search + 1);
                continue;
            }
            1
        } else {
            search = next_boundary(text, search + 1);
            continue;
        };
        let mut cursor = search + op_len;
        if cursor >= bytes.len() || !is_perl_subst_delim(bytes[cursor]) {
            search = next_boundary(text, search + 1);
            continue;
        }
        let delim = bytes[cursor];
        cursor += 1;
        let first = match scan_perl_content(bytes, cursor, delim) {
            Some(end) => end,
            None => {
                search = next_boundary(text, search + 1);
                continue;
            }
        };
        cursor = first + 1;
        if with_two_parts {
            let second = match scan_perl_content(bytes, cursor, delim) {
                Some(end) => end,
                None => {
                    search = next_boundary(text, search + 1);
                    continue;
                }
            };
            cursor = second + 1;
        }
        while cursor < bytes.len() && is_perl_subst_modifier(bytes[cursor]) {
            cursor += 1;
        }
        if !text.is_char_boundary(cursor) {
            search = next_boundary(text, search + 1);
            continue;
        }
        return Some(CustomHit {
            start: search,
            end: cursor,
            groups: vec![Some((search, cursor))],
        });
    }
    None
}

/// Matches `q`, `qw`, or `qq` quote-like operators. `two_letter` selects the
/// `q[wq]` rule (index 7) versus the plain-`q` rule (index 8); the engine
/// tries both and keeps the leftmost, breaking ties by rule order, exactly
/// like the upstream alternation.
fn find_perl_qquote(text: &str, pos: usize, two_letter: bool) -> Option<CustomHit> {
    let bytes = text.as_bytes();
    let mut search = pos;
    while search < bytes.len() {
        let byte = bytes[search];
        if byte != b'q' && byte != b'Q' {
            search = next_boundary(text, search + 1);
            continue;
        }
        if search > 0 {
            let prev = bytes[search - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                search = next_boundary(text, search + 1);
                continue;
            }
        }
        let mut cursor = search + 1;
        if two_letter {
            if cursor >= bytes.len()
                || (bytes[cursor] != b'w'
                    && bytes[cursor] != b'q'
                    && bytes[cursor] != b'W'
                    && bytes[cursor] != b'Q')
            {
                search = next_boundary(text, search + 1);
                continue;
            }
            cursor += 1;
        }
        while cursor < bytes.len() && is_perl_space(bytes[cursor]) {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            return None;
        }
        let delim = bytes[cursor];
        let delim_pos = cursor;
        cursor += 1;
        let close = match delim {
            b'{' => b'}',
            b'(' => b')',
            b'[' => b']',
            b'<' => b'>',
            _ => delim,
        };
        if !text.is_char_boundary(delim_pos) || !text.is_char_boundary(cursor) {
            search = next_boundary(text, search + 1);
            continue;
        }
        // The upstream lookahead gate only requires that a close exists.
        if !bytes[cursor..].contains(&close) {
            search = next_boundary(text, search + 1);
            continue;
        }
        let bracket_group = match delim {
            b'{' => 3,
            b'(' => 4,
            b'[' => 5,
            b'<' => 6,
            _ => 0,
        };
        let mut groups = vec![
            Some((search, cursor)),
            Some((search, cursor)),
            Some((delim_pos, cursor)),
            None,
            None,
            None,
            None,
            None,
        ];
        if bracket_group >= 3 {
            groups[bracket_group] = Some((delim_pos, cursor));
        } else {
            groups[7] = Some((delim_pos, cursor));
        }
        return Some(CustomHit {
            start: search,
            end: cursor,
            groups,
        });
    }
    None
}

fn find_custom(id: HlCustom, text: &str, pos: usize) -> Option<CustomHit> {
    match id {
        HlCustom::None => None,
        HlCustom::PyFuncname => find_py_funcname(text, pos),
        HlCustom::CssSelector => find_css_selector(text, pos),
        HlCustom::DtdTagname => find_dtd_tagname(text, pos),
        HlCustom::HtmlTagname => find_html_tagname(text, pos),
        HlCustom::PerlQqw => find_perl_qquote(text, pos, true),
        HlCustom::PerlQ => find_perl_qquote(text, pos, false),
        HlCustom::PerlSubst2 => find_perl_subst(text, pos, true),
        HlCustom::PerlSubst1 => find_perl_subst(text, pos, false),
    }
}

// ---------- custom end matchers ----------

/// Finds the end of a `//` line comment that continues past a trailing
/// backslash: the first line end not preceded by `\`. Returns an empty match.
fn find_cpp_line_end(text: &str, pos: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut cursor = pos;
    loop {
        match bytes[cursor..].iter().position(|&b| b == b'\n') {
            Some(offset) => {
                let end = cursor + offset;
                if end > 0 && bytes[end - 1] == b'\\' {
                    cursor = end + 1;
                    continue;
                }
                return Some((end, end));
            }
            None => {
                let end = bytes.len();
                if end > pos && bytes[end - 1] == b'\\' {
                    return None;
                }
                return Some((end, end));
            }
        }
    }
}

/// Finds the end of a PHP code region: the first line end or `?>`,
/// whichever comes first. The match is always empty.
fn find_php_code_end(text: &str, pos: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let line_end = bytes[pos..]
        .iter()
        .position(|&b| b == b'\n')
        .map(|offset| pos + offset)
        .unwrap_or(bytes.len());
    let mut tag_end = None;
    let mut cursor = pos;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'?' && bytes[cursor + 1] == b'>' {
            tag_end = Some(cursor);
            break;
        }
        cursor += 1;
    }
    let end = match tag_end {
        Some(tag) if tag < line_end => tag,
        _ => line_end,
    };
    Some((end, end))
}

/// Finds the next normal-diff hunk header line (`a…`/`d…`); the end of the
/// `normNew` region. The match is always empty.
fn find_diff_norm_end(text: &str, pos: usize) -> Option<(usize, usize)> {
    diff_norm_re()
        .find_at(text.as_bytes(), pos)
        .map(|m| (m.start(), m.start()))
}

/// Finds the end of a CSS property value: the next `;` or `}`.
/// The match is always empty.
fn find_css_prop_end(text: &str, pos: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut cursor = pos;
    while cursor < bytes.len() {
        if bytes[cursor] == b';' || bytes[cursor] == b'}' {
            return Some((cursor, cursor));
        }
        cursor += 1;
    }
    None
}

fn find_custom_end(id: HlCustomEnd, text: &str, pos: usize) -> Option<(usize, usize)> {
    match id {
        HlCustomEnd::CppLineEnd => find_cpp_line_end(text, pos),
        HlCustomEnd::PhpCodeEnd => find_php_code_end(text, pos),
        HlCustomEnd::DiffNormEnd => find_diff_norm_end(text, pos),
        HlCustomEnd::CssPropEnd => find_css_prop_end(text, pos),
    }
}

// ---------- engine ----------

fn preg_quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else {
            out.push('\\');
            out.push(ch);
        }
    }
    out
}

fn matching_brackets(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '(' => ')',
            ')' => '(',
            '<' => '>',
            '>' => '<',
            '[' => ']',
            ']' => '[',
            '{' => '}',
            '}' => '{',
            _ => ch,
        })
        .collect()
}

struct Frame {
    state: i32,
    last_delim: &'static str,
    last_inner: &'static str,
    end: EndState,
}

enum EndState {
    Top,
    State(Regex),
    Custom(HlCustomEnd),
    Remember(Regex),
}

struct RuleHit {
    start: usize,
    end: usize,
    /// Capture groups 0..=count (absolute byte ranges); missing groups are `None`.
    groups: Vec<Option<(usize, usize)>>,
}

fn group_text<'t>(
    text: &'t str,
    groups: &[Option<(usize, usize)>],
    index: usize,
) -> &'t str {
    match groups.get(index).copied().flatten() {
        Some((start, end)) => text.get(start..end).unwrap_or(""),
        None => "",
    }
}

/// Substitutes `%k%` / `%bk%` in a remember-endpattern, exactly like the
/// upstream tokenizer.
fn substitute_end(
    pattern: &str,
    text: &str,
    groups: &[Option<(usize, usize)>],
    count: usize,
) -> String {
    let mut out = pattern.to_string();
    for k in 0..=count {
        let value = group_text(text, groups, k);
        out = out.replace(&format!("%{k}%"), &preg_quote(value));
        out = out.replace(&format!("%b{k}%"), &preg_quote(&matching_brackets(value)));
    }
    out
}

fn find_rule(
    table_rules: &[HlRule],
    state: &CompiledState,
    text: &str,
    pos: usize,
) -> Option<(usize, RuleHit)> {
    let mut best: Option<(usize, usize, usize, Vec<Option<(usize, usize)>>)> = None;
    let mut consider = |rule_index: usize,
                        start: usize,
                        end: usize,
                        groups: Vec<Option<(usize, usize)>>| {
        // Byte-oriented matching mirrors the upstream engine, but emitted
        // text must stay on character boundaries for slicing.
        if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return;
        }
        let replace = match &best {
            None => true,
            Some((best_start, best_index, _, _)) => {
                start < *best_start || (start == *best_start && rule_index < *best_index)
            }
        };
        if replace {
            best = Some((start, rule_index, end, groups));
        }
    };
    if let Some(re) = &state.regex {
        if let Some(caps) = re.captures_at(text.as_bytes(), pos) {
            let mut offset = 0;
            for (rule_index, rule) in table_rules.iter().enumerate() {
                let mut found = None;
                for group in 1..=rule.groups {
                    if let Some(m) = caps.get(offset + group) {
                        found = Some((m, group));
                        break;
                    }
                }
                if let Some((m, _)) = found {
                    // Capture layout mirrors the upstream engine: index 0 is
                    // the rule's whole match, and indexes 1.. are the rule's
                    // INNER groups (the outer wrapper is not addressable).
                    // The rule occupies `rule.groups` captures starting at
                    // `offset + 1` (outer first, then inners).
                    let mut groups = vec![None; rule.groups + 1];
                    groups[0] = Some((m.start(), m.end()));
                    for g in 1..rule.groups {
                        groups[g] =
                            caps.get(offset + g + 1).map(|m| (m.start(), m.end()));
                    }
                    consider(rule_index, m.start(), m.end(), groups);
                    break;
                }
                offset += rule.groups;
            }
        }
    }
    for (rule_index, rule) in table_rules.iter().enumerate() {
        if rule.custom == HlCustom::None {
            continue;
        }
        if let Some(hit) = find_custom(rule.custom, text, pos) {
            consider(rule_index, hit.start, hit.end, hit.groups);
        }
    }
    best.map(|(start, rule_index, end, groups)| {
        (rule_index, RuleHit { start, end, groups })
    })
}

fn find_end(end: &EndState, text: &str, pos: usize) -> Option<(usize, usize)> {
    let found = match end {
        EndState::Top => None,
        EndState::State(re) => re
            .find_at(text.as_bytes(), pos)
            .map(|m| (m.start(), m.end())),
        EndState::Custom(id) => find_custom_end(*id, text, pos),
        EndState::Remember(re) => re
            .find_at(text.as_bytes(), pos)
            .map(|m| (m.start(), m.end())),
    };
    // Byte-oriented matching mirrors the upstream engine, but emitted text
    // must stay on character boundaries for slicing.
    found.filter(|(start, end)| {
        text.is_char_boundary(*start) && text.is_char_boundary(*end)
    })
}

/// Raw emitted token: class plus source byte range.
#[derive(Debug)]
struct RawToken {
    class: &'static str,
    start: usize,
    end: usize,
}

fn tokenize(compiled: &CompiledLang, table: &HlLang, text: &str) -> Vec<RawToken> {
    let mut out = Vec::new();
    let mut emit = |class: &'static str, start: usize, end: usize| {
        if start < end {
            out.push(RawToken { class, start, end });
        }
    };
    let state_rules = |state: i32| -> &'static [HlRule] {
        table
            .states
            .iter()
            .find(|(id, _)| *id == state)
            .map(|(_, rules)| *rules)
            .unwrap_or(&[])
    };
    let state_end = |state: i32| -> EndState {
        match table.ends.iter().find(|(id, _)| *id == state) {
            None => EndState::Top,
            Some((_, HlEnd::Pattern(pattern))) => EndState::State(
                Regex::new(&format!("(?:{pattern})"))
                    .expect("invalid highlight end pattern"),
            ),
            Some((_, HlEnd::Custom(id))) => EndState::Custom(*id),
        }
    };
    let mut state: i32 = -1;
    let mut pos = 0;
    let mut last_inner = compiled.def_class;
    let mut last_delim = compiled.def_class;
    let mut end = EndState::Top;
    let mut stack: Vec<Frame> = Vec::new();
    let len = text.len();
    loop {
        if pos >= len {
            break;
        }
        let compiled_state = match compiled.states.get(&state) {
            Some(state) => state,
            None => {
                emit(last_inner, pos, len);
                break;
            }
        };
        let rules = state_rules(state);
        let (endpos, endlen) = find_end(&end, text, pos)
            .map(|(start, stop)| (start, stop - start))
            .unwrap_or((usize::MAX, 0));
        let hit = find_rule(rules, compiled_state, text, pos);
        let rule_wins = match &hit {
            Some((_, hit)) => endpos == usize::MAX || hit.start < endpos,
            None => false,
        };
        if let Some((rule_index, hit)) = hit.filter(|_| rule_wins) {
            if hit.start == hit.end {
                // Zero-width rule matches cannot advance; the upstream
                // engine would spin here. Consume one character as gap so
                // tokenization always terminates.
                let next = next_boundary(text, pos + 1);
                emit(last_inner, pos, next);
                pos = next;
                continue;
            }
            let rule = &rules[rule_index];
            if hit.start > pos {
                emit(last_inner, pos, hit.start);
            }
            if rule.target != -1 {
                emit(rule.delim, hit.start, hit.end);
                let new_end = if rule.remember {
                    let source =
                        match table.ends.iter().find(|(id, _)| *id == rule.target) {
                            Some((_, HlEnd::Pattern(pattern))) => *pattern,
                            _ => "",
                        };
                    let substituted =
                        // Upstream substitutes `%0%`..`%count%` where `count`
                        // is the inner-group count (index 0 is the whole
                        // match, so the last inner index is groups - 1).
                        substitute_end(
                            source,
                            text,
                            &hit.groups,
                            rule.groups.saturating_sub(1),
                        );
                    EndState::Remember(
                        Regex::new(&format!("(?:{substituted})"))
                            .expect("invalid substituted highlight end"),
                    )
                } else {
                    state_end(rule.target)
                };
                stack.push(Frame {
                    state,
                    last_delim,
                    last_inner,
                    end: std::mem::replace(&mut end, EndState::Top),
                });
                end = new_end;
                state = rule.target;
                last_inner = rule.inner;
                last_delim = rule.delim;
                pos = hit.end;
            } else {
                // Block rule: resolve the token class.
                if let Some(parts) = rule.parts {
                    // Upstream parts index the rule's INNER groups (1-based);
                    // a group without a part entry advances the cursor but
                    // emits nothing, and gaps use the rule's inner class.
                    let mut partpos = hit.start;
                    for group in 1..rule.groups {
                        let span = hit.groups.get(group).copied().flatten();
                        let Some((start, stop)) = span else {
                            continue;
                        };
                        if let Some(class) = parts
                            .iter()
                            .find(|(index, _)| *index == group)
                            .map(|(_, class)| *class)
                        {
                            if start > partpos {
                                emit(rule.inner, partpos, start);
                            }
                            emit(class, start, stop);
                        }
                        partpos = stop;
                    }
                    emit(rule.inner, partpos, hit.end);
                } else if let Some(keywords) = rule.keywords {
                    let mut class = rule.inner;
                    let whole = group_text(text, &hit.groups, 0);
                    for (group, pattern) in keywords {
                        if compiled.disabled.iter().any(|d| d == group) {
                            continue;
                        }
                        if let Some(re) = compiled.keywords.get(&(*group, *pattern)) {
                            if re.is_match(whole.as_bytes()) {
                                class =
                                    compiled.kwmap.get(group).copied().unwrap_or(class);
                                break;
                            }
                        }
                    }
                    emit(class, hit.start, hit.end);
                } else {
                    emit(rule.inner, hit.start, hit.end);
                }
                pos = hit.end;
            }
        } else if endpos != usize::MAX {
            if endpos > pos {
                emit(last_inner, pos, endpos);
            }
            emit(last_delim, endpos, endpos + endlen);
            match stack.pop() {
                Some(frame) => {
                    state = frame.state;
                    last_delim = frame.last_delim;
                    last_inner = frame.last_inner;
                    end = frame.end;
                }
                None => {
                    end = EndState::Top;
                }
            }
            pos = endpos + endlen;
        } else {
            emit(last_inner, pos, len);
            break;
        }
    }
    out
}

fn merge_tokens(tokens: &[RawToken]) -> Vec<(&'static str, usize, usize)> {
    let mut merged: Vec<(&'static str, usize, usize)> = Vec::with_capacity(tokens.len());
    for token in tokens {
        if let Some(last) = merged.last_mut() {
            if last.0 == token.class && last.2 == token.start {
                last.2 = token.end;
                continue;
            }
        }
        merged.push((token.class, token.start, token.end));
    }
    merged
}

macro_rules! language {
    ($name:ident, $module:ident) => {
        fn $name() -> &'static CompiledLang {
            static COMPILED: OnceLock<CompiledLang> = OnceLock::new();
            COMPILED.get_or_init(|| compile_lang(&tables::$module::LANG))
        }
    };
}

language!(compiled_cpp, cpp);
language!(compiled_css, css);
language!(compiled_diff, diff);
language!(compiled_dtd, dtd);
language!(compiled_html, html);
language!(compiled_java, java);
language!(compiled_javascript, javascript);
language!(compiled_perl, perl);
language!(compiled_php, php);
language!(compiled_python, python);
language!(compiled_ruby, ruby);
language!(compiled_sql, sql);
language!(compiled_xml, xml);

fn compiled(language: &str) -> Option<&'static CompiledLang> {
    match language {
        "cpp" => Some(compiled_cpp()),
        "css" => Some(compiled_css()),
        "diff" => Some(compiled_diff()),
        "dtd" => Some(compiled_dtd()),
        "html" => Some(compiled_html()),
        "java" => Some(compiled_java()),
        "javascript" => Some(compiled_javascript()),
        "perl" => Some(compiled_perl()),
        "php" => Some(compiled_php()),
        "python" => Some(compiled_python()),
        "ruby" => Some(compiled_ruby()),
        "sql" => Some(compiled_sql()),
        "xml" => Some(compiled_xml()),
        _ => None,
    }
}

/// Highlights a trimmed code body for a Wikidot `[[code]]` language.
///
/// Returns `None` when the language has no highlighter (plain rendering),
/// mirroring upstream behavior for unmapped `type` values.
pub fn highlight_code(language: &str, body: &str) -> Option<String> {
    let compiled = compiled(language)?;
    let processed = preprocess(body, compiled.tabsize);
    let tokens = tokenize(compiled, table_for(language)?, &processed);
    let mut html = String::new();
    for (class, start, end) in merge_tokens(&tokens) {
        let text = processed.get(start..end).unwrap_or("");
        html.push_str(&format!(
            "<span class=\"{}\">{}</span>",
            full_class_name(class),
            escape_html(text)
        ));
    }
    Some(html)
}

fn table_for(language: &str) -> Option<&'static HlLang> {
    match language {
        "cpp" => Some(&tables::cpp::LANG),
        "css" => Some(&tables::css::LANG),
        "diff" => Some(&tables::diff::LANG),
        "dtd" => Some(&tables::dtd::LANG),
        "html" => Some(&tables::html::LANG),
        "java" => Some(&tables::java::LANG),
        "javascript" => Some(&tables::javascript::LANG),
        "perl" => Some(&tables::perl::LANG),
        "php" => Some(&tables::php::LANG),
        "python" => Some(&tables::python::LANG),
        "ruby" => Some(&tables::ruby::LANG),
        "sql" => Some(&tables::sql::LANG),
        "xml" => Some(&tables::xml::LANG),
        _ => None,
    }
}
