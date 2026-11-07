use super::parser_functions::LiteralRegionIndex;
use std::collections::BTreeSet;

pub(super) fn substitute_wikidot(text: &mut String) {
    if !text.contains("[!--") {
        return;
    }

    elide_comment_only_lines(text);
    if !text.contains("[!--") {
        return;
    }

    let literal_regions = LiteralRegionIndex::new(text);
    let block_name_comments = regular_block_name_comment_starts(text);
    let spec_comments = iftags_spec_comment_starts(text);
    let code_closer_comments = code_closer_comment_starts(text);
    let mut output = String::with_capacity(text.len());
    let mut copied_until = 0usize;
    let mut scan = 0usize;
    let mut removed = false;
    let mut seams: Vec<usize> = Vec::new();
    let mut break_drops: Vec<std::ops::Range<usize>> = Vec::new();

    while let Some(relative_open) = text[scan..].find("[!--") {
        let open = scan + relative_open;
        if literal_regions.contains(open)
            || block_name_comments.contains(&open)
            || spec_comments.contains(&open)
            || code_closer_comments.contains(&open)
        {
            scan = open + "[!--".len();
            continue;
        }

        let close_search = open + "[!--".len();
        let Some(relative_close) = text[close_search..].find("--]") else {
            break;
        };
        let close = close_search + relative_close + "--]".len();
        if comment_contains_iftags_head(&text[open..close]) {
            scan = close;
            continue;
        }
        if leading_comment_remains_an_owner_barrier(text, open, close) {
            if line_remainder_splices(text, close, &literal_regions, &block_name_comments)
                && let Some(drop) = line_start_break_drop(text, open)
            {
                break_drops.push(drop);
            }
            scan = close;
            continue;
        }
        let (remove_start, remove_end) =
            comment_only_subscript(text, open, close).unwrap_or((open, close));
        push_copied_range(&mut output, text, copied_until..remove_start, &break_drops);
        seams.push(output.len());
        if remove_start == open && comment_elision_needs_raw_barrier(text, open, close) {
            output.push('\0');
        }
        copied_until = remove_end;
        scan = remove_end;
        removed = true;
    }

    if removed || !break_drops.is_empty() {
        push_copied_range(&mut output, text, copied_until..text.len(), &break_drops);
        collapse_comment_seams(&mut output, &seams);
        *text = output;
    }
}

/// Live Wikidot removes a whole physical line whose only content is a
/// column-0 comment: the comment and the newline that ends the comment's last
/// line both disappear, so the lines around it become adjacent. A remainder
/// after the closer (even a space) keeps the line in place and is left to the
/// ordinary comment path. Comments that open inside another comment (a closer
/// written as `[!----]`) are never treated as lines of their own.
fn elide_comment_only_lines(text: &mut String) {
    let literal_regions = LiteralRegionIndex::new(text);
    let block_name_comments = regular_block_name_comment_starts(text);
    let spec_comments = iftags_spec_comment_starts(text);
    let code_closer_comments = code_closer_comment_starts(text);

    let mut output = String::with_capacity(text.len());
    let mut copied_until = 0usize;
    let mut scan = 0usize;
    let mut removed = false;
    while let Some(relative_open) = text[scan..].find("[!--") {
        let open = scan + relative_open;
        if literal_regions.contains(open)
            || block_name_comments.contains(&open)
            || spec_comments.contains(&open)
            || code_closer_comments.contains(&open)
        {
            scan = open + "[!--".len();
            continue;
        }
        let line_start = text[..open].rfind('\n').map_or(0, |newline| newline + 1);
        if open != line_start {
            scan = open + "[!--".len();
            continue;
        }
        let close_search = open + "[!--".len();
        let Some(relative_close) = text[close_search..].find("--]") else {
            break;
        };
        let close = close_search + relative_close + "--]".len();
        if comment_contains_iftags_head(&text[open..close]) {
            scan = close;
            continue;
        }

        let bytes = text.as_bytes();
        let mut end = close;
        if bytes.get(end) == Some(&b'\r') {
            end += 1;
        }
        let break_end = match bytes.get(end) {
            Some(b'\n') => Some(end + 1),
            None => Some(end),
            _ => None,
        };
        let Some(break_end) = break_end else {
            scan = close;
            continue;
        };
        if previous_line_is_blank(text, line_start) && next_line_is_blank(text, break_end)
        {
            scan = close;
            continue;
        }
        output.push_str(&text[copied_until..line_start]);
        copied_until = break_end;
        scan = break_end;
        removed = true;
    }

    if removed {
        output.push_str(&text[copied_until..]);
        *text = output;
    }
}

/// `,,[!-- c --],,` is a subscript whose body is only a comment. Live drops
/// the markers and the comment. A bare `,,,,` run is not that shape and stays.
fn comment_only_subscript(
    text: &str,
    open: usize,
    close: usize,
) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    if open < 2 || close + 2 > bytes.len() {
        return None;
    }
    if &bytes[open - 2..open] != b",," || &bytes[close..close + 2] != b",," {
        return None;
    }
    if open >= 3 && bytes[open - 3] == b',' {
        return None;
    }
    if bytes.get(close + 2) == Some(&b',') {
        return None;
    }
    Some((open - 2, close + 2))
}

fn previous_line_is_blank(text: &str, line_start: usize) -> bool {
    if line_start == 0 {
        return false;
    }
    let before = &text[..line_start - 1];
    let start = before.rfind('\n').map_or(0, |offset| offset + 1);
    before[start..]
        .bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
}

fn next_line_is_blank(text: &str, line_start: usize) -> bool {
    if line_start >= text.len() {
        return false;
    }
    let line_end = text[line_start..]
        .find('\n')
        .map_or(text.len(), |offset| line_start + offset);
    text[line_start..line_end]
        .bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
}

/// Live Wikidot keeps the definition-list prefix width when a comment between
/// the marker and the first separator is elided: `: [!-- c --] : e` behaves
/// like `:  : e` (an empty key), not like `: : e`.
fn seam_run_starts_definition_list(text: &str, start: usize) -> bool {
    if start == 0 || text.as_bytes()[start - 1] != b':' {
        return false;
    }
    let colon = start - 1;
    colon == 0 || text.as_bytes()[colon - 1] == b'\n'
}

fn collapse_comment_seams(output: &mut String, seams: &[usize]) {
    if seams.is_empty() {
        return;
    }
    let heads = block_head_ranges(output);
    let bytes = output.as_bytes();
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if matches!(bytes[index], b' ' | b'\t') {
            let start = index;
            while index < bytes.len() && matches!(bytes[index], b' ' | b'\t') {
                index += 1;
            }
            if index - start > 1
                && seams.iter().any(|&seam| start <= seam && seam <= index)
                && !heads
                    .iter()
                    .any(|head| head.start <= start && index <= head.end)
                && !seam_run_starts_definition_list(output, start)
            {
                ranges.push(start..index);
            }
        } else {
            index += 1;
        }
    }
    for range in ranges.into_iter().rev() {
        let replacement = if end_block_link_comment_separator(output, range.start) {
            "  "
        } else {
            " "
        };
        output.replace_range(range, replacement);
    }
}

fn end_block_link_comment_separator(output: &str, run_start: usize) -> bool {
    let bytes = output.as_bytes();
    if run_start < 2 || bytes[run_start - 2] != b']' || bytes[run_start - 1] != b']' {
        return false;
    }
    let before = &output[..run_start - 2];
    let Some(open) = before.rfind("[[") else {
        return false;
    };
    let inside = &before[open + 2..];
    inside.starts_with('/') && !inside.contains('\n') && !inside.contains("]]")
}

fn block_head_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    let literal_regions = LiteralRegionIndex::new(text);
    let mut ranges = Vec::new();
    let mut search = 0usize;
    while let Some(relative_open) = text[search..].find("[[") {
        let open = search + relative_open;
        if literal_regions.contains(open) {
            search = open + "[[".len();
            continue;
        }
        if let Some(relative_close) = text[open + "[[".len()..].find("]]") {
            ranges.push(open..open + "[[".len() + relative_close + "]]".len());
        }
        search = open + "[[".len();
    }
    ranges
}

/// Whether a comment's text carries an iftags head. Live Wikidot
/// pairs gates with closers inside comments, so eliding such a
/// comment would remove a gate's closer (or a nested gate's opener)
/// before the parser can pair with it; the comment stays in the text
/// and the comment rule consumes it as usual.
fn comment_contains_iftags_head(comment: &str) -> bool {
    let lower = comment.to_ascii_lowercase();
    lower.contains("[[iftags") || lower.contains("[[/iftags")
}

/// Live Wikidot (anonymous PagePreview, 2026-11-05) elides a column-0 comment
/// and one preceding line break whatever follows it: list markers, headings,
/// rules, separators and gallery heads all stay literal text joined to the
/// previous line. No construct is exempt.
fn exposed_line_start_construct(_tail: &str) -> bool {
    false
}

fn line_remainder_splices(
    source: &str,
    mut pos: usize,
    literal_regions: &LiteralRegionIndex,
    block_name_comments: &std::collections::BTreeSet<usize>,
) -> bool {
    let bytes = source.as_bytes();
    loop {
        while pos < source.len() && matches!(bytes[pos], b' ' | b'\t') {
            pos += 1;
        }
        if pos >= source.len() {
            return false;
        }
        if bytes[pos] == b'\r' || bytes[pos] == b'\n' {
            if bytes[pos] == b'\r' && bytes.get(pos + 1) == Some(&b'\n') {
                pos += 2;
            } else {
                pos += 1;
            }
            match chained_comment_close(source, pos, literal_regions, block_name_comments)
            {
                Some(close) => {
                    pos = close;
                    continue;
                }
                None => return false,
            }
        }
        if source[pos..].starts_with("[!--") {
            match chained_comment_close(source, pos, literal_regions, block_name_comments)
            {
                Some(close) => {
                    pos = close;
                    continue;
                }
                None => return false,
            }
        }
        let line_end = source[pos..]
            .find('\n')
            .map_or(source.len(), |newline| pos + newline);
        return !exposed_line_start_construct(&source[pos..line_end]);
    }
}

fn chained_comment_close(
    source: &str,
    open: usize,
    literal_regions: &LiteralRegionIndex,
    block_name_comments: &std::collections::BTreeSet<usize>,
) -> Option<usize> {
    if !source[open..].starts_with("[!--")
        || literal_regions.contains(open)
        || block_name_comments.contains(&open)
    {
        return None;
    }
    let relative_close = source[open + "[!--".len()..].find("--]")?;
    let close = open + "[!--".len() + relative_close + "--]".len();
    if comment_contains_iftags_head(&source[open..close]) {
        return None;
    }
    Some(close)
}

fn push_copied_range(
    output: &mut String,
    text: &str,
    range: std::ops::Range<usize>,
    break_drops: &[std::ops::Range<usize>],
) {
    let mut cursor = range.start;
    for drop in break_drops {
        if drop.end <= cursor {
            continue;
        }
        if drop.start >= range.end {
            break;
        }
        if drop.start > cursor {
            output.push_str(&text[cursor..drop.start]);
        }
        cursor = cursor.max(drop.end);
    }
    if cursor < range.end {
        output.push_str(&text[cursor..range.end]);
    }
}

fn line_start_break_drop(source: &str, open: usize) -> Option<std::ops::Range<usize>> {
    let line_start = source[..open].rfind('\n').map_or(0, |newline| newline + 1);
    if open != line_start || line_start == 0 {
        return None;
    }

    let bytes = source.as_bytes();
    let mut pos = line_start;
    let mut breaks = 0usize;
    let mut latest_start = line_start;
    let mut drop_start = line_start;
    while pos > 0 {
        let start = if bytes[pos - 1] == b'\n' {
            if pos >= 2 && bytes[pos - 2] == b'\r' {
                pos - 2
            } else {
                pos - 1
            }
        } else if bytes[pos - 1] == b'\r' {
            pos - 1
        } else {
            break;
        };
        breaks += 1;
        drop_start = latest_start;
        latest_start = start;
        pos = start;
    }

    match breaks {
        0 => None,
        1 => Some(latest_start..line_start),
        _ => Some(drop_start..line_start),
    }
}

fn comment_elision_needs_raw_barrier(source: &str, open: usize, close: usize) -> bool {
    let before = source[..open].chars().next_back();
    let after = source[close..].chars().next();
    matches!(
        (before, after),
        (Some('@'), Some('@' | '<')) | (Some('>'), Some('@'))
    )
}

fn leading_comment_remains_an_owner_barrier(
    source: &str,
    open: usize,
    close: usize,
) -> bool {
    let line_start = source[..open].rfind('\n').map_or(0, |newline| newline + 1);
    if !source[line_start..open]
        .bytes()
        .all(|byte| matches!(byte, b' ' | b'\t'))
    {
        return false;
    }

    let tail = source[close..].trim_start_matches([' ', '\t']);
    !exposed_line_start_construct(tail)
}

fn code_closer_comment_starts(source: &str) -> BTreeSet<usize> {
    let mut comments = BTreeSet::new();
    let mut cursor = 0usize;
    while let Some(relative_open) = source[cursor..].find("[!--") {
        let open = cursor + relative_open;
        cursor = open + "[!--".len();
        let before = &source[..open];
        if !before.ends_with("]]") {
            continue;
        }
        let Some(marker) = before.rfind("[[") else {
            continue;
        };
        if !source[marker..].starts_with("[[/") {
            continue;
        }
        let head = &source[marker + "[[/".len()..open - "]]".len()];
        if !head.eq_ignore_ascii_case("code") {
            continue;
        }
        let close_search = open + "[!--".len();
        let Some(relative_close) = source[close_search..].find("--]") else {
            continue;
        };
        let rest = &source[close_search + relative_close + "--]".len()..];
        if rest.is_empty()
            || rest.starts_with([' ', '\t', '\r', '\n'])
            || rest.starts_with("[!--")
        {
            comments.insert(open);
        }
    }
    comments
}

fn iftags_spec_comment_starts(source: &str) -> BTreeSet<usize> {
    let mut comments = BTreeSet::new();
    let mut cursor = 0usize;
    while let Some(relative_open) = source[cursor..].find("[[") {
        let open = cursor + relative_open;
        let mut at = open + 2;
        if source.as_bytes().get(at) == Some(&b'[') {
            cursor = at + 1;
            continue;
        }
        while matches!(source.as_bytes().get(at), Some(b' ' | b'\t')) {
            at += 1;
        }
        let mut name = String::new();
        loop {
            if at >= source.len() {
                break;
            }
            if source[at..].starts_with("[!--") {
                let close_search = at + "[!--".len();
                let Some(relative_close) = source[close_search..].find("--]") else {
                    break;
                };
                at = close_search + relative_close + "--]".len();
                continue;
            }
            let ch = source[at..]
                .chars()
                .next()
                .expect("name scan remains on a character boundary");
            if !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')) {
                break;
            }
            name.push(ch);
            at += ch.len_utf8();
        }
        if name.eq_ignore_ascii_case("iftags") {
            let region_end = source[at..]
                .find("]]")
                .map(|relative| at + relative)
                .unwrap_or(source.len());
            let mut pos = at;
            while pos < region_end {
                let Some(relative) = source[pos..region_end].find("[!--") else {
                    break;
                };
                let comment = pos + relative;
                comments.insert(comment);
                pos = comment + "[!--".len();
            }
        }
        cursor = open + 2;
    }
    comments
}

fn regular_block_name_comment_starts(source: &str) -> BTreeSet<usize> {
    let mut comments = BTreeSet::new();
    let mut cursor = 0usize;
    while let Some(relative_open) = source[cursor..].find("[[") {
        let open = cursor + relative_open;
        let mut at = open + 2;
        if source.as_bytes().get(at) == Some(&b'[') {
            cursor = at + 1;
            continue;
        }
        while matches!(source.as_bytes().get(at), Some(b' ' | b'\t')) {
            at += 1;
        }
        if matches!(source.as_bytes().get(at), Some(b'#' | b'/' | b'*')) {
            cursor = at + 1;
            continue;
        }

        let mut authored_name_byte = false;
        loop {
            if at >= source.len()
                || source[at..].starts_with("]]")
                || matches!(
                    source.as_bytes().get(at),
                    Some(b' ' | b'\t' | b'\r' | b'\n')
                )
            {
                break;
            }
            if source[at..].starts_with("[!--") {
                let close_search = at + "[!--".len();
                let Some(relative_close) = source[close_search..].find("--]") else {
                    break;
                };
                if authored_name_byte {
                    comments.insert(at);
                }
                at = close_search + relative_close + "--]".len();
                continue;
            }
            authored_name_byte = true;
            at += source[at..]
                .chars()
                .next()
                .expect("name scan remains on a character boundary")
                .len_utf8();
        }
        cursor = open + 2;
    }
    comments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_elide_outside_canonical_literal_owners() {
        let mut source = concat!(
            "==[!--x--]==\n",
            "[[code]]\nA[!--kept--]B\n[[/code]]\n",
            "@@A[!--kept--]B@@\n",
            "ht[!--x--]tps://example.com",
        )
        .to_owned();

        substitute_wikidot(&mut source);

        assert_eq!(
            source,
            concat!(
                "====\n",
                "[[code]]\nA[!--kept--]B\n[[/code]]\n",
                "@@A[!--kept--]B@@\n",
                "https://example.com",
            ),
        );
    }

    #[test]
    fn comments_do_not_join_regular_opening_block_names() {
        let mut source = concat!(
            "[[mo[!--x--]dule CSS]]x[[/module]]\n",
            "[[inc[!--x--]lude secret]]\n",
            "[[#i[!--x--]f 1 | yes | no ]]",
        )
        .to_owned();

        substitute_wikidot(&mut source);

        assert!(source.contains("mo[!--x--]dule"), "{source}");
        assert!(source.contains("inc[!--x--]lude"), "{source}");
        assert!(source.contains("[[#if 1 | yes | no ]]"), "{source}");
    }
}
