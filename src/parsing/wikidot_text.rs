use std::borrow::Cow;

#[inline]
pub(crate) fn trim_wikidot_ascii(value: &str) -> &str {
    value.trim_matches([' ', '\t', '\r', '\n'])
}

pub(crate) fn trim_cow<'a>(value: Cow<'a, str>) -> Cow<'a, str> {
    match value {
        Cow::Borrowed(value) => Cow::Borrowed(value.trim()),
        Cow::Owned(value) => Cow::Owned(value.trim().to_owned()),
    }
}

pub(crate) fn trim_wikidot_ascii_cow<'a>(value: Cow<'a, str>) -> Cow<'a, str> {
    match value {
        Cow::Borrowed(value) => Cow::Borrowed(trim_wikidot_ascii(value)),
        Cow::Owned(value) => {
            let trimmed = trim_wikidot_ascii(&value);
            if trimmed.len() == value.len() {
                Cow::Owned(value)
            } else {
                Cow::Owned(trimmed.to_owned())
            }
        }
    }
}

pub(crate) fn discard_wikidot_controls<'a>(value: Cow<'a, str>) -> Cow<'a, str> {
    if !value.chars().any(is_discarded_control) {
        return value;
    }
    Cow::Owned(
        value
            .chars()
            .filter(|&ch| !is_discarded_control(ch))
            .collect(),
    )
}

#[inline]
fn is_discarded_control(ch: char) -> bool {
    matches!(ch as u32, 0x00..=0x08 | 0x0b..=0x0c | 0x0e..=0x1a | 0x1c..=0x1f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_text_helpers_preserve_nbsp_and_delete_discarded_c0() {
        assert_eq!(trim_wikidot_ascii(" \t\u{a0}x\u{a0}\t "), "\u{a0}x\u{a0}");
        assert_eq!(discard_wikidot_controls(Cow::Borrowed("A\0\u{b}B")), "AB");
    }
}
