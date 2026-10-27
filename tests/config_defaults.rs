use w3b::config::{built_in_defaults, Config};

#[test]
fn empty_document_uses_defaults() {
    let c: Config = toml::from_str("").unwrap();
    assert_eq!(c.behavior.timeout_secs, 15);
    assert!(!c.style.link.underline);
}

#[test]
fn built_in_defaults_match_the_shipped_file() {
    let c = built_in_defaults();
    assert_eq!(c.behavior.home_url, "https://example.com");
    assert_eq!(c.behavior.timeout_secs, 15);
    assert_eq!(c.behavior.max_response_bytes, 8_388_608);
    assert_eq!(c.style.link.fg.as_deref(), Some("blue"));
    assert!(c.style.link.underline);
    assert_eq!(c.style.status.codes.get("404").map(|s| s.bg.as_deref()), Some(Some("magenta")));
}
