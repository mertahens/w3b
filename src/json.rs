//! JSON that is read rather than run. The body of a JSON response is laid out by
//! nesting, coloured by role, and left as ordinary characters, so `/` searches it
//! like any other text and `y` can still copy the address of the document itself.
use crate::dom::{Block, BlockKind, Doc, Piece};

/// What a stretch of characters is, as far as someone reading JSON is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Role {
    /// Indentation, or anything w3b has no colour for.
    #[default]
    Plain,
    /// The `"key"` in front of a `:`.
    Key,
    /// Any other quoted string.
    String,
    Number,
    /// `true`, `false` and `null`.
    Literal,
    /// `{`, `}`, `[`, `]`, `:` and `,`.
    Punct,
}

/// Two spaces a level: enough to see the nesting, narrow enough that a typical
/// API object still fits the terminal without wrapping.
const INDENT: &str = "  ";

/// How deep nesting may go before w3b stops calling the body JSON. Far past any
/// real response, and shallow enough that a hostile body cannot grow the stack.
const MAX_DEPTH: usize = 64;

type Tok<'a> = (Role, &'a str);

/// Whether the whole body is one JSON value. Used to second-guess a server that
/// calls JSON `application/octet-stream`; it scans, it does not build a value tree.
pub fn is_json(src: &str) -> bool {
    scan(src).is_some()
}

/// The body laid out one entry per line, each line a list of coloured spans.
/// `None` when the bytes are not JSON, which is the caller's cue to show them raw.
pub fn lines(src: &str) -> Option<Vec<Vec<(Role, String)>>> {
    let toks = scan(src)?;
    let mut out: Vec<Vec<(Role, String)>> = Vec::new();
    let mut cur: Vec<(Role, String)> = Vec::new();
    let mut depth = 0usize;
    let mut prev = "";
    for (role, tok) in toks {
        let close = tok == "}" || tok == "]";
        if close {
            depth = depth.saturating_sub(1);
        }
        let open_prev = prev == "{" || prev == "[";
        // A line ends after every opening bracket and every comma, and before
        // every closing one - except an empty container, which stays `[]` whole.
        let brk = !cur.is_empty() && (open_prev || prev == "," || close) && !(open_prev && close);
        if brk {
            out.push(std::mem::take(&mut cur));
        }
        if cur.is_empty() {
            let pad = INDENT.repeat(depth);
            if !pad.is_empty() {
                cur.push((Role::Plain, pad));
            }
        }
        // The one space a reader expects, and it belongs to the punctuation so a
        // search for `"key":` does not straddle two runs.
        if tok == ":" {
            cur.push((Role::Punct, ": ".to_string()));
        } else {
            cur.push((role, tok.to_string()));
        }
        if tok == "{" || tok == "[" {
            depth += 1;
        }
        prev = tok;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    Some(out)
}

/// A page document for the body: one preformatted block per line, monospace so it
/// sits on the same dark slab a `<pre>` gets, and coloured by role.
pub fn doc(src: &str) -> Option<Doc> {
    let body = lines(src)?;
    // The gutter is as wide as the last line needs, so the numbers line up on their
    // right edge and the body starts at the same column on every line.
    let gutter = body.len().to_string().len();
    let mut doc = Doc::default();
    for (n, line) in body.into_iter().enumerate() {
        let mut block = Block { kind: BlockKind::Pre, indent: 0, pieces: Vec::new(), hidden_by: None };
        block.pieces.push(Piece { text: format!("{:>gutter$} ", n + 1), mono: true, muted: true, ..plain() });
        for (role, text) in line {
            block.pieces.push(Piece { text, mono: true, json: role, ..plain() });
        }
        doc.blocks.push(block);
    }
    Some(doc)
}

fn plain() -> Piece {
    Piece {
        text: String::new(),
        link: None,
        mono: false,
        bold: false,
        italic: false,
        code: false,
        muted: false,
        media: None,
        field: None,
        disclosure: None,
        json: Role::Plain,
        hard_break: false,
    }
}

// The scanner. Every token keeps its slice of the original text, so a key is
// shown exactly as it was written - including a key order that a parse into a map
// would quietly sort away.

fn scan(src: &str) -> Option<Vec<Tok<'_>>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    skip_ws(src, &mut i);
    value(src, &mut i, &mut out, 0)?;
    skip_ws(src, &mut i);
    // Trailing garbage means the body is not one JSON value, whatever the header
    // said, and showing it raw is the honest answer.
    if i != src.len() {
        return None;
    }
    Some(out)
}

fn at(src: &str, i: usize) -> Option<u8> {
    src.as_bytes().get(i).copied()
}

fn skip_ws(src: &str, i: &mut usize) {
    while matches!(at(src, *i), Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r')) {
        *i += 1;
    }
}

fn value<'a>(src: &'a str, i: &mut usize, out: &mut Vec<Tok<'a>>, depth: usize) -> Option<()> {
    if depth > MAX_DEPTH {
        return None;
    }
    skip_ws(src, i);
    match at(src, *i)? {
        b'{' => {
            out.push((Role::Punct, "{"));
            *i += 1;
            skip_ws(src, i);
            if at(src, *i) == Some(b'}') {
                out.push((Role::Punct, "}"));
                *i += 1;
                return Some(());
            }
            loop {
                skip_ws(src, i);
                string(src, i, out, Role::Key)?;
                skip_ws(src, i);
                if at(src, *i) != Some(b':') {
                    return None;
                }
                out.push((Role::Punct, ":"));
                *i += 1;
                value(src, i, out, depth + 1)?;
                skip_ws(src, i);
                match at(src, *i) {
                    Some(b',') => {
                        out.push((Role::Punct, ","));
                        *i += 1;
                    }
                    Some(b'}') => {
                        out.push((Role::Punct, "}"));
                        *i += 1;
                        return Some(());
                    }
                    _ => return None,
                }
            }
        }
        b'[' => {
            out.push((Role::Punct, "["));
            *i += 1;
            skip_ws(src, i);
            if at(src, *i) == Some(b']') {
                out.push((Role::Punct, "]"));
                *i += 1;
                return Some(());
            }
            loop {
                value(src, i, out, depth + 1)?;
                skip_ws(src, i);
                match at(src, *i) {
                    Some(b',') => {
                        out.push((Role::Punct, ","));
                        *i += 1;
                    }
                    Some(b']') => {
                        out.push((Role::Punct, "]"));
                        *i += 1;
                        return Some(());
                    }
                    _ => return None,
                }
            }
        }
        b'"' => string(src, i, out, Role::String)?,
        b't' => word(src, i, out, "true")?,
        b'f' => word(src, i, out, "false")?,
        b'n' => word(src, i, out, "null")?,
        c if c == b'-' || c.is_ascii_digit() => number(src, i, out)?,
        _ => return None,
    }
    Some(())
}

fn word<'a>(src: &'a str, i: &mut usize, out: &mut Vec<Tok<'a>>, what: &'static str) -> Option<()> {
    if !src.as_bytes()[*i..].starts_with(what.as_bytes()) {
        return None;
    }
    out.push((Role::Literal, what));
    *i += what.len();
    Some(())
}

fn string<'a>(src: &'a str, i: &mut usize, out: &mut Vec<Tok<'a>>, role: Role) -> Option<()> {
    if at(src, *i) != Some(b'"') {
        return None;
    }
    let start = *i;
    *i += 1;
    loop {
        match at(src, *i)? {
            // A delimiter byte is never part of a multi-byte character, so the
            // slice boundaries below always land between characters.
            b'"' => {
                *i += 1;
                out.push((role, &src[start..*i]));
                return Some(());
            }
            b'\\' => {
                *i += 1;
                match at(src, *i)? {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => *i += 1,
                    b'u' => {
                        *i += 1;
                        for _ in 0..4 {
                            if !at(src, *i)?.is_ascii_hexdigit() {
                                return None;
                            }
                            *i += 1;
                        }
                    }
                    _ => return None,
                }
            }
            // A raw control character - a newline especially - is not JSON.
            c if c < 0x20 => return None,
            _ => *i += 1,
        }
    }
}

fn number<'a>(src: &'a str, i: &mut usize, out: &mut Vec<Tok<'a>>) -> Option<()> {
    let start = *i;
    if at(src, *i) == Some(b'-') {
        *i += 1;
    }
    let int = *i;
    while at(src, *i).is_some_and(|c| c.is_ascii_digit()) {
        *i += 1;
    }
    if *i == int || (at(src, int) == Some(b'0') && *i > int + 1) {
        // no digits at all, or a leading zero such as `01`
        return None;
    }
    if at(src, *i) == Some(b'.') {
        *i += 1;
        let frac = *i;
        while at(src, *i).is_some_and(|c| c.is_ascii_digit()) {
            *i += 1;
        }
        if *i == frac {
            return None;
        }
    }
    if matches!(at(src, *i), Some(b'e') | Some(b'E')) {
        *i += 1;
        if matches!(at(src, *i), Some(b'+') | Some(b'-')) {
            *i += 1;
        }
        let exp = *i;
        while at(src, *i).is_some_and(|c| c.is_ascii_digit()) {
            *i += 1;
        }
        if *i == exp {
            return None;
        }
    }
    out.push((Role::Number, &src[start..*i]));
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(src: &str) -> String {
        lines(src)
            .expect("json")
            .iter()
            .map(|l| l.iter().map(|(_, t)| t.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn an_object_is_one_entry_a_line_with_the_nesting_shown() {
        assert_eq!(
            flat(r#"{"name":"w3b","stars":100,"on":true,"tags":["tui","browser"],"empty":{},"nested":{"a":[1,{"b":null}]}}"#),
            "{\n  \"name\": \"w3b\",\n  \"stars\": 100,\n  \"on\": true,\n  \"tags\": [\n    \"tui\",\n    \"browser\"\n  ],\n  \"empty\": {},\n  \"nested\": {\n    \"a\": [\n      1,\n      {\n        \"b\": null\n      }\n    ]\n  }\n}"
        );
    }

    #[test]
    fn each_part_carries_its_own_role() {
        let roles = lines(r#"{"a":"b","n":-1.5e3,"t":false,"z":null}"#)
            .expect("json")
            .iter()
            .flatten()
            .map(|(r, t)| (*r, t.to_string()))
            .filter(|(_, t)| !t.trim().is_empty())
            .collect::<Vec<_>>();
        assert!(roles.contains(&(Role::Key, "\"a\"".to_string())), "{roles:?}");
        assert!(roles.contains(&(Role::String, "\"b\"".to_string())), "{roles:?}");
        assert!(roles.contains(&(Role::Number, "-1.5e3".to_string())), "{roles:?}");
        assert!(roles.contains(&(Role::Literal, "false".to_string())), "{roles:?}");
        assert!(roles.contains(&(Role::Literal, "null".to_string())), "{roles:?}");
        assert!(roles.contains(&(Role::Punct, ",".to_string())), "{roles:?}");
        assert!(roles.contains(&(Role::Punct, ": ".to_string())), "{roles:?}");
    }

    #[test]
    fn the_keys_keep_the_order_the_server_wrote_them_in() {
        // A parse into a map would sort these; a reader wants them as sent.
        let text = flat(r#"{"z":1,"a":2,"m":3}"#);
        let at = |k: &str| text.find(k).unwrap_or(usize::MAX);
        assert!(at("\"z\"") < at("\"a\"") && at("\"a\"") < at("\"m\""), "{text}");
    }

    #[test]
    fn what_is_not_json_is_said_so_and_shown_raw() {
        for bad in [
            "",
            "{",
            "{\"a\":1,}",
            "[1,]",
            "{\"a\" 1}",
            "nope",
            "01",
            "{'a':1}",
            "[1 2]",
            "\"open",
            "tru",
            "nul",
            "{\"a\":1} more",
            "[1\n2]",
            "{\"a\":\"b\nc\"}",
        ] {
            assert!(!is_json(bad), "should not read as json: {bad:?}");
        }
    }

    #[test]
    fn any_json_value_is_a_body_not_only_an_object() {
        for good in ["[]", "{}", "3", "-0.5", "1e9", "\"hi\"", "null", "true", "  {\n  \"a\": []\n}\n"] {
            assert!(is_json(good), "should read as json: {good:?}");
        }
        // Escapes are part of the string, not a break in it.
        assert!(is_json(r#"{"a":"\u00e9\t\"q\""}"#));
        // Whitespace anywhere a reader puts it: the format a server pretty-prints
        // in, with its spaces after commas and around colons.
        assert!(is_json("{ \"a\" : 1,\n  \"b\" : [ 2 , 3 ], \"c\" : { \"d\" : 4 } }\n"));
    }

    #[test]
    fn nesting_past_the_cap_is_not_json_rather_than_a_crash() {
        let deep = "[".repeat(200) + &"]".repeat(200);
        assert!(!is_json(&deep), "a hostile body must not be laid out");
        let fine = "[".repeat(20) + &"]".repeat(20);
        assert!(is_json(&fine), "20 levels is a normal document");
    }

    #[test]
    fn the_gutter_numbers_the_lines_of_the_body() {
        let items: Vec<String> = (1..=12).map(|n| format!(r#""item{n}""#)).collect();
        let src = format!("[{}]", items.join(","));
        let doc = doc(&src).expect("doc");
        assert_eq!(doc.blocks.len(), 14, "12 items plus the two brackets");
        // Two digits of number, then a space, right aligned so the body lines up.
        let marks: Vec<String> = doc.blocks.iter().map(|b| b.pieces[0].text.clone()).collect();
        assert_eq!(marks[0], " 1 ", "{marks:?}");
        assert_eq!(marks[11], "12 ", "{marks:?}");
        assert_eq!(marks[13], "14 ", "{marks:?}");
        assert!(doc.blocks.iter().all(|b| b.pieces[0].muted), "a gutter, not content");
        assert_eq!(doc.blocks[0].pieces[1].text, "[", "and the body follows it");
    }

    #[test]
    fn a_json_document_is_one_block_a_line_and_monospace() {
        let doc = doc(r#"{"a":1,"b":[2,3]}"#).expect("doc");
        assert_eq!(doc.blocks.len(), 7, "{doc:?}");
        assert!(doc.blocks.iter().all(|b| b.kind == BlockKind::Pre));
        assert!(doc.blocks.iter().flat_map(|b| &b.pieces).all(|p| p.mono), "the slab");
        let roles = doc.blocks.iter().flat_map(|b| &b.pieces).map(|p| p.json).collect::<Vec<_>>();
        assert!(roles.contains(&Role::Key), "{roles:?}");
        assert!(roles.contains(&Role::Number), "{roles:?}");
        assert!(roles.contains(&Role::Punct), "{roles:?}");
        let body = |b: &Block| b.pieces[1..].iter().map(|p| p.text.as_str()).collect::<String>();
        assert_eq!(body(&doc.blocks[6]), "}", "the closing brace stands on its own");
    }
}
