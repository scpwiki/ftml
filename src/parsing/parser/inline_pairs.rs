//! Wikidot's per-construct inline pairing, computed over the token stream.
//!
//! Each Wikidot inline construct pairs its own markers, first fit, over the
//! whole line chunk, and never looks at any other construct. Two pairs can
//! therefore interleave (`**a//b**c//`), which a recursive parser cannot
//! represent, and a marker with no partner can sit inside another pair
//! (`**a//b **c//`), which a recursive parser pairs by accident. For such a
//! chunk this scan assigns every marker token a role, and `consume` emits the
//! flat events directly. The lowering stack then nests them.
//!
//! Any other chunk gets no roles and parses normally. So does a chunk holding
//! anything whose text the scan cannot see through, such as a link, whose label
//! Wikidot tokenizes before any inline construct runs.

use super::block_pairs::WikidotBlockPairs;
use crate::parsing::rule::impls::block::get_block_rule_with_name;
use crate::parsing::rule::impls::normalize_wikidot_color;
use crate::parsing::{ExtractedToken, Token};
use crate::tree::InlineFormatKind;
use std::collections::BTreeMap;
use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InlineMarkerRole {
    Open(InlineFormatKind),
    Close(InlineFormatKind),
    ColorOpen { color: String, body_start: usize },
    ColorClose,
    Literal,
    Removed,
}

#[derive(Debug, Default)]
pub(crate) struct WikidotInlineMarkers {
    roles: BTreeMap<usize, InlineMarkerRole>,
}

impl WikidotInlineMarkers {
    pub(crate) fn role_at_byte(&self, byte: usize) -> Option<&InlineMarkerRole> {
        self.roles.get(&byte)
    }
}

/// Keeps the head scan constant per `##`, which keeps the whole pass linear.
const MAX_COLOR_HEAD_TOKENS: usize = 32;

pub(crate) fn is_marker(token: Token) -> bool {
    matches!(
        token,
        Token::Bold
            | Token::Italics
            | Token::Underline
            | Token::DoubleDash
            | Token::Superscript
            | Token::Subscript
            | Token::LeftMonospace
            | Token::RightMonospace
            | Token::Color
    )
}

enum TokenClass {
    Inert,
    Opaque,
    InlineScope(usize),
    Barrier(usize),
    ImageBarrier(usize),
}

fn block_name<'t>(tokens: &[ExtractedToken<'t>], index: usize) -> Option<&'t str> {
    let name = tokens.get(index + 1)?;
    (name.token == Token::Identifier).then_some(name.slice)
}

/// Live Text_Wiki runs inline pairing (rules 57-69) only in segments between earlier
/// block-rule tokens. Paired heads that participate in shipped markup for the F13
/// family and for the legacy `[[div]]` / `[[collapsible]]` cases split the scan here.
fn inline_pairing_splits_at_paired_block(name: &str) -> bool {
    name.eq_ignore_ascii_case("ul")
        || name.eq_ignore_ascii_case("ol")
        || name.eq_ignore_ascii_case("li")
        || name.eq_ignore_ascii_case("table")
        || name.eq_ignore_ascii_case("row")
        || name.eq_ignore_ascii_case("cell")
        || name.eq_ignore_ascii_case("hcell")
        || name.eq_ignore_ascii_case("tabview")
        || name.eq_ignore_ascii_case("tabs")
        || name.eq_ignore_ascii_case("tab")
        || name.eq_ignore_ascii_case("hidden")
        || name.eq_ignore_ascii_case("footnote")
        || name.eq_ignore_ascii_case("footnoteblock")
        || name.eq_ignore_ascii_case("note")
        || name.eq_ignore_ascii_case("collapsible")
        || name.eq_ignore_ascii_case("blockquote")
        || name.eq_ignore_ascii_case("quote")
        || name.eq_ignore_ascii_case("div")
        || name.eq_ignore_ascii_case("code")
        || name.eq_ignore_ascii_case("math")
        || name == "="
        || name == "<"
        || name == ">"
        || name == "=="
}

fn head_end(tokens: &[ExtractedToken<'_>], index: usize) -> Option<usize> {
    let abort_on_marker = tokens
        .get(index)
        .is_some_and(|t| t.token == Token::LeftBlock);
    for (offset, token) in tokens[index..].iter().enumerate() {
        match token.token {
            Token::RightBlock => return Some(index + offset + 1),
            Token::ParagraphBreak | Token::InputEnd => return None,
            token if abort_on_marker && is_marker(token) => return None,
            _ => {}
        }
    }
    None
}

/// Wikidot's image rule writes an aligned image as a token between two blank
/// lines, so the inline rules that run later see two separate chunks.
fn aligned_image_head(tokens: &[ExtractedToken<'_>], index: usize) -> bool {
    if tokens[index].token != Token::LeftBlock {
        return false;
    }
    let mut head = String::new();
    for token in tokens.iter().skip(index + 1).take(4) {
        if matches!(token.token, Token::Whitespace | Token::RightBlock) {
            break;
        }
        head.push_str(token.slice);
    }
    ["f<image", "f>image", "<image", ">image", "=image"]
        .iter()
        .any(|aligned| head.eq_ignore_ascii_case(aligned))
}

fn classify_block(
    tokens: &[ExtractedToken<'_>],
    index: usize,
    pairs: &WikidotBlockPairs,
) -> TokenClass {
    if aligned_image_head(tokens, index)
        && let Some(end) = head_end(tokens, index)
    {
        return TokenClass::ImageBarrier(end);
    }
    let Some(name) = block_name(tokens, index) else {
        return TokenClass::Opaque;
    };
    let Some(end) = head_end(tokens, index) else {
        return TokenClass::Opaque;
    };
    if name.eq_ignore_ascii_case("span") || name.eq_ignore_ascii_case("size") {
        return TokenClass::InlineScope(end);
    }
    let byte = tokens[index].span.start;
    if !inline_pairing_splits_at_paired_block(name) {
        return TokenClass::Opaque;
    }
    if tokens[index].token == Token::LeftBlock {
        let paired = pairs.pair_at_head_byte(byte).is_some();
        let div_or_collapsible =
            name.eq_ignore_ascii_case("div") || name.eq_ignore_ascii_case("collapsible");
        if div_or_collapsible {
            if paired {
                return TokenClass::Barrier(end);
            }
        } else if get_block_rule_with_name(name).is_some() || paired {
            return TokenClass::Barrier(end);
        }
        return TokenClass::Opaque;
    }
    let paired = pairs.pair_at_closer_byte(byte).is_some();
    if tokens[index].token == Token::LeftBlockEnd {
        let div_or_collapsible =
            name.eq_ignore_ascii_case("div") || name.eq_ignore_ascii_case("collapsible");
        if div_or_collapsible {
            if paired {
                return TokenClass::Barrier(end);
            }
        } else if get_block_rule_with_name(name).is_some() || paired {
            return TokenClass::Barrier(end);
        }
        return TokenClass::Opaque;
    }
    TokenClass::Opaque
}

fn classify(
    tokens: &[ExtractedToken<'_>],
    index: usize,
    line_start: bool,
    pairs: &WikidotBlockPairs,
) -> TokenClass {
    let token = tokens[index].token;
    match token {
        Token::ParagraphBreak | Token::InputEnd => TokenClass::Barrier(index + 1),
        Token::LeftBlock | Token::LeftBlockEnd => classify_block(tokens, index, pairs),
        Token::Equals | Token::Colon if line_start => TokenClass::Opaque,
        Token::Identifier
        | Token::Other
        | Token::Whitespace
        | Token::LineBreak
        | Token::Pipe
        | Token::Equals
        | Token::Colon
        | Token::Underscore
        | Token::DoubleQuote
        | Token::RightBracket
        | Token::RightBlock
        | Token::RightParentheses
        | Token::InputStart => TokenClass::Inert,
        token if is_marker(token) => TokenClass::Inert,
        _ => TokenClass::Opaque,
    }
}

pub(crate) fn compute_wikidot_inline_markers(
    tokens: &[ExtractedToken<'_>],
    pairs: &WikidotBlockPairs,
) -> WikidotInlineMarkers {
    let mut markers = WikidotInlineMarkers::default();
    let mut start = 0;
    let mut opaque = false;
    let mut after_image = false;
    let mut index = 0;
    while index < tokens.len() {
        let line_start = index == start || tokens[index - 1].token == Token::LineBreak;
        match classify(tokens, index, line_start, pairs) {
            TokenClass::Inert => index += 1,
            TokenClass::Opaque => {
                opaque = true;
                index += 1;
            }
            TokenClass::InlineScope(end) => index = end,
            TokenClass::Barrier(end) | TokenClass::ImageBarrier(end) => {
                let image = matches!(
                    classify(tokens, index, line_start, pairs),
                    TokenClass::ImageBarrier(_)
                );
                let block_structure_barrier = image
                    || matches!(
                        tokens[index].token,
                        Token::LeftBlock | Token::LeftBlockEnd | Token::InputEnd
                    );
                if !opaque {
                    scan_chunk(
                        tokens,
                        start..index,
                        after_image || image,
                        block_structure_barrier,
                        &mut markers,
                    );
                }
                start = end;
                opaque = false;
                after_image = image;
                index = end;
            }
        }
    }
    if !opaque {
        let mut end = tokens.len();
        if end > 0 && tokens[end - 1].token == Token::InputEnd {
            end -= 1;
        }
        if start < end {
            scan_chunk(tokens, start..end, after_image, true, &mut markers);
        }
    }
    markers
}

struct Pair {
    open: usize,
    close: usize,
    role: PairKind,
}

enum PairKind {
    Format(InlineFormatKind),
    Color { color: String, pipe: usize },
}

/// The state one chunk builds up rule by rule, in Wikidot's rule order. An empty
/// pair leaves nothing behind, so a later rule sees the tokens around it as
/// adjacent: `x//**** a//b` keeps its `//` literal because `Strong` runs first
/// and leaves `// a//`, while `x//____ a//b` forms because `Underline` runs after
/// `Emphasis`.
struct ChunkScan<'a, 't> {
    tokens: &'a [ExtractedToken<'t>],
    range: Range<usize>,
    removed: Vec<bool>,
    pairs: Vec<Pair>,
}

impl ChunkScan<'_, '_> {
    fn is_removed(&self, index: usize) -> bool {
        self.removed[index - self.range.start]
    }

    fn remove(&mut self, index: usize) {
        self.removed[index - self.range.start] = true;
    }

    fn next_visible(&self, index: usize) -> Option<usize> {
        (index + 1..self.range.end).find(|&next| !self.is_removed(next))
    }

    fn previous_visible(&self, index: usize) -> Option<usize> {
        (self.range.start..index)
            .rev()
            .find(|&previous| !self.is_removed(previous))
    }

    fn is_space(&self, index: Option<usize>) -> bool {
        index.is_none_or(|index| self.tokens[index].token == Token::Whitespace)
    }
}

/// `image_bounded`: the chunk ends or starts at an aligned image, where the
/// legacy inline parser would otherwise pair markers across the image.
fn scan_chunk(
    tokens: &[ExtractedToken<'_>],
    range: Range<usize>,
    image_bounded: bool,
    block_structure_barrier: bool,
    markers: &mut WikidotInlineMarkers,
) {
    if block_structure_barrier {
        let mut marker_positions: Vec<usize> = Vec::new();
        for i in range.clone() {
            if is_marker(tokens[i].token) {
                marker_positions.push(i);
            } else if !matches!(
                tokens[i].token,
                Token::Whitespace | Token::LineBreak | Token::InputStart
            ) {
                marker_positions.clear();
                break;
            }
        }
        if marker_positions.len() == 1 && tokens[marker_positions[0]].token == Token::Bold
        {
            markers.roles.insert(
                tokens[marker_positions[0]].span.start,
                InlineMarkerRole::Literal,
            );
            return;
        }
        if marker_positions.len() == 2
            && marker_positions[1] == marker_positions[0] + 1
            && tokens[marker_positions[0]].token == Token::Bold
            && tokens[marker_positions[1]].token == Token::Bold
        {
            for i in marker_positions {
                markers
                    .roles
                    .insert(tokens[i].span.start, InlineMarkerRole::Literal);
            }
            return;
        }
    }
    let mut scan = ChunkScan {
        tokens,
        removed: vec![false; range.len()],
        range: range.clone(),
        pairs: Vec::new(),
    };
    pair_color(&mut scan);
    pair_symmetric(&mut scan, Token::Bold, InlineFormatKind::Bold);
    pair_symmetric(&mut scan, Token::Italics, InlineFormatKind::Italics);
    pair_symmetric(&mut scan, Token::Underline, InlineFormatKind::Underline);
    pair_symmetric(
        &mut scan,
        Token::DoubleDash,
        InlineFormatKind::Strikethrough,
    );
    if !pair_monospace(&mut scan) {
        return;
    }
    pair_symmetric(&mut scan, Token::Superscript, InlineFormatKind::Superscript);
    pair_symmetric(&mut scan, Token::Subscript, InlineFormatKind::Subscript);

    let mut kinds: Vec<Token> = Vec::new();
    let mut marker_count = 0;
    let mut removed_count = 0;
    for (offset, token) in tokens[range.clone()].iter().enumerate() {
        if !is_marker(token.token) {
            continue;
        }
        if scan.removed[offset] {
            removed_count += 1;
        }
        marker_count += 1;
        let kind = match token.token {
            Token::RightMonospace => Token::LeftMonospace,
            kind => kind,
        };
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    let paired = scan.pairs.len() * 2 + removed_count;
    let unpaired_marker = paired > 0 && marker_count > paired && kinds.len() > 1;
    if !image_bounded && !unpaired_marker && !pairs_interleave(&scan.pairs) {
        return;
    }

    for (offset, token) in tokens[range.clone()].iter().enumerate() {
        if is_marker(token.token) {
            let index = range.start + offset;
            if scan.removed[offset]
                && removed_bold_kept_for_footnote_rule(tokens, &range, &scan, index)
            {
                continue;
            }
            let role = if scan.removed[offset] {
                InlineMarkerRole::Removed
            } else {
                InlineMarkerRole::Literal
            };
            markers.roles.insert(token.span.start, role);
        }
    }
    for pair in scan.pairs {
        let (open, close) = match pair.role {
            PairKind::Format(kind) => {
                (InlineMarkerRole::Open(kind), InlineMarkerRole::Close(kind))
            }
            PairKind::Color { color, pipe } => (
                InlineMarkerRole::ColorOpen {
                    color,
                    body_start: tokens[pipe].span.end,
                },
                InlineMarkerRole::ColorClose,
            ),
        };
        markers.roles.insert(tokens[pair.open].span.start, open);
        markers.roles.insert(tokens[pair.close].span.start, close);
    }
}

fn removed_bold_in_chunk(scan: &ChunkScan<'_, '_>, index: usize) -> bool {
    scan.range.contains(&index) && tokens_bold(scan, index) && scan.is_removed(index)
}

fn tokens_bold(scan: &ChunkScan<'_, '_>, index: usize) -> bool {
    scan.tokens
        .get(index)
        .is_some_and(|token| token.token == Token::Bold)
}

fn removed_bold_kept_for_footnote_rule(
    tokens: &[ExtractedToken<'_>],
    range: &Range<usize>,
    scan: &ChunkScan<'_, '_>,
    index: usize,
) -> bool {
    if !tokens_bold(scan, index) {
        return false;
    }
    let partner = if removed_bold_in_chunk(scan, index + 1) {
        index + 1
    } else if removed_bold_in_chunk(scan, index.wrapping_sub(1)) {
        index - 1
    } else {
        return false;
    };
    if index.max(partner) + 1 != range.end {
        return false;
    }
    tokens.get(range.end).is_some_and(|token| {
        token.token == Token::LeftBlock
            && block_name(tokens, range.end)
                .is_some_and(|name| name.eq_ignore_ascii_case("footnote"))
    })
}

/// `M(|[^\s](?:.*[^\s])?)M` ungreedy, except that a strike has no empty form.
fn pair_symmetric(scan: &mut ChunkScan<'_, '_>, token: Token, kind: InlineFormatKind) {
    let positions: Vec<usize> = scan
        .range
        .clone()
        .filter(|&index| scan.tokens[index].token == token && !scan.is_removed(index))
        .collect();
    let mut next_close = vec![None; positions.len() + 1];
    for slot in (0..positions.len()).rev() {
        let unpadded = !scan.is_space(scan.previous_visible(positions[slot]));
        next_close[slot] = if unpadded {
            Some(slot)
        } else {
            next_close[slot + 1]
        };
    }
    let mut slot = 0;
    while slot < positions.len() {
        let open = positions[slot];
        let next = scan.next_visible(open);
        let adjacent = next.is_some() && positions.get(slot + 1).copied() == next;
        if adjacent && kind != InlineFormatKind::Strikethrough {
            scan.remove(open);
            scan.remove(positions[slot + 1]);
            slot += 2;
            continue;
        }
        let first_candidate = slot + 1 + usize::from(adjacent);
        let close = if scan.is_space(next) {
            None
        } else {
            next_close.get(first_candidate).copied().flatten()
        };
        if let Some(close) = close {
            scan.pairs.push(Pair {
                open,
                close: positions[close],
                role: PairKind::Format(kind),
            });
            slot = close + 1;
        } else {
            slot += 1;
        }
    }
}

/// `{{({*?.*}*?)}}` with the U flag: the body runs to the first `}}`, and an
/// adjacent `}}` extends it. Returns false for a stray `}` next to the close,
/// which splits a token, so the chunk keeps its normal parse.
fn pair_monospace(scan: &mut ChunkScan<'_, '_>) -> bool {
    let mut index = scan.range.start;
    while index < scan.range.end {
        if scan.tokens[index].token != Token::LeftMonospace {
            index += 1;
            continue;
        }
        let Some(first) = (index + 1..scan.range.end)
            .find(|&candidate| scan.tokens[candidate].token == Token::RightMonospace)
        else {
            return true;
        };
        let mut close = first;
        while close + 1 < scan.range.end
            && scan.tokens[close + 1].token == Token::RightMonospace
        {
            close += 1;
        }
        if close + 1 < scan.range.end && scan.tokens[close + 1].slice.starts_with('}') {
            return false;
        }
        if scan.next_visible(index) == Some(close) {
            scan.remove(index);
            scan.remove(close);
        } else {
            scan.pairs.push(Pair {
                open: index,
                close,
                role: PairKind::Format(InlineFormatKind::Monospace),
            });
        }
        index = close + 1;
    }
    true
}

/// `##(.+?)\|(.+?)##`, with a color Wikidot accepts.
fn pair_color(scan: &mut ChunkScan<'_, '_>) {
    let mut index = scan.range.start;
    while index < scan.range.end {
        if scan.tokens[index].token != Token::Color {
            index += 1;
            continue;
        }
        let Some((color, pipe)) = color_head(scan.tokens, index, scan.range.end) else {
            index += 1;
            continue;
        };
        let close = (pipe + 2..scan.range.end)
            .find(|&candidate| scan.tokens[candidate].token == Token::Color);
        let Some(close) = close else {
            index += 1;
            continue;
        };
        for head_token in index + 1..=pipe {
            scan.remove(head_token);
        }
        scan.pairs.push(Pair {
            open: index,
            close,
            role: PairKind::Color { color, pipe },
        });
        index = close + 1;
    }
}

fn color_head(
    tokens: &[ExtractedToken<'_>],
    open: usize,
    end: usize,
) -> Option<(String, usize)> {
    let mut text = String::new();
    let head_end = end.min(open + 1 + MAX_COLOR_HEAD_TOKENS);
    for (offset, token) in tokens[open + 1..head_end].iter().enumerate() {
        match token.token {
            Token::Pipe => {
                let color = normalize_wikidot_color(&text)?.into_owned();
                return Some((color, open + 1 + offset));
            }
            Token::Identifier
            | Token::Other
            | Token::Whitespace
            | Token::Color
            | Token::DoubleDash
            | Token::RightParentheses => {
                text.push_str(token.slice);
            }
            _ => return None,
        }
    }
    None
}

fn pairs_interleave(pairs: &[Pair]) -> bool {
    let mut sorted: Vec<(usize, usize)> =
        pairs.iter().map(|pair| (pair.open, pair.close)).collect();
    sorted.sort_unstable();
    let mut open_closes: Vec<usize> = Vec::new();
    for (open, close) in sorted {
        while open_closes.last().is_some_and(|&last| last < open) {
            open_closes.pop();
        }
        if open_closes.last().is_some_and(|&last| last < close) {
            return true;
        }
        open_closes.push(close);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::PageInfo;
    use crate::parsing::parser::block_pairs;

    #[test]
    fn bold_does_not_pair_across_ul_head() {
        let source = "**[[ul]]\n[[li]]\nx\n[[/li]]\n[[/ul]]**";
        let tokenization = crate::tokenize(source);
        let pairs = block_pairs::compute_wikidot_block_pairs(
            tokenization.tokens(),
            source,
            &PageInfo::dummy(),
        );
        let markers = compute_wikidot_inline_markers(tokenization.tokens(), &pairs);
        let mut roles: Vec<(usize, InlineMarkerRole)> = markers
            .roles
            .iter()
            .map(|(&byte, role)| (byte, role.clone()))
            .collect();
        roles.sort_unstable_by_key(|(byte, _)| *byte);
        assert!(
            roles
                .iter()
                .filter(|(_, role)| matches!(
                    role,
                    InlineMarkerRole::Open(InlineFormatKind::Bold)
                ))
                .count()
                <= 1,
            "unexpected bold open roles: {roles:?}"
        );
        for (byte, role) in roles {
            if source[byte..].starts_with("**") {
                assert!(
                    matches!(role, InlineMarkerRole::Literal),
                    "expected literal ** at {byte}, got {role:?}"
                );
            }
        }
    }

    #[test]
    fn literal_bold_markers_on_both_sides_of_ul_block() {
        let source = "**[[ul]]\n[[li]]\nx\n[[/li]]\n[[/ul]]**";
        let tokenization = crate::tokenize(source);
        let pairs = block_pairs::compute_wikidot_block_pairs(
            tokenization.tokens(),
            source,
            &PageInfo::dummy(),
        );
        let markers = compute_wikidot_inline_markers(tokenization.tokens(), &pairs);
        let bold_bytes: Vec<usize> = tokenization
            .tokens()
            .iter()
            .filter(|t| t.token == Token::Bold)
            .map(|t| t.span.start)
            .collect();
        assert_eq!(bold_bytes.len(), 2, "expected two ** tokens");
        for byte in bold_bytes {
            assert!(
                matches!(markers.role_at_byte(byte), Some(InlineMarkerRole::Literal)),
                "expected literal at byte {byte}, got {:?}",
                markers.role_at_byte(byte)
            );
        }
    }
}
