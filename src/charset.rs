//! bytes -> String. Web pages declare charset in headers, in a `<meta>` tag,
//! or not at all; `encoding_rs` + `chardetng` is what Firefox uses.
use regex::Regex;
/// Charset declared by the HTTP `Content-Type` header, if any.
pub fn charset_from_content_type(content_type: Option<&str>) -> Option<String> {
    let value = content_type?;
    let idx = value.rfind("charset")?;
    let rest = &value[idx + "charset".len()..];
    let rest = rest.trim_start().strip_prefix('=')?.trim();
    let rest = rest.trim_start_matches(['"', '\'']);
    let end = rest.find([';', '"', '\'', ' ', '\t']).unwrap_or(rest.len());
    let charset = rest[..end].trim();
    if charset.is_empty() {
        None
    } else {
        Some(charset.to_string())
    }
}

/// Charset declared by a `<meta charset=...>` in the first bytes of the doc.
pub fn charset_from_meta(prefix: &str) -> Option<String> {
    let re = Regex::new(r#"(?i)<meta[^>]+charset\s*=\s*["']?\s*([a-z0-9][a-z0-9_\-:.]*)"#).ok()?;
    let cap = re.captures(prefix)?.get(1)?.as_str().to_string();
    Some(cap)
}

fn label_to_encoding(label: &str) -> Option<&'static encoding_rs::Encoding> {
    encoding_rs::Encoding::for_label(label.as_bytes())
}

/// Decode page bytes. `content_type` is the raw response header value.
pub fn decode(bytes: &[u8], content_type: Option<&str>) -> String {
    if let Some(encoding) = charset_from_content_type(content_type).as_deref().and_then(label_to_encoding) {
        return strip_bom(&encoding.decode_without_bom_handling(bytes).0);
    }

    // Sniff the head for a meta tag before guessing, using an ASCII-ish view.
    let head: String =
        bytes.iter().take(2048).map(|&b| if b.is_ascii() { b as char } else { '\u{fffd}' }).collect();
    if let Some(label) = charset_from_meta(&head) {
        if let Some(encoding) = label_to_encoding(&label) {
            return strip_bom(&encoding.decode_without_bom_handling(bytes).0);
        }
    }

    // Valid UTF-8 (possibly with a BOM) is by far the common case.
    if let Ok(text) = std::str::from_utf8(bytes) {
        return strip_bom(text);
    }

    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    let upto = bytes.len().min(64 * 1024);
    detector.feed(&bytes[..upto], upto == bytes.len());
    let encoding = detector.guess(None, chardetng::Utf8Detection::Allow);
    let (decoded, _, _) = encoding.decode(bytes);
    strip_bom(&decoded)
}

fn strip_bom(text: &str) -> String {
    text.strip_prefix('\u{feff}').map_or_else(|| text.to_string(), str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_charset_from_content_type() {
        assert_eq!(charset_from_content_type(Some("text/html; charset=UTF-8")).as_deref(), Some("UTF-8"));
        assert_eq!(
            charset_from_content_type(Some(r#"text/html;charset="iso-8859-1""#)).as_deref(),
            Some("iso-8859-1")
        );
        assert_eq!(charset_from_content_type(Some("text/html")), None);
        assert_eq!(charset_from_content_type(None), None);
    }

    #[test]
    fn reads_charset_from_meta() {
        let html = r#"<html><head><meta http-equiv="Content-Type" content="text/html; charset=Shift_JIS">"#;
        assert_eq!(charset_from_meta(html).as_deref(), Some("Shift_JIS"));
        assert_eq!(charset_from_meta("<meta charset='windows-1252'>").as_deref(), Some("windows-1252"));
        assert_eq!(charset_from_meta("<p>no meta"), None);
    }

    #[test]
    fn decodes_declared_legacy_charset() {
        let bytes = [0x3c, 0x68, 0x31, 0x3e, 0xe9, 0x3c, 0x2f, 0x68, 0x31, 0x3e]; // <h1>é</h1> latin-1
        let out = decode(&bytes, Some("text/html; charset=windows-1252"));
        assert!(out.contains('é'), "{out}");
    }

    #[test]
    fn decodes_meta_declared_charset() {
        // "日本語" in Shift_JIS, announced only by a meta tag.
        let body = encoding_rs::SHIFT_JIS.encode("\u{65e5}\u{672c}\u{8a9e}").0;
        assert_eq!(body.as_ref(), &[0x93, 0xfa, 0x96, 0x7b, 0x8c, 0xea]);
        let mut html = b"<html><head><meta charset=shift_jis></head><p>".to_vec();
        html.extend_from_slice(&body);
        html.extend_from_slice(b"</p></html>");
        let out = decode(&html, Some("text/html"));
        assert!(out.contains("日本語"), "{out}");
    }

    #[test]
    fn utf8_input_round_trips() {
        let s = "<h1>naïve — ünïcödé 日本語</h1>";
        assert_eq!(decode(s.as_bytes(), Some("text/html")), s);
    }

    #[test]
    fn invalid_bytes_are_replaced_not_fatal() {
        // 0x80 is not valid UTF-8; whatever single-byte encoding is guessed the
        // ASCII around it must survive and nothing may panic.
        let out = decode(&[b'c', b'a', b'f', 0x80, b'!'], Some("text/html"));
        assert!(out.starts_with("caf"), "{out:?}");
        assert_eq!(decode(&[0xff, 0xfe, b'a'], Some("text/html")).chars().count(), 1);
    }
}
