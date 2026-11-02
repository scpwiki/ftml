use super::parser_functions::LiteralRegionIndex;
use std::collections::BTreeSet;

pub(super) fn substitute_wikidot(text: &mut String) {
    if !text.contains("[!--") {
        return;
    }

    let literal_regions = LiteralRegionIndex::new(text);
    let block_name_comments = regular_block_name_comment_starts(text);
    let mut output = String::with_capacity(text.len());
    let mut copied_until = 0usize;
    let mut scan = 0usize;
    let mut removed = false;

    while let Some(relative_open) = text[scan..].find("[!--") {
        let open = scan + relative_open;
        if literal_regions.contains(open) || block_name_comments.contains(&open) {
            scan = open + "[!--".len();
            continue;
        }

        let close_search = open + "[!--".len();
        let Some(relative_close) = text[close_search..].find("--]") else {
            break;
        };
        let close = close_search + relative_close + "--]".len();
        if leading_comment_remains_an_owner_barrier(text, open, close) {
            scan = close;
            continue;
        }
        output.push_str(&text[copied_until..open]);
        if comment_elision_needs_raw_barrier(text, open, close) {
            output.push('\0');
        }
        copied_until = close;
        scan = close;
        removed = true;
    }

    if removed {
        output.push_str(&text[copied_until..]);
        *text = output;
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
    let lower = tail
        .get(..tail.len().min("[[gallery".len()))
        .unwrap_or(tail);
    let exposed = tail.starts_with("~~~~")
        || tail.starts_with('+')
        || tail.starts_with("----")
        || tail.starts_with("* ")
        || tail.starts_with("# ")
        || tail.starts_with("====")
        || lower.eq_ignore_ascii_case("[[gallery");
    !exposed
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
