//! The real fetch path: ureq against a local HTTP server. No internet.
mod common;

use common::{Response, TestServer};
use std::sync::Arc;
use w3b::app::{App, Viewport};
use w3b::config::built_in_defaults;
use w3b::net::{normalize_target, HttpFetcher};
use w3b::theme::Theme;
use w3b::Fetcher;

/// The same wiring, but nothing is fetched until the caller opens a URL, so a
/// test can change the config first.
fn quiet_app(url: &str, width: usize, height: usize) -> App {
    let config = built_in_defaults();
    let theme = Theme::build(&config).expect("theme");
    App::empty(
        config,
        theme,
        Arc::new(HttpFetcher::new(&Default::default())),
        url,
        Viewport { width, height },
    )
}

fn fetch_app(server: &TestServer, path: &str, width: usize, height: usize) -> App {
    let config = built_in_defaults();
    let theme = Theme::build(&config).expect("theme");
    let url = server.url(path);
    let mut app = App::empty(
        config,
        theme,
        Arc::new(HttpFetcher::new(&Default::default())),
        &url,
        Viewport { width, height },
    );
    app.open(&url);
    app
}

#[test]
fn plain_page_over_http() {
    let server = TestServer::start(vec![(
        "/index.html",
        Response::html(200, "<title>Real</title><h1>hello</h1><p>over http</p>"),
    )]);
    let app = fetch_app(&server, "/index.html", 60, 12);
    assert_eq!(app.status().text(), "200 OK");
    assert_eq!(app.title(), "Real");
    assert_eq!(app.url(), server.url("/index.html"));
    assert!(app.plain_text().contains("over http"));
    assert_eq!(server.request_paths(), vec!["/index.html".to_string()]);
}

#[test]
fn relative_links_resolve_against_the_response_url() {
    let server = TestServer::start(vec![
        ("/dir/page.html", Response::html(200, r#"<a href="next.html">next</a> <a href="/top">top</a>"#)),
        ("/dir/next.html", Response::html(200, "<title>next</title>")),
    ]);
    let mut app = fetch_app(&server, "/dir/page.html", 60, 12);
    let targets: Vec<String> = app
        .page
        .links
        .iter()
        .map(|l| match &l.follow {
            w3b::page::Follow::Url(u) => u.clone(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(targets, vec![server.url("/dir/next.html"), server.url("/top")]);

    // The cursor opens on the first link, so Enter follows it. (`Tab` steps to
    // the next link, not the one already under the cursor.)
    app.handle_key(common::key(ratatui::crossterm::event::KeyCode::Enter));
    app.handle_key(common::key(ratatui::crossterm::event::KeyCode::Enter));
    app.settle();
    assert_eq!(app.status().text(), "200 OK");
    assert!(app.url().ends_with("/dir/next.html"), "{}", app.url());
}

#[test]
fn not_found_renders_the_body_and_sets_the_status() {
    let server = TestServer::start(vec![]);
    let app = fetch_app(&server, "/missing", 60, 12);
    assert_eq!(app.status().text(), "404 NOT FOUND");
    assert!(app.plain_text().contains("no such route"));
}

#[test]
fn server_error_status() {
    let server = TestServer::start(vec![("/boom", Response::html(503, "<h1>temporarily down</h1>"))]);
    let app = fetch_app(&server, "/boom", 60, 12);
    assert_eq!(app.status().text(), "503 SERVICE UNAVAILABLE");
    assert_eq!(app.status().kind(), w3b::status::StatusKind::ServerError);
    assert!(app.plain_text().contains("temporarily down"));
}

#[test]
fn redirects_are_followed_and_the_final_url_is_kept() {
    let server = TestServer::start(vec![
        ("/old", Response::html(301, "<p>ignored</p>").header("Location", "/new")),
        ("/new", Response::html(200, "<title>fresh</title><p>arrived</p>")),
    ]);
    let app = fetch_app(&server, "/old", 60, 12);
    assert_eq!(app.status().text(), "200 OK", "the final response is what we show");
    assert!(app.url().ends_with("/new"), "{}", app.url());
    assert_eq!(app.title(), "fresh");
    assert!(app.plain_text().contains("arrived"));
}

#[test]
fn charset_from_the_content_type_header_is_honoured() {
    // "café" in windows-1252
    let body: Vec<u8> = b"<title>caf\xe9</title><p>r\xe9sum\xe9</p>".to_vec();
    let server =
        TestServer::start(vec![("/latin", Response::bytes(200, "text/html; charset=windows-1252", &body))]);
    let app = fetch_app(&server, "/latin", 60, 12);
    assert_eq!(app.title(), "café");
    assert!(app.plain_text().contains("résumé"));
}

#[test]
fn meta_charset_is_honoured_without_a_header() {
    let body = encoding_rs::SHIFT_JIS
        .encode("<meta charset=\"shift_jis\"><p>\u{65e5}\u{672c}\u{8a9e}</p>")
        .0
        .to_vec();
    let server = TestServer::start(vec![("/jp", Response::bytes(200, "text/html", &body))]);
    let app = fetch_app(&server, "/jp", 60, 12);
    assert!(app.plain_text().contains("\u{65e5}\u{672c}\u{8a9e}"), "{}", app.plain_text());
}

#[test]
fn plain_text_is_rendered_as_preformatted_text() {
    let server = TestServer::start(vec![(
        "/log",
        Response::bytes(200, "text/plain; charset=utf-8", b"line one\n   indented\n\nlast"),
    )]);
    let app = fetch_app(&server, "/log", 60, 12);
    let text = app.plain_text();
    assert!(text.contains("line one"), "{text}");
    assert!(text.contains("   indented"), "indentation survives: {text}");
}

#[test]
fn binary_bodies_are_not_rendered_as_text() {
    let server =
        TestServer::start(vec![("/pdf", Response::bytes(200, "application/pdf", b"%PDF-1.7\n\x00\x01"))]);
    let app = fetch_app(&server, "/pdf", 60, 12);
    assert_eq!(app.status().text(), "200 OK");
    let text = app.plain_text();
    assert!(text.to_lowercase().contains("not"), "a notice is shown: {text}");
    assert!(!text.contains("%PDF-1.7"), "raw bytes are not dumped: {text}");
}

#[test]
fn oversized_bodies_report_too_large() {
    let mut config = built_in_defaults();
    config.behavior.max_response_bytes = 64;
    let theme = Theme::build(&config).unwrap();
    let server = TestServer::start(vec![("/big", Response::html(200, &"<p>x</p>".repeat(200)))]);
    let url = server.url("/big");
    let fetcher = HttpFetcher::new(&config.behavior);
    let mut app = App::empty(config, theme, Arc::new(fetcher), &url, Viewport { width: 60, height: 12 });
    app.open(&url);
    assert_eq!(app.status().text(), "ERROR TOO LARGE", "{}", app.status().text());
    assert!(app.plain_text().contains(&url));
}

#[test]
fn unreachable_host_reports_dns() {
    let config = built_in_defaults();
    let fetcher = HttpFetcher::new(&config.behavior);
    let err = fetcher.fetch("http://this-host-does-not-exist.invalid/").unwrap_err();
    let kind = w3b::error::NetError::classify(match &err {
        w3b::Error::Http(e) => e,
        other => panic!("expected an http error, got {other:?}"),
    });
    assert_eq!(kind.label(), "DNS");
}

#[test]
fn unsupported_schemes_are_refused_before_fetching() {
    assert!(w3b::net::is_supported_scheme("https://x.test"));
    assert!(w3b::net::is_supported_scheme("http://x.test"));
    assert!(!w3b::net::is_supported_scheme("ftp://x.test"));
    assert!(!w3b::net::is_supported_scheme("file:///etc/passwd"));
}

#[test]
fn typed_targets_become_urls_or_searches() {
    assert_eq!(normalize_target("example.com").unwrap(), "https://example.com/");
    assert_eq!(normalize_target("http://example.com").unwrap(), "http://example.com/");
    assert_eq!(normalize_target("example.com/a?b=c").unwrap(), "https://example.com/a?b=c");
    assert_eq!(normalize_target("rust browser").unwrap(), "https://duckduckgo.com/?q=rust+browser");
    assert_eq!(normalize_target("ä").unwrap(), "https://duckduckgo.com/?q=%C3%A4");
    assert!(normalize_target("").is_err());
    // Without a scheme in front, an address is still an address.
    assert_eq!(normalize_target("example.com:8080/x").unwrap(), "https://example.com:8080/x");
    assert_eq!(normalize_target("www.example.com").unwrap(), "https://www.example.com/");
    assert_eq!(normalize_target("localhost:8080").unwrap(), "http://localhost:8080/");
    assert_eq!(normalize_target("localhost/x").unwrap(), "http://localhost/x");
    assert_eq!(normalize_target("127.0.0.1:3000").unwrap(), "http://127.0.0.1:3000/");
    // A half-typed or upper case scheme is still understood.
    assert_eq!(normalize_target("https:/example.com").unwrap(), "https://example.com/");
    assert_eq!(normalize_target("HTTPS://example.com/x").unwrap(), "https://example.com/x");
    // A scheme w3b cannot fetch is not an address: searching beats mangling it
    // into a url.
    assert_eq!(
        normalize_target("mailto:me@example.com").unwrap(),
        "https://duckduckgo.com/?q=mailto%3Ame%40example.com"
    );
}

#[test]
fn an_embedded_style_hides_an_element() {
    let html =
        "<title>Styled</title><p>kept</p><style>.ad { display: none }</style><p class=ad>sponsored</p>";
    let server = TestServer::start(vec![("/page", Response::html(200, html))]);
    let app = fetch_app(&server, "/page", 60, 12);
    let text = app.plain_text();
    assert!(text.contains("kept"), "{text}");
    assert!(!text.contains("sponsored"), "display: none must be gone: {text}");
    assert_eq!(server.request_paths(), vec!["/page".to_string()], "an embed needs no request");
}

#[test]
fn a_linked_stylesheet_is_fetched_and_applied() {
    let html = "<title>Led</title><link rel=stylesheet href=/site.css><h1>heading</h1><p>body</p>";
    let server = TestServer::start(vec![
        ("/page", Response::html(200, html)),
        ("/site.css", Response::bytes(200, "text/css", b"h1 { display: none } p { visibility: hidden }")),
    ]);
    let app = fetch_app(&server, "/page", 60, 12);
    let text = app.plain_text();
    assert!(!text.contains("heading"), "the sheet hid the heading: {text}");
    assert!(!text.contains("body"), "visibility: hidden hides too: {text}");
    assert!(server.request_paths().contains(&"/site.css".to_string()), "{:?}", server.request_paths());
}

#[test]
fn imported_stylesheets_are_followed() {
    let html = "<title>Chain</title><link rel=stylesheet href=/a.css><p>deep</p>";
    let server = TestServer::start(vec![
        ("/page", Response::html(200, html)),
        ("/a.css", Response::bytes(200, "text/css", b"@import \"b.css\"; h1 { color: red }")),
        ("/b.css", Response::bytes(200, "text/css", b"p { display: none }")),
    ]);
    let app = fetch_app(&server, "/page", 60, 12);
    let text = app.plain_text();
    assert!(!text.contains("deep"), "the imported sheet applies: {text}");
    let paths = server.request_paths();
    assert!(paths.contains(&"/a.css".to_string()) && paths.contains(&"/b.css".to_string()), "{paths:?}");
}

#[test]
fn a_stylesheet_that_is_really_html_is_ignored() {
    // Servers answer a missing .css with an HTML error page; it must not be
    // read as CSS, and must not break the page.
    let html = "<title>Guards</title><link rel=stylesheet href=/gone.css><p>still here</p>";
    let server = TestServer::start(vec![("/page", Response::html(200, html))]);
    let app = fetch_app(&server, "/page", 60, 12);
    assert!(app.plain_text().contains("still here"));
    assert!(server.request_paths().contains(&"/gone.css".to_string()));
}

#[test]
fn stylesheet_fetches_are_capped() {
    let html = "<title>Cap</title>\
        <link rel=stylesheet href=/one.css>\
        <link rel=stylesheet href=/two.css>\
        <link rel=stylesheet href=/three.css>\
        <p class=one>one</p><p class=two>two</p><p class=three>three</p>";
    let server = TestServer::start(vec![
        ("/page", Response::html(200, html)),
        ("/one.css", Response::bytes(200, "text/css", b".one { display: none }")),
        ("/two.css", Response::bytes(200, "text/css", b".two { display: none }")),
        ("/three.css", Response::bytes(200, "text/css", b".three { display: none }")),
    ]);
    let url = server.url("/page");
    let css_calls = || server.request_paths().iter().filter(|p| p.ends_with(".css")).count();

    // The default budget covers all three.
    let app = fetch_app(&server, "/page", 60, 12);
    let text = app.plain_text();
    assert!(!text.contains("one") && !text.contains("two") && !text.contains("three"), "{text}");
    assert_eq!(css_calls(), 3, "{:?}", server.request_paths());

    // A cap of two applies the first two sheets in document order only.
    let mut app = quiet_app(&url, 60, 12);
    app.config.behavior.max_stylesheets = 2;
    app.open(&url);
    assert_eq!(css_calls(), 5, "{:?}", server.request_paths());
    let text = app.plain_text();
    assert!(!text.contains("one") && !text.contains("two"), "the first two apply: {text}");
    assert!(text.contains("three"), "the third is out of budget: {text}");

    // Zero means: never ask for a sheet.
    let before = css_calls();
    let mut app = quiet_app(&url, 60, 12);
    app.config.behavior.max_stylesheets = 0;
    app.open(&url);
    assert_eq!(css_calls(), before, "no fetches with the cap at zero");
    assert!(app.plain_text().contains("one"));
}

#[test]
fn a_print_only_stylesheet_is_skipped() {
    let html = "<title>Print</title><link rel=stylesheet media=print href=/print.css><p>screen</p>";
    let server = TestServer::start(vec![
        ("/page", Response::html(200, html)),
        ("/print.css", Response::bytes(200, "text/css", b"p { display: none }")),
    ]);
    let app = fetch_app(&server, "/page", 60, 12);
    assert!(app.plain_text().contains("screen"), "print styles do not apply to a terminal");
    assert!(!server.request_paths().contains(&"/print.css".to_string()));
}

#[test]
fn css_can_make_a_line_monospace() {
    let html =
        "<title>Mono</title><style>.term { font-family: monospace }</style><p class=term>make build</p>";
    let server = TestServer::start(vec![("/page", Response::html(200, html))]);
    let app = fetch_app(&server, "/page", 60, 12);
    let mono = app
        .page
        .doc
        .blocks
        .iter()
        .flat_map(|b| b.pieces.iter())
        .filter(|p| p.mono)
        .map(|p| p.text.clone())
        .collect::<Vec<_>>();
    assert!(!mono.is_empty(), "the sheet asked for a monospace font");
    assert!(mono.iter().any(|t| t.contains("make")), "{mono:?}");
}

#[test]
fn a_hidden_link_cannot_be_tabbed_to() {
    let html = "<title>Hidden</title><style>nav { display: none }</style><nav><a href=/gone>gone</a></nav><p>text</p>";
    let server = TestServer::start(vec![("/page", Response::html(200, html))]);
    let app = fetch_app(&server, "/page", 60, 12);
    assert!(app.page.links.is_empty(), "a hidden link is not a link");
    assert!(!app.plain_text().contains("gone"), "{:?}", app.plain_text());
}
