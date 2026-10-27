//! Untrusted page text must never contain terminal control sequences.
use std::borrow::Cow;

/// True when the text can be passed to the renderer untouched.
fn is_clean(text: &str) -> bool {
    text.bytes().all(|b| b == b'\n' || b == b'\t' || (0x20..0x7f).contains(&b))
        && text.chars().all(|c| !('\u{80}'..='\u{9f}').contains(&c))
}

/// Strip C0/C1 control characters (including ESC, so page text cannot inject
/// escape sequences into the terminal) and normalize line endings.
pub fn sanitize(text: &str) -> Cow<'_, str> {
    if is_clean(text) {
        return Cow::Borrowed(text);
    }
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(normalized.len());
    for c in normalized.chars() {
        match c {
            '\n' | '\t' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{7f}' => {}
            c if ('\u{80}'..='\u{9f}').contains(&c) => {}
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// Collapse HTML whitespace runs to a single space, trimming the run.
pub fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_text_is_borrowed() {
        let t = "plain text with a newline\nand a tab\t";
        assert!(matches!(sanitize(t), Cow::Borrowed(_)));
    }

    #[test]
    fn strips_escape_sequences() {
        assert_eq!(sanitize("\x1b]0;pwned\x07"), "]0;pwned");
        assert_eq!(sanitize("\x1b[31mred\x1b[0m"), "[31mred[0m");
        assert_eq!(sanitize("\x1b[1;1Hoverwrote the screen"), "[1;1Hoverwrote the screen");
    }

    #[test]
    fn strips_control_characters() {
        assert_eq!(sanitize("a\0b\u{7}c\u{80}d\u{1b}e"), "abcde");
        assert_eq!(sanitize("del\u{7f}"), "del");
    }

    #[test]
    fn keeps_newlines_and_tabs() {
        assert_eq!(sanitize("a\r\nb\tc\rd"), "a\nb\tc\nd");
    }

    #[test]
    fn keeps_unicode() {
        assert_eq!(sanitize("héllo wörld 日本語 🎉"), "héllo wörld 日本語 🎉");
    }

    #[test]
    fn collapses_whitespace() {
        assert_eq!(collapse_whitespace("  a\n\n  b\t\tc  "), "a b c");
    }
}
