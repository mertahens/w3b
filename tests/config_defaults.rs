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
    // What floats over the page is bright ink with no background of its own, and a
    // frame a step brighter than the rules drawn inside a page.
    assert_eq!(c.style.pane.fg.as_deref(), Some("gray"));
    assert_eq!(c.style.pane.bg, None, "a pane has no background");
    assert_eq!(c.style.pane_border.fg.as_deref(), Some("gray"));
    assert_ne!(
        c.style.pane_border.fg.as_deref(),
        c.style.border.fg.as_deref(),
        "the frame of a pane is brighter than a table rule"
    );
    // The bar writes its own words in the same bright grey. A badge is given its own
    // ink, which is the only ink that works on its background, so the bar never
    // repaints it - which is why brightening the bar is safe.
    assert_eq!(c.style.status.fg.as_deref(), Some("gray"));
    assert_eq!(c.style.status.bg.as_deref(), Some("dark_gray"));
    assert_eq!(c.style.status.ok.fg.as_deref(), Some("black"), "a badge keeps the ink it was given");
}
