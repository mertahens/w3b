//! Layout regressions: HTML in, plain text out. Snapshots make any change in
//! the flow/wrapping rules visible immediately.
use w3b::dump::{render_html, render_html_css};

fn show(html: &str, width: usize) -> String {
    render_html(html, width)
}

#[test]
fn nested_lists_and_inline_markup() {
    let html = r#"<ul><li>one<ul><li>one.a</li><li>one.b</li></ul></li><li>two</li></ul>"#;
    insta::assert_snapshot!("nested_lists", show(html, 40));
}

#[test]
fn ordered_lists_are_numbered() {
    insta::assert_snapshot!("ordered", show("<ol><li>alpha</li><li>beta</li></ol>", 30));
}

#[test]
fn quotes_tables_and_rules() {
    let html = "<blockquote><p>quoted text that is long enough to wrap around</p></blockquote><hr><table><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>";
    insta::assert_snapshot!("quote_table_hr", show(html, 36));
}

#[test]
fn table_grid_with_headers_links_and_wrapping() {
    let html = "<table><tr><th>Tool</th><th align=right>Stars</th><th>Note</th></tr>\
        <tr><td><a href=/w3b>w3b</a></td><td>12</td><td>a long note that has to wrap inside its cell</td></tr>\
        <tr><td>curl</td><td>100</td><td>x</td></tr></table>";
    insta::assert_snapshot!("table_grid", show(html, 46));
}

#[test]
fn nested_tables_and_caption() {
    let html = "<table><caption>Sizes</caption><tr><td>small</td><td><table><tr><td>inner</td></tr></table></td></tr></table>";
    insta::assert_snapshot!("table_caption", show(html, 40));
}

#[test]
fn preformatted_blocks_keep_everything() {
    let html = "<pre>fn main() {\n\tprintln!(\"hi\");\n}</pre>";
    insta::assert_snapshot!("pre", show(html, 40));
}

#[test]
fn long_words_are_split_across_rows() {
    let html = "<p>supercalifragilisticexpialidocious and short</p>";
    insta::assert_snapshot!("long_word", show(html, 20));
}

#[test]
fn links_stay_on_their_text_when_it_wraps() {
    let doc = w3b::dom::parse("<p>click <a href=\"/x\">the very long link label that wraps</a> now</p>");
    let laid = w3b::layout::layout(&doc, 24, 4, true);
    let placements = laid.links.iter().filter(|p| p.link == 0).count();
    assert!(placements >= 2, "the link is spread over {} placements", laid.links.len());
    for p in laid.links.iter().filter(|p| p.link == 0) {
        let row = &laid.rows[p.row];
        assert!(p.end <= row.width, "placement {}..{} exceeds row width {}", p.start, p.end, row.width);
        assert_eq!(laid.link_at(p.row, p.start), Some(0));
    }
    insta::assert_snapshot!("wrapped_link", w3b::dump::plain_text(&laid));
}

#[test]
fn entities_and_unicode_survive() {
    let out = show("<p>a &amp; b &lt;tag&gt; caf&eacute; 日本語 — ok</p>", 200);
    assert!(out.contains("a & b <tag> café"), "{out}");
    assert!(out.contains("日本語"), "{out}");
    assert!(out.contains('—'), "{out}");
}

#[test]
fn control_characters_from_the_page_never_reach_the_output() {
    let out = show("<p>evil \u{1b}[31mred\u{1b}[0m text</p>", 60);
    assert!(!out.contains('\u{1b}'), "{out:?}");
    assert!(out.contains("red"), "{out}");
}

#[test]
fn empty_and_degenerate_documents_still_produce_a_row() {
    for html in ["", "<html></html>", "<p></p>", "   ", "<div></div><span></span>"] {
        let laid = w3b::layout::layout(&w3b::dom::parse(html), 40, 4, true);
        assert!(!laid.rows.is_empty(), "no rows for {html:?}");
    }
}

#[test]
fn images_and_forms_show_placeholders() {
    let html = "<img src=\"x.png\" alt=\"logo\"><form><input type=\"text\" value=\"x\"><input type=\"checkbox\"><button>Send</button></form>";
    insta::assert_snapshot!("placeholders", show(html, 60));
}

#[test]
fn the_fixture_pages_are_stable() {
    let article = std::fs::read_to_string("fixtures/article.html").expect("fixture");
    insta::assert_snapshot!("article_60", show(&article, 60));
    let second = std::fs::read_to_string("fixtures/second.html").expect("fixture");
    insta::assert_snapshot!("second_60", show(&second, 60));
}

#[test]
fn narrow_terminals_never_panic_or_produce_zero_width_rows() {
    let html = std::fs::read_to_string("fixtures/article.html").expect("fixture");
    for width in [1, 2, 3, 5, 8] {
        let laid = w3b::layout::layout(&w3b::dom::parse(&html), width, 4, true);
        assert!(!laid.rows.is_empty(), "width {width} produced no rows");
        for row in laid.rows.iter().filter(|r| !r.is_blank()) {
            assert!(row.runs.iter().all(|r| !r.text.is_empty()), "width {width}: empty run");
        }
    }
}

#[test]
fn css_hides_whole_elements() {
    let html = "<h1>title</h1><nav><a href=/x>menu</a></nav><aside class=ad>buy now</aside><p>content</p>";
    insta::assert_snapshot!("css_hidden", render_html_css(html, "nav, .ad { display: none }", 30));
}

#[test]
fn css_hides_a_table_column() {
    let html =
        "<table><tr><th>tool</th><th>stars</th></tr><tr><td>w3b</td><td class=hide>12</td></tr></table>";
    insta::assert_snapshot!("css_table_column", render_html_css(html, ".hide { display: none }", 40));
}

#[test]
fn an_inline_style_attribute_hides_without_a_sheet() {
    assert_eq!(render_html("<p style=\"display:none\">gone</p><p>here</p>", 20), "here\n");
    assert_eq!(render_html("<p style=\"visibility:hidden\">gone</p><p>here</p>", 20), "here\n");
}

#[test]
fn a_selector_that_matches_nothing_changes_nothing() {
    let html = "<p>one</p><p>two</p>";
    assert_eq!(render_html_css(html, ".missing { display: none }", 20), render_html(html, 20));
}
