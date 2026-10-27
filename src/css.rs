//! A small CSS engine: enough to hide elements and spot monospace text.
//!
//! w3b has no box model, so most of CSS is meaningless to it. Two parts of it
//! are not: whether an element should be on the page at all (`display: none`,
//! `visibility: hidden`), and which elements ask for a monospaced font (which
//! w3b paints as a grey block spanning the line). This module parses embedded
//! `<style>` sheets, imported and linked stylesheets, and inline `style="..."`
//! attributes, and answers those questions.
//!
//! Supported selectors: `*`, `tag`, `#id`, `.class` and compounds of them
//! (`div.note#x`), combined with descendant (space) and child (`>`)
//! combinators, and comma-separated selector lists. `+` and `~` are treated as
//! a descendant, pseudo-classes are dropped, at-rules other than `@import` are
//! skipped with their contents. Declarations keep their value as a string;
//! only the properties above are ever looked at. Specificity is computed the
//! usual (id, class, tag) way and a tie goes to the later rule, so the cascade
//! behaves like the real one for the subset that exists here.
use ego_tree::NodeRef;
use scraper::node::Node;
use std::collections::BTreeMap;

/// One `selector list { declarations }` pair.
#[derive(Debug, Clone)]
pub struct Rule {
    pub selectors: Vec<Selector>,
    pub declarations: BTreeMap<String, String>,
}

/// One compound selector, such as `div.note` or `#main`.
#[derive(Debug, Clone, PartialEq)]
pub struct Compound {
    pub tag: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
}

/// A selector path, with `child[i]` saying whether `compounds[i]` was joined
/// to the compound before it with `>`.
#[derive(Debug, Clone, PartialEq)]
pub struct Selector {
    pub compounds: Vec<Compound>,
    pub child: Vec<bool>,
}

/// A parsed stylesheet.
#[derive(Debug, Clone, Default)]
pub struct Sheet {
    pub rules: Vec<Rule>,
    /// `@import` targets, exactly as written in the sheet.
    pub imports: Vec<String>,
}

/// An element's identity as the matcher sees it. Owned, so a caller can hand
/// it over without keeping DOM nodes alive.
#[derive(Debug, Clone, Default)]
pub struct Identity {
    pub tag: String,
    pub id: Option<String>,
    pub classes: Vec<String>,
}

impl Identity {
    pub fn new(tag: &str, id: Option<&str>, classes: &[&str]) -> Identity {
        Identity {
            tag: tag.to_ascii_lowercase(),
            id: id.map(str::to_string),
            classes: classes.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// The element under test plus its ancestors, outermost first.
#[derive(Debug)]
pub struct Target<'a> {
    pub el: &'a Identity,
    pub ancestors: &'a [Identity],
}

impl<'a> Target<'a> {
    /// A stand-alone element: nothing above it to match against.
    pub fn alone(el: &'a Identity) -> Target<'a> {
        Target { el, ancestors: &[] }
    }
}

/// Monospace family names that mean "fixed width text" here.
const MONO_FAMILIES: &[&str] = &[
    "monospace",
    "courier",
    "consolas",
    "menlo",
    "monaco",
    "inconsolata",
    "dejavu sans mono",
    "liberation mono",
    "source code pro",
    "source code",
    "fira mono",
    "fira code",
    "jetbrains mono",
    "ubuntu mono",
    "roboto mono",
    "sf mono",
    "space mono",
    "ibm plex mono",
];

pub fn is_monospace_family(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    MONO_FAMILIES.iter().any(|family| lower.contains(family))
}

/// Parse a stylesheet. Never fails: anything unrecognised is skipped.
pub fn parse(css: &str) -> Sheet {
    let text = strip_comments(css);
    let chars: Vec<char> = text.chars().collect();
    let mut sheet = Sheet::default();
    let mut pos = 0usize;
    let mut prelude = String::new();

    while pos < chars.len() {
        let c = chars[pos];
        pos += 1;
        match c {
            '{' => {
                let head = std::mem::take(&mut prelude);
                let mut body = String::new();
                let mut depth = 1usize;
                while pos < chars.len() {
                    let inner = chars[pos];
                    pos += 1;
                    match inner {
                        '{' => {
                            depth += 1;
                            body.push(inner);
                        }
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                            body.push(inner);
                        }
                        _ => body.push(inner),
                    }
                }
                // A block left open at the end of the sheet is dropped, which is
                // what a browser does with it too.
                add_block(&mut sheet, head.trim(), &body);
            }
            // A `;` outside a block ends a statement: only `@import` matters.
            ';' => {
                let stmt = std::mem::take(&mut prelude);
                let stmt = stmt.trim();
                if let Some(arg) = stmt.strip_prefix("@import") {
                    if let Some(url) = import_url(arg.trim()) {
                        sheet.imports.push(url);
                    }
                }
            }
            '}' => prelude.clear(),
            _ => prelude.push(c),
        }
    }
    sheet
}

/// Remove `/* ... */` runs without touching anything else.
fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(idx) = rest.find("/*") {
        out.push_str(&rest[..idx]);
        let tail = &rest[idx + 2..];
        match tail.find("*/") {
            Some(end) => rest = &tail[end + 2..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Turn `prelude { body }` into rules. At-rules with a block are skipped.
fn add_block(sheet: &mut Sheet, prelude: &str, body: &str) {
    if prelude.is_empty() || prelude.starts_with('@') {
        return;
    }
    let selectors: Vec<Selector> = prelude.split(',').filter_map(parse_selector).collect();
    if selectors.is_empty() {
        return;
    }
    let declarations = parse_declarations(body);
    if declarations.is_empty() {
        return;
    }
    sheet.rules.push(Rule { selectors, declarations });
}

/// Split `text` into declarations, dropping bad ones.
pub fn parse_declarations(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for entry in text.split(';') {
        let Some((name, value)) = entry.split_once(':') else { continue };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().trim_start_matches("!important").trim().to_string();
        if name.is_empty() || value.is_empty() {
            continue;
        }
        if !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            continue;
        }
        out.insert(name, value);
    }
    out
}

/// The url of an `@import` argument: `url(x)`, `"x"` or `'x'`.
fn import_url(arg: &str) -> Option<String> {
    let arg = arg.trim().trim_end_matches(';').trim();
    let inner = if let Some(rest) = arg.strip_prefix("url(") { rest.strip_suffix(')')? } else { arg };
    let url = inner.trim().trim_matches(['"', '\'']);
    if url.is_empty() || url.starts_with("data:") {
        return None;
    }
    Some(url.to_string())
}

/// Parse one selector from a comma-separated list.
fn parse_selector(text: &str) -> Option<Selector> {
    let mut compounds: Vec<Compound> = Vec::new();
    let mut child: Vec<bool> = Vec::new();
    let mut direct = false;
    for token in text.split_whitespace() {
        if token == ">" {
            direct = true;
            continue;
        }
        if token == "+" || token == "~" {
            // Sibling combinators: approximate with an unrelated element.
            child.push(direct && !compounds.is_empty());
            compounds.push(Compound { tag: Some("*".to_string()), id: None, classes: Vec::new() });
            continue;
        }
        // `child[i]` records whether compounds[i] was joined with `>`.
        child.push(direct && !compounds.is_empty());
        direct = false;
        for part in token.split(',') {
            if let Some(compound) = parse_compound(part) {
                compounds.push(compound);
            }
        }
    }
    if compounds.is_empty() {
        return None;
    }
    Some(Selector { compounds, child })
}

/// Parse `div.note#x`, dropping any pseudo class or element.
fn parse_compound(text: &str) -> Option<Compound> {
    let text = text.split(':').next().unwrap_or("");
    if text.is_empty() {
        return None;
    }
    let head: String = text.chars().take_while(|c| *c != '#' && *c != '.').collect();
    let tag = if head.is_empty() || head == "*" { None } else { Some(head.to_ascii_lowercase()) };
    let mut id: Option<String> = None;
    let mut classes: Vec<String> = Vec::new();
    let mut rest = &text[head.len()..];
    while !rest.is_empty() {
        let sep = rest.chars().next().unwrap_or('.');
        let tail = &rest[1..];
        let name: String = tail.chars().take_while(|c| *c != '#' && *c != '.').collect();
        let consumed = 1 + name.chars().count();
        if !name.is_empty() {
            if sep == '#' {
                id = Some(name);
            } else {
                classes.push(name);
            }
        }
        if consumed > rest.chars().count() {
            break;
        }
        rest = &rest[consumed..];
    }
    if tag.is_none() && id.is_none() && classes.is_empty() {
        return Some(Compound { tag: Some("*".to_string()), id, classes });
    }
    Some(Compound { tag, id, classes })
}

impl Compound {
    fn matches(&self, el: &Identity) -> bool {
        if let Some(tag) = &self.tag {
            if tag != "*" && !tag.eq_ignore_ascii_case(&el.tag) {
                return false;
            }
        }
        if let Some(id) = &self.id {
            if el.id.as_deref() != Some(id.as_str()) {
                return false;
            }
        }
        self.classes.iter().all(|want| el.classes.iter().any(|have| have == want))
    }

    /// (ids, classes, tags), compared lexicographically as real CSS does.
    fn specificity(&self) -> (u32, u32, u32) {
        let id = usize::from(self.id.is_some()) as u32;
        let tags = usize::from(self.tag.as_deref().is_some_and(|t| t != "*")) as u32;
        (id, self.classes.len() as u32, tags)
    }
}

impl Selector {
    /// The last compound must match the element; the others walk ancestors.
    pub fn matches(&self, t: &Target<'_>) -> bool {
        let n = self.compounds.len();
        if n == 0 || !self.compounds[n - 1].matches(t.el) {
            return false;
        }
        let mut depth = t.ancestors.len();
        for ci in (0..n - 1).rev() {
            let direct = self.child.get(ci + 1).copied().unwrap_or(false);
            if direct {
                if depth == 0 || !self.compounds[ci].matches(&t.ancestors[depth - 1]) {
                    return false;
                }
                depth -= 1;
            } else {
                let mut found = false;
                while depth > 0 {
                    depth -= 1;
                    if self.compounds[ci].matches(&t.ancestors[depth]) {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return false;
                }
            }
        }
        true
    }

    fn specificity(&self) -> (u32, u32, u32) {
        self.compounds.iter().fold((0, 0, 0), |acc, c| {
            let s = c.specificity();
            (acc.0 + s.0, acc.1 + s.1, acc.2 + s.2)
        })
    }
}

impl Sheet {
    /// Parse a stylesheet.
    pub fn parse(css: &str) -> Sheet {
        parse(css)
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// True when some selector looks at the ancestor chain.
    pub fn needs_ancestors(&self) -> bool {
        self.rules.iter().any(|rule| rule.selectors.iter().any(|s| s.compounds.len() > 1))
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Winning declaration for `property`; an inline `style="..."` wins.
    pub fn value(&self, t: &Target<'_>, property: &str, inline: Option<&str>) -> Option<String> {
        if let Some(inline) = inline {
            if let Some(value) = parse_declarations(inline).get(property) {
                return Some(value.clone());
            }
        }
        let mut best: Option<((u32, u32, u32), &str)> = None;
        for rule in &self.rules {
            let Some(value) = rule.declarations.get(property) else { continue };
            let mut spec: Option<(u32, u32, u32)> = None;
            for selector in &rule.selectors {
                if selector.matches(t) {
                    let s = selector.specificity();
                    if spec.map(|current| s > current).unwrap_or(true) {
                        spec = Some(s);
                    }
                }
            }
            let Some(spec) = spec else { continue };
            // Higher specificity wins; a tie goes to the later rule.
            if best.map(|(current, _)| spec >= current).unwrap_or(true) {
                best = Some((spec, value.as_str()));
            }
        }
        best.map(|(_, value)| value.to_string())
    }

    /// `display: none`, or `visibility: hidden` / `collapse`.
    pub fn hidden(&self, t: &Target<'_>, inline: Option<&str>) -> bool {
        if self.value(t, "display", inline).as_deref() == Some("none") {
            return true;
        }
        matches!(self.value(t, "visibility", inline).as_deref(), Some("hidden") | Some("collapse"))
    }

    /// True when the element asks for a monospaced font.
    pub fn monospace(&self, t: &Target<'_>, inline: Option<&str>) -> bool {
        self.value(t, "font-family", inline).map(|v| is_monospace_family(&v)).unwrap_or(false)
    }

    /// Whether any rule mentions `property` at all.
    pub fn mentions(&self, property: &str) -> bool {
        self.rules.iter().any(|rule| rule.declarations.contains_key(property))
    }
}

/// Pull the CSS a document carries out of it before the DOM is built: the text
/// of every `<style>` (and `<style scoped>`-like) element plus the href of every
/// stylesheet `<link>`, in document order.
pub fn extract(html: &str) -> (String, Vec<String>) {
    let doc = scraper::Html::parse_fragment(html);
    let mut sheet = String::new();
    let mut links: Vec<String> = Vec::new();
    gather(doc.tree.root(), &mut sheet, &mut links);
    (sheet, links)
}

fn gather(node: NodeRef<'_, Node>, sheet: &mut String, links: &mut Vec<String>) {
    if let Some(el) = node.value().as_element() {
        match el.name() {
            "style" => {
                if !sheet.is_empty() {
                    sheet.push('\n');
                }
                sheet.push_str(&text_of(node));
            }
            "link" => {
                let rel = el.attr("rel").unwrap_or("");
                let is_sheet = rel.split_whitespace().any(|part| part.eq_ignore_ascii_case("stylesheet"));
                let media = el.attr("media").unwrap_or("all");
                let wanted = media
                    .split(',')
                    .any(|m| matches!(m.trim().to_ascii_lowercase().as_str(), "all" | "screen"));
                if is_sheet && wanted {
                    if let Some(href) = el.attr("href").map(str::trim).filter(|v| !v.is_empty()) {
                        links.push(href.to_string());
                    }
                }
            }
            // Never pull CSS out of places it cannot live.
            "script" | "textarea" | "pre" => return,
            _ => {}
        }
    }
    for child in node.children() {
        gather(child, sheet, links);
    }
}

fn text_of(node: NodeRef<'_, Node>) -> String {
    let mut out = String::new();
    for child in node.descendants() {
        if let Some(text) = child.value().as_text() {
            out.push_str(text);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(css: &str) -> Sheet {
        Sheet::parse(css)
    }

    fn id(tag: &str, id: Option<&str>, classes: &[&str]) -> Identity {
        Identity::new(tag, id, classes)
    }

    /// `body > div > leaf`
    struct Tree {
        ancestors: Vec<Identity>,
        leaf: Identity,
    }

    fn tree(leaf_tag: &str) -> Tree {
        Tree { ancestors: vec![id("body", None, &[]), id("div", None, &[])], leaf: id(leaf_tag, None, &[]) }
    }

    fn t(tree: &Tree) -> Target<'_> {
        Target { el: &tree.leaf, ancestors: &tree.ancestors }
    }

    #[test]
    fn parses_rules_and_declarations() {
        let s = sheet("h1 { color: red; display: none } .note, #x { visibility: hidden; }");
        assert_eq!(s.len(), 2);
        let tree = tree("h1");
        assert_eq!(s.value(&t(&tree), "display", None).as_deref(), Some("none"));
        assert_eq!(s.value(&t(&tree), "color", None).as_deref(), Some("red"));
        assert_eq!(s.value(&t(&tree), "visibility", None), None);
    }

    #[test]
    fn comments_are_stripped() {
        let s = sheet("/* h1 { display: none } */\nh1 { color: blue /* { } */; }");
        assert_eq!(s.len(), 1);
        let tree = tree("h1");
        assert_eq!(s.value(&t(&tree), "color", None).as_deref(), Some("blue"));
        assert_eq!(s.value(&t(&tree), "display", None), None);
    }

    #[test]
    fn tag_id_and_class_selectors() {
        let s = sheet("p { a: 1 } .hide { b: 2 } #main { c: 3 } p.hide { d: 4 }");
        let el = id("p", Some("main"), &["hide"]);
        let target = Target::alone(&el);
        assert_eq!(s.value(&target, "a", None).as_deref(), Some("1"));
        assert_eq!(s.value(&target, "b", None).as_deref(), Some("2"));
        assert_eq!(s.value(&target, "c", None).as_deref(), Some("3"));
        assert_eq!(s.value(&target, "d", None).as_deref(), Some("4"));

        let el = id("p", None, &[]);
        let target = Target::alone(&el);
        assert_eq!(s.value(&target, "b", None), None);
        assert_eq!(s.value(&target, "d", None), None);
        assert_eq!(s.value(&target, "a", None).as_deref(), Some("1"));
    }

    #[test]
    fn descendant_combinators() {
        let s = sheet("div p { a: 1 } section p { c: 3 } body section p { d: 4 }");
        let tree = tree("p");
        assert_eq!(s.value(&t(&tree), "a", None).as_deref(), Some("1"));
        assert_eq!(s.value(&t(&tree), "c", None), None);
        let tree = Tree {
            ancestors: vec![id("body", None, &[]), id("section", None, &[])],
            leaf: id("p", None, &[]),
        };
        assert_eq!(s.value(&t(&tree), "c", None).as_deref(), Some("3"));
        assert_eq!(s.value(&t(&tree), "d", None).as_deref(), Some("4"));
    }

    #[test]
    fn child_combinator_skips_grandchildren() {
        let s = sheet("ul > li { display: none }");
        let nested =
            Tree { ancestors: vec![id("ul", None, &[]), id("div", None, &[])], leaf: id("li", None, &[]) };
        assert!(!s.hidden(&t(&nested), None));
        let direct = Tree { ancestors: vec![id("ul", None, &[])], leaf: id("li", None, &[]) };
        assert!(s.hidden(&t(&direct), None));
    }

    #[test]
    fn specificity_then_order_decides() {
        let s = sheet("p { color: red } div p { color: green } .x { color: blue }");
        let tree = tree("p");
        assert_eq!(s.value(&t(&tree), "color", None).as_deref(), Some("green"));
        let mut leaf = tree.leaf.clone();
        leaf.classes.push("x".to_string());
        let target = Target { el: &leaf, ancestors: &tree.ancestors };
        assert_eq!(s.value(&target, "color", None).as_deref(), Some("blue"));

        let later = sheet("p { color: red } p { color: green }");
        assert_eq!(later.value(&t(&tree), "color", None).as_deref(), Some("green"));
    }

    #[test]
    fn inline_style_beats_the_sheet() {
        let s = sheet("p { display: none }");
        let tree = tree("p");
        assert!(s.hidden(&t(&tree), None));
        assert!(!s.hidden(&t(&tree), Some("display: inline")));
    }

    #[test]
    fn visibility_hidden_and_collapse() {
        let s = sheet(".gone { visibility: hidden } .col { visibility: collapse }");
        assert!(!s.hidden(&Target::alone(&id("td", None, &[])), None));
        assert!(s.hidden(&Target::alone(&id("td", None, &["gone"])), None));
        assert!(s.hidden(&Target::alone(&id("td", None, &["gone", "col"])), None));
    }

    #[test]
    fn monospace_detection() {
        let s =
            sheet("code { font-family: monospace } .term { font-family: 'JetBrains Mono', monospace } p { font-family: serif }");
        assert!(s.monospace(&Target::alone(&id("code", None, &[])), None));
        assert!(s.monospace(&Target::alone(&id("div", None, &["term"])), None));
        assert!(!s.monospace(&Target::alone(&id("p", None, &[])), None));
        assert!(s.monospace(&Target::alone(&id("p", None, &[])), Some("font-family: Courier New")));
    }

    #[test]
    fn collects_imports_and_skips_at_blocks() {
        let s = sheet("@import url(other.css);\n@import \"second.css\";\n@media print { h1 { color: red } }\nh1 { color: blue }");
        assert_eq!(s.imports, vec!["other.css", "second.css"]);
        let tree = tree("h1");
        assert_eq!(s.value(&t(&tree), "color", None).as_deref(), Some("blue"));
        assert!(!s.mentions("display"));
    }

    #[test]
    fn a_stray_brace_does_not_break_the_next_rule() {
        let s = sheet("h1 { color: red\n} p { display: none }");
        let par = tree("p");
        assert_eq!(s.value(&t(&par), "display", None).as_deref(), Some("none"));
        let h1 = tree("h1");
        assert_eq!(s.value(&t(&h1), "color", None).as_deref(), Some("red"));
    }

    #[test]
    fn a_block_left_open_is_dropped() {
        let s = sheet("@media print { h1 { color: red");
        let tree = tree("h1");
        assert_eq!(s.value(&t(&tree), "color", None), None);
    }

    #[test]
    fn malformed_input_is_harmless() {
        for junk in ["", "}", "{{{", "h1 {", ":", "@", "/*", "h1 { color }", ".{.}", "@import"] {
            let _ = sheet(junk);
        }
    }

    #[test]
    fn extracts_embedded_and_linked_css() {
        let html = "<html><head>\
            <style>h1 { display: none }</style>\
            <link rel=stylesheet href=/a.css>\
            <link rel='stylesheet alternate' href=/b.css>\
            <link rel=stylesheet media=print href=/c.css>\
            <link rel=icon href=/i.png>\
            </head><body><p>hi</p></body></html>";
        let (sheet, links) = extract(html);
        assert!(sheet.contains("h1 { display: none }"));
        assert_eq!(links, vec!["/a.css", "/b.css"]);
    }

    #[test]
    fn extract_survives_an_empty_document() {
        let (sheet, links) = extract("");
        assert!(sheet.is_empty());
        assert!(links.is_empty());
    }

    #[test]
    fn needs_ancestors_only_for_chained_selectors() {
        assert!(!sheet("h1 { color: red } .x { a: b }").needs_ancestors());
        assert!(sheet("div p { color: red }").needs_ancestors());
        assert!(sheet("a > b { color: red }").needs_ancestors());
    }
}
