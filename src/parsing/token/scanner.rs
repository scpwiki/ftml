use super::{ExtractedToken, Token};

pub fn extract_all(text: &str) -> Vec<ExtractedToken<'_>> {
    extract_all_with_trailing_comment_closer(text, false)
}

pub(super) fn extract_all_with_trailing_comment_closer(
    text: &str,
    trailing_comment_closer: bool,
) -> Vec<ExtractedToken<'_>> {
    extract_all_with_comment_scan_visits(text, trailing_comment_closer).0
}

fn extract_all_with_comment_scan_visits(
    text: &str,
    trailing_comment_closer: bool,
) -> (Vec<ExtractedToken<'_>>, usize) {
    let mut tokens = Vec::with_capacity(text.len() / 2 + 2);
    push(&mut tokens, text, Token::InputStart, 0, 0);

    let bytes = text.as_bytes();
    let mut comment_closers = CommentCloserCursor::new(bytes);
    let mut comment_active = false;
    let mut comment_closer_bracket = None;
    let mut bracket_run = None::<(u8, usize)>;
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        let bracket_run_offset = match bytes[start] {
            symbol @ (b'[' | b']') => match bracket_run {
                Some((active, run_start)) if active == symbol => start - run_start,
                _ => {
                    bracket_run = Some((symbol, start));
                    0
                }
            },
            _ => {
                bracket_run = None;
                0
            }
        };
        let force_right_bracket = comment_closer_bracket == Some(start);
        if force_right_bracket {
            comment_closer_bracket = None;
        }
        if comment_active && let Some(bracket) = comment_closer_bracket_at(bytes, start) {
            comment_active = false;
            comment_closer_bracket = Some(bracket);
        }
        let valid_comment_opener = has(bytes, start, b"[!--")
            && (trailing_comment_closer
                || comment_closers.has_at_or_after(start + b"[!--".len()));
        let was_comment_active = comment_active;
        let (token, end) = next_token(
            text,
            bytes,
            start,
            valid_comment_opener,
            force_right_bracket,
            bracket_run_offset,
        );
        if was_comment_active
            && text[start..end].ends_with("--")
            && bytes.get(end) == Some(&b']')
        {
            comment_active = false;
            comment_closer_bracket = Some(end);
        }
        if token == Token::LeftComment {
            comment_active = true;
        }
        push(&mut tokens, text, token, start, end);
        index = end;
    }

    push(&mut tokens, text, Token::InputEnd, text.len(), text.len());
    (tokens, comment_closers.scan_visits)
}

struct CommentCloserCursor<'a> {
    bytes: &'a [u8],
    initialized: bool,
    next: Option<usize>,
    search_start: usize,
    scan_visits: usize,
}

impl<'a> CommentCloserCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            initialized: false,
            next: None,
            search_start: 0,
            scan_visits: 0,
        }
    }

    fn has_at_or_after(&mut self, minimum: usize) -> bool {
        if !self.initialized {
            self.initialized = true;
            self.find_next();
        }
        while self.next.is_some_and(|position| position < minimum) {
            self.find_next();
        }
        self.next.is_some()
    }

    fn find_next(&mut self) {
        while self.search_start + 2 < self.bytes.len() {
            let position = self.search_start;
            self.search_start += 1;
            self.scan_visits += 1;
            if has(self.bytes, position, b"--]") {
                self.next = Some(position);
                return;
            }
        }
        self.search_start = self.bytes.len();
        self.next = None;
    }
}

fn push<'t>(
    tokens: &mut Vec<ExtractedToken<'t>>,
    text: &'t str,
    token: Token,
    start: usize,
    end: usize,
) {
    tokens.push(ExtractedToken {
        token,
        slice: &text[start..end],
        span: start..end,
    });
}

fn next_token(
    text: &str,
    bytes: &[u8],
    start: usize,
    valid_comment_opener: bool,
    force_right_bracket: bool,
    bracket_run_offset: usize,
) -> (Token, usize) {
    let byte = bytes[start];
    if is_discarded_control(byte) {
        return (Token::DiscardedControl, start + 1);
    }
    if byte.is_ascii_alphanumeric() {
        if matches!(byte, b'f' | b'h' | b'm')
            && let Some(end) = scan_url(bytes, start)
        {
            return (Token::Url, end);
        }

        return scan_identifier_or_email(bytes, start);
    }

    if let Some(end) = scan_variable(bytes, start) {
        return (Token::Variable, end);
    }

    if byte == b'\\' && bytes.get(start + 1) == Some(&b'\n') {
        return (Token::LineBreak, start + 2);
    }

    if let Some((token, end)) = scan_newline(bytes, start) {
        return (token, end);
    }

    if matches!(byte, b' ' | b'\t') {
        return (Token::Whitespace, scan_space(bytes, start));
    }

    if byte == b'*' {
        if let Some(end) = scan_url(bytes, start + 1) {
            return (Token::UrlStar, end);
        }
        if has(bytes, start, b"**") && scan_url(bytes, start + 2).is_some() {
            return (Token::Other, start + 1);
        }
    }

    if let Some((token, end)) = scan_literal(
        bytes,
        start,
        valid_comment_opener,
        force_right_bracket,
        bracket_run_offset,
    ) {
        return (token, end);
    }

    if let Some((token, end)) = scan_repeated_symbol(bytes, start) {
        return (token, end);
    }

    (Token::Other, next_char_end(text, start))
}

fn is_discarded_control(byte: u8) -> bool {
    matches!(byte, 0x00..=0x08 | 0x0b..=0x0c | 0x0e..=0x1a | 0x1c..=0x1f)
}

fn scan_literal(
    bytes: &[u8],
    start: usize,
    valid_comment_opener: bool,
    force_right_bracket: bool,
    bracket_run_offset: usize,
) -> Option<(Token, usize)> {
    let result = match bytes[start] {
        b'@' if has(bytes, start, b"@@") => (Token::Raw, start + 2),
        b'@' if has(bytes, start, b"@<") => (Token::LeftRaw, start + 2),
        b'>' if has(bytes, start, b">@") => (Token::RightRaw, start + 2),
        b'[' if valid_comment_opener => (Token::LeftComment, start + 4),
        b'[' if has(bytes, start, b"[[[[") && bracket_run_offset.is_multiple_of(4) => {
            (Token::LeftBracket, start + 1)
        }
        b'[' if has(bytes, start, b"[[[*") => (Token::LeftLinkStar, start + 4),
        b'[' if has(bytes, start, b"[[[") => (Token::LeftLink, start + 3),
        b'[' if has(bytes, start, b"[[$") => (Token::LeftMath, start + 3),
        b'[' if has(bytes, start, b"[[#") && !has(bytes, start, b"[[##") => {
            (Token::LeftBlockAnchor, start + 3)
        }
        b'[' if has(bytes, start, b"[[*") => (Token::LeftBlockStar, start + 3),
        b'[' if has(bytes, start, b"[[/") => (Token::LeftBlockEnd, start + 3),
        b'[' if has(bytes, start, b"[[") => (Token::LeftBlock, start + 2),
        b'[' if has(bytes, start, b"[#") => (Token::LeftBracketAnchor, start + 2),
        b'[' if has(bytes, start, b"[*") && !has(bytes, start, b"[**") => {
            (Token::LeftBracketStar, start + 2)
        }
        b'[' => (Token::LeftBracket, start + 1),
        b'(' if has(bytes, start, b"((") => (Token::LeftParentheses, start + 2),
        b']' if force_right_bracket => (Token::RightBracket, start + 1),
        b']' if has(bytes, start, b"]]]")
            && !is_right_link_trailing_bracket(bracket_run_offset) =>
        {
            (Token::RightLink, start + 3)
        }
        b'$' if has(bytes, start, b"$]]") => (Token::RightMath, start + 3),
        b']' if has(bytes, start, b"]]")
            && !is_right_link_trailing_bracket(bracket_run_offset) =>
        {
            (Token::RightBlock, start + 2)
        }
        b']' => (Token::RightBracket, start + 1),
        b')' if has(bytes, start, b"))") => (Token::RightParentheses, start + 2),
        b'*' if has(bytes, start, b"**") => (Token::Bold, start + 2),
        b'/' if has(bytes, start, b"//") => (Token::Italics, start + 2),
        b'_' if has(bytes, start, b"__") => (Token::Underline, start + 2),
        b'^' if has(bytes, start, b"^^") => (Token::Superscript, start + 2),
        b',' if has(bytes, start, b",,") => (Token::Subscript, start + 2),
        b'#' if has(bytes, start, b"##") => (Token::Color, start + 2),
        b'{' if has(bytes, start, b"{{") => (Token::LeftMonospace, start + 2),
        b'}' if has(bytes, start, b"}}") => (Token::RightMonospace, start + 2),
        b'|' if has(bytes, start, b"||~") => (Token::TableColumnTitle, start + 3),
        b'|' if has(bytes, start, b"||>") => (Token::TableColumnRight, start + 3),
        b'|' if has(bytes, start, b"||=") => (Token::TableColumnCenter, start + 3),
        b'|' if has(bytes, start, b"||") => (Token::TableColumn, start + 2),
        b'<' if has(bytes, start, b"<<") => (Token::LeftDoubleAngle, start + 2),
        b'|' => (Token::Pipe, start + 1),
        b'=' => (Token::Equals, start + 1),
        b':' => (Token::Colon, start + 1),
        b'_' => (Token::Underscore, start + 1),
        b'\\' if has(bytes, start, br#"\""#) => (Token::EscapedDoubleQuote, start + 2),
        b'"' => (Token::DoubleQuote, start + 1),
        b'\\' if has(bytes, start, br#"\\"#) => (Token::EscapedBackslash, start + 2),
        b'\\' if start + 1 == bytes.len() => (Token::LineBreak, start + 1),
        b'*' if bytes.get(start + 1) != Some(&b'*') => (Token::BulletItem, start + 1),
        b'#' if bytes.get(start + 1) != Some(&b'#') => (Token::NumberedItem, start + 1),
        _ => return None,
    };

    Some(result)
}

fn is_right_link_trailing_bracket(bracket_run_offset: usize) -> bool {
    bracket_run_offset % 4 == 3
}

fn scan_repeated_symbol(bytes: &[u8], start: usize) -> Option<(Token, usize)> {
    match bytes[start] {
        b'~' => {
            let end = scan_same(bytes, start, b'~');
            let count = end - start;
            if count >= 4 {
                if bytes.get(end) == Some(&b'<') {
                    Some((Token::ClearFloatLeft, end + 1))
                } else if bytes.get(end) == Some(&b'>') {
                    Some((Token::ClearFloatRight, end + 1))
                } else {
                    Some((Token::ClearFloatBoth, end))
                }
            } else if count >= 2 {
                Some((Token::DoubleTilde, start + 2))
            } else {
                None
            }
        }
        b'-' => {
            let end = scan_same(bytes, start, b'-');
            match end - start {
                2 => Some((Token::DoubleDash, end)),
                3 => Some((Token::DoubleDash, start + 2)),
                count if count >= 4 => Some((Token::TripleDash, end)),
                _ => None,
            }
        }
        b'>' => Some((Token::Quote, scan_same(bytes, start, b'>'))),
        b'+' => {
            let plus_end = scan_plus_heading(bytes, start);
            if plus_end > start {
                Some((Token::Heading, plus_end))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn scan_plus_heading(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while end < bytes.len() && bytes[end] == b'+' && end - start < 6 {
        end += 1;
    }

    if end == start {
        return start;
    }

    if bytes.get(end) == Some(&b'*') && bytes.get(end + 1) != Some(&b'*') {
        end + 1
    } else {
        end
    }
}

fn scan_url(bytes: &[u8], start: usize) -> Option<usize> {
    let body_start = if has(bytes, start, b"http://") {
        start + 7
    } else if has(bytes, start, b"https://") {
        start + 8
    } else if has(bytes, start, b"ftp://") {
        start + 6
    } else if has(bytes, start, b"mailto:") {
        start + 7
    } else {
        return None;
    };

    let mut end = body_start;
    while end < bytes.len() {
        let character = char_at(bytes, end)?;
        if matches!(
            bytes[end],
            b'\n' | b'\r' | b' ' | b'\t' | b'"' | b'\'' | b'|' | b'[' | b']'
        ) || character.is_whitespace()
            || is_discarded_control(bytes[end])
            || has(bytes, end, b">@")
            || has(bytes, end, b"@@")
        {
            break;
        }
        end += character.len_utf8();
    }

    (end > body_start).then_some(end)
}

fn char_at(bytes: &[u8], index: usize) -> Option<char> {
    let width = match *bytes.get(index)? {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => return None,
    };
    std::str::from_utf8(bytes.get(index..index + width)?)
        .ok()?
        .chars()
        .next()
}

fn scan_identifier_or_email(bytes: &[u8], start: usize) -> (Token, usize) {
    debug_assert!(bytes[start].is_ascii_alphanumeric());

    let identifier_end = scan_identifier(bytes, start);
    match bytes.get(identifier_end) {
        Some(b' ' | b'\t' | b'\n' | b'\r') | None => {
            return (Token::Identifier, identifier_end);
        }
        _ => {}
    }

    let local_end = scan_email_dotted(bytes, start, is_email_local_byte);
    if bytes.get(local_end) != Some(&b'@') {
        return (Token::Identifier, identifier_end);
    }
    let domain_start = local_end + 1;
    let domain_end = scan_email_dotted(bytes, domain_start, is_email_label_byte);
    let has_second_label = bytes[domain_start..domain_end].contains(&b'.');
    if domain_end == domain_start || !has_second_label {
        return (Token::Identifier, identifier_end);
    }

    (Token::Email, domain_end)
}

fn is_email_local_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn is_email_label_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-'
}

fn scan_email_dotted(bytes: &[u8], start: usize, part: fn(u8) -> bool) -> usize {
    let mut end = start;
    while bytes.get(end).copied().is_some_and(part) {
        end += 1;
        if bytes.get(end) == Some(&b'.') && bytes.get(end + 1).copied().is_some_and(part)
        {
            end += 1;
        }
    }
    end
}

fn scan_identifier(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while end < bytes.len() && bytes[end].is_ascii_alphanumeric() {
        end += 1;
    }
    end
}

fn scan_variable(bytes: &[u8], start: usize) -> Option<usize> {
    if !has(bytes, start, b"{$") {
        return None;
    }

    let identifier_start = start + 2;
    let identifier_end = scan_identifier(bytes, identifier_start);
    if identifier_end == identifier_start || bytes.get(identifier_end) != Some(&b'}') {
        return None;
    }

    Some(identifier_end + 1)
}

fn scan_newline(bytes: &[u8], start: usize) -> Option<(Token, usize)> {
    let mut end = start;
    let mut count = 0;

    while let Some(next) = scan_newline_once(bytes, end) {
        end = next;
        count += 1;
    }

    match count {
        0 => None,
        1 => Some((Token::LineBreak, end)),
        _ => Some((Token::ParagraphBreak, end)),
    }
}

fn scan_newline_once(bytes: &[u8], start: usize) -> Option<usize> {
    match bytes.get(start) {
        Some(b'\r') if bytes.get(start + 1) == Some(&b'\n') => Some(start + 2),
        Some(b'\r' | b'\n') => Some(start + 1),
        _ => None,
    }
}

fn scan_space(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while end < bytes.len() && matches!(bytes[end], b' ' | b'\t') {
        end += 1;
    }
    end
}

fn scan_same(bytes: &[u8], start: usize, byte: u8) -> usize {
    let mut end = start;
    while end < bytes.len() && bytes[end] == byte {
        end += 1;
    }
    end
}

fn has(bytes: &[u8], start: usize, literal: &[u8]) -> bool {
    bytes
        .get(start..start.saturating_add(literal.len()))
        .is_some_and(|candidate| candidate == literal)
}

fn comment_closer_bracket_at(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'-') {
        return None;
    }
    let end = scan_same(bytes, start, b'-');
    (end - start >= 2 && bytes.get(end) == Some(&b']')).then_some(end)
}

fn next_char_end(text: &str, start: usize) -> usize {
    if text.as_bytes()[start].is_ascii() {
        start + 1
    } else {
        start
            + text[start..]
                .chars()
                .next()
                .expect("valid UTF-8")
                .len_utf8()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_continued_boundary_is_one_line_break_token() {
        let tokens = extract_all("\\\n");

        assert_eq!(tokens.len(), 3);
        assert_eq!(tokens[0].token, Token::InputStart);
        assert_eq!(tokens[1].token, Token::LineBreak);
        assert_eq!(tokens[1].slice, "\\\n");
        assert_eq!(tokens[2].token, Token::InputEnd);
    }

    #[test]
    fn malformed_comment_candidate_scan_is_linear_and_token_memory_is_bounded() {
        let input = "A[!--hidden------ ]B".repeat(32_768);
        let (tokens, comment_scan_visits) =
            extract_all_with_comment_scan_visits(&input, false);

        assert!(comment_scan_visits <= input.len());
        assert!(tokens.len() <= input.len() + 2);
        assert!(tokens.iter().all(|token| token.token != Token::LeftComment),);
    }

    #[test]
    fn comment_closer_scan_is_lazy_for_inputs_without_candidates() {
        let input = "ordinary --] punctuation".repeat(4_096);
        let (_, comment_scan_visits) =
            extract_all_with_comment_scan_visits(&input, false);

        assert_eq!(comment_scan_visits, 0);
    }

    #[test]
    fn active_comment_closer_owns_only_the_first_bracket_of_a_run() {
        let active = extract_all("[!--x--]]]");
        let active_tokens = active.iter().map(|token| token.token).collect::<Vec<_>>();
        assert_eq!(
            active_tokens,
            vec![
                Token::InputStart,
                Token::LeftComment,
                Token::Identifier,
                Token::DoubleDash,
                Token::RightBracket,
                Token::RightBlock,
                Token::InputEnd,
            ],
        );

        let ordinary = extract_all("x--]]]");
        assert!(ordinary.iter().any(|token| token.token == Token::RightLink),);
    }

    #[test]
    fn email_tokens_leave_pipe_ownership_to_the_parser() {
        for input in [
            "MAILTO:User@Example.COM|Label",
            "MailTo:User@Example.COM|Label",
            "mailto:o'hara@example.com|Label",
        ] {
            let tokens = extract_all(input);
            assert!(
                tokens.iter().any(|token| token.token == Token::Pipe),
                "{input}: {tokens:#?}"
            );
            assert!(
                tokens
                    .iter()
                    .filter(|token| token.token == Token::Email)
                    .all(|token| !token.slice.contains('|')),
                "{input}: {tokens:#?}",
            );
        }
    }

    #[test]
    fn url_scan_decodes_multibyte_characters_and_stops_at_unicode_whitespace() {
        let url = |input: &str| {
            extract_all(input)
                .into_iter()
                .find(|token| token.token == Token::Url)
                .map(|token| token.slice.to_owned())
        };

        assert_eq!(
            url("see https://example.com/ü日本🎉 then"),
            Some("https://example.com/ü日本🎉".to_owned()),
        );
        assert_eq!(
            url("https://example.com/日本\u{3000}after"),
            Some("https://example.com/日本".to_owned()),
        );
        assert_eq!(char_at("ü".as_bytes(), 1), None);
    }

    #[test]
    fn many_urls_in_a_long_input_tokenize_in_linear_time() {
        let input = "see https://scp-wiki.wikidot.com/scp-173 and more\n".repeat(32_768);
        let started = std::time::Instant::now();
        let tokens = extract_all(&input);

        assert_eq!(
            tokens
                .iter()
                .filter(|token| token.token == Token::Url)
                .count(),
            32_768,
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            started.elapsed(),
        );
    }

    #[test]
    fn leading_space_before_equals_is_a_separate_token() {
        let tokens = Token::extract_all("A\n ====\nB");
        let kinds = tokens.iter().map(|token| token.token).collect::<Vec<_>>();
        assert!(
            kinds.windows(3).any(|window| {
                window == [Token::LineBreak, Token::Whitespace, Token::Equals]
            }),
            "{kinds:?}",
        );
    }
}
