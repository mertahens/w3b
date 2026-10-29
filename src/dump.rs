//! Plain text rendering: used by `--dump` and by tests.
use crate::layout::Layout;

pub fn plain_text(layout: &Layout) -> String {
    let mut out = String::new();
    for row in &layout.rows {
        let mut line = String::new();
        let mut col = 0usize;
        for run in &row.runs {
            while col < run.start {
                line.push(' ');
                col += 1;
            }
            line.push_str(&run.text);
            col = run.end();
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// Render an HTML fragment at a fixed width. Handy in tests and one-liners.
pub fn render_html(html: &str, width: usize) -> String {
    let doc = crate::dom::parse(html);
    plain_text(&crate::layout::layout(&doc, width, 4, true))
}

/// Render a markdown document at a fixed width, read the way the browser reads one.
pub fn render_markdown(src: &str, width: usize) -> String {
    let doc = crate::dom::parse(&crate::markdown::to_html(src));
    plain_text(&crate::layout::layout(&doc, width, 4, true))
}

/// Render a JSON body at a fixed width, laid out the way the browser lays it out.
pub fn render_json(json: &str, width: usize) -> String {
    let doc = crate::json::doc(json).expect("the body must be valid json to lay it out");
    plain_text(&crate::layout::layout(&doc, width, 4, true))
}

/// Render an HTML fragment with an embedded stylesheet applied.
pub fn render_html_css(html: &str, css: &str, width: usize) -> String {
    let sheet = crate::css::parse(css);
    let doc = crate::dom::parse_with_css(html, &sheet);
    plain_text(&crate::layout::layout(&doc, width, 4, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_paragraphs_separated_by_blank_lines() {
        let text = render_html("<p>one</p><p>two</p>", 20);
        assert_eq!(text, "one\n\ntwo\n");
    }

    #[test]
    fn strips_trailing_whitespace() {
        let text = render_html("<p>hello   </p>", 20);
        assert_eq!(text, "hello\n");
    }
}
