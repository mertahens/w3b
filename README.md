# w3b

A text-mode web browser for the terminal. Vim-like cursor navigation, ratatui
rendering, and a status line that tells you what actually happened.

```
Design notes

Some bold and italic text with a second page link, plus a
mail link and a anchor.

• first item
• second item that is quite long and should wrap around the
  terminal width nicely

  fn main() {
      println!("hello");
  }
────────────────────────────────────────────────────────────
 200 OK  https://example.com — Design notes - w3b  41 ms  0%
```

* The **cursor** is a real cursor: it moves with `j k h l`, and the page scrolls
  to keep it visible. Standing on a link and pressing `Enter` follows it.
* The **colours and layout of the page are ignored**. Style comes from your
  config file only — the web's colour schemes are noise in a terminal. Two parts
  of CSS do matter and are honoured: whether an element is on the page at all,
  and which elements ask for a monospace font (see [CSS](#css)).
* **Tables** are drawn as box-drawing grids (`┌─┬┐ │ ├┼┤ └┴┘`) with the header
  rule, wrapped cells and `align` respected; when the terminal is too narrow for
  a grid they fall back to one line per row.
* **Line breaks** follow the elements: a block (`div`, `p`, `h1`, …) ends a line
  and adds an empty one, an inline element (`span`, `a`, `b`, `code`, …) never
  starts or ends a line by itself, and `<br>` breaks the line inside its block
  with no empty line. Two `<br>` in a row leave one blank line between them.
* **Images** are never fetched or drawn. They leave an `[img: …]` label holding
  whatever the image says about itself: `alt`, else `title`, else `aria-label`,
  else the file name from `src`. An image that says nothing is left off the page —
  a decorative `alt=""`, or a base64 `data:` source with no name. Labels are
  painted with `[style.image]` (black on purple), and stay black-and-underlined
  inside a link thanks to `[style.linked_image]`. Links that point at a `data:` or
  `javascript:` payload are plain text, never link targets.
* A **link that is a heading** keeps the heading's own colours and only gains the
  link underline, so `<h2><a href=...>` still reads as a heading.
* The **status line** at the bottom shows `[status]  url — title  time  percent`
  - the load time can be switched off with `show_load_time = false`. Status comes
  from the HTTP status code (`200 OK`, `404 NOT FOUND`) or a braille spinner and
  `LOADING` while a request is in flight, and network failures show the reason
  (`ERROR DNS`, `ERROR TLS`, `ERROR TIMEOUT`, …).
* `--dump URL` prints the same layout as plain text, for scripts and tests.

## Build

```sh
cargo build --release       # ./target/release/w3b
cargo test                  # the whole test suite, no network needed
cargo run -- https://example.com
```

## Usage

```
w3b [TARGET] [OPTIONS]

  TARGET                 url, or a search term, or nothing (behavior.home_url)
  -c, --config <PATH>    config file (also $W3B_CONFIG)
  -d, --dump             print the page as plain text and exit
      --width <COLS>     width for --dump (default 80)
      --init-config      write the built-in default config to --config-out
      --config-out <PATH> where --init-config writes (default: the config path)
      --print-config     print the built-in default config
  -V, --version
```

Exit codes: `0` for 2xx/3xx, `1` for 4xx/5xx (the error page body is still
printed), `2` for a failed request or a bad config.

## Keys

| key | action |
| --- | --- |
| `j` `k`, `↑` `↓` | down / up one line |
| `g g`, `G` | top / bottom |
| `h` `l`, `←` `→` | left / right (scrolls sideways when `wrap = false`) |
| `0` `^` `$` | line start, first non-blank, line end |
| `Ctrl-d` `Ctrl-u` | half page down / up |
| `Ctrl-f` `Ctrl-b`, `PgDn` `PgUp` | page down / up |
| `Enter` | follow the link under the cursor, or type into the form field under it |
| `g` then `g` `e` `o` `i` `u` `h` `r` | go-to menu: top, end, back (out), forward (in), enter a url, home, reload (`Esc` cancels) |
| `Tab` `Shift-Tab` | next / previous link or form field |
| `o` or `:` | open a url (or a search term) |
| `/` | search, then `n` `N` for next / previous |
| `r` | reload |
| `Backspace` or `[` | back in history |
| `]` | forward in history |
| `y` | yank: the link under the cursor, or the page url when nothing is under it. The text goes to the terminal clipboard (OSC 52, `yank_to_clipboard`) and a `yanked: ...` note is shown |
| `w` | toggle line wrapping |
| `i` or `?` | key help |
| while typing in a field | `Backspace` erases, `Ctrl-u` clears, `Ctrl-w` deletes a word, `Enter` or `Esc` is done |
| `q`, `Ctrl-c` | quit |

`<input>` and `<textarea>` are editable: move onto one (or press `Tab` to the
next link and then `Enter`) and type. A `password` field shows dots but keeps
what you typed, and a value is remembered for as long as you stay on the page -
navigating away starts over. Nothing is submitted (see
[Not implemented](#not-implemented-on-purpose-for-now)).

Mouse wheel and left-click work too; disable with `mouse = false`. The link the
cursor stands on is highlighted with `[style.hovered_link]` — the link colour
moves from the ink to the background, with black text — and its target is shown
in the status line as `-> url`. The cursor is not tied to the length of its line:
`h`/`l` and `j`/`k` carry it anywhere across the width of the page, so it keeps
its column over short lines.

## Configuration

`$XDG_CONFIG_HOME/w3b/config.toml` (falls back to `w3b.toml`), or `--config`,
or `$W3B_CONFIG`. Start one with:

```sh
w3b --init-config                 # write the defaults to the config path
w3b --init-config --config-out ./my.toml && w3b -c ./my.toml https://example.com
w3b --print-config                # or just read the defaults
```

Every colour, modifier and behaviour switch lives in the file. Unknown keys are
an error, so typos are caught instead of ignored.

```toml
[style]
text = {}                     # body text
link = { fg = "blue", underline = true }
visited = { fg = "magenta", underline = true }
heading = { fg = "light_green", bold = true }
strong = { bold = true }
emphasis = { italic = true }
code = { fg = "cyan" }
muted = { fg = "dark_gray" }
divider = { fg = "dark_gray" }
placeholder = { fg = "dark_gray", italic = true }
image = { fg = "black", bg = "magenta", italic = true }
linked_image = { fg = "black", underline = true }
monospace = { fg = "#d0d0d0", bg = "#141414" }
hovered_link = { fg = "black", bg = "blue" }
cursor = { bg = "yellow", fg = "black", bold = true }
cursor_line = { bg = "#2b2b3b" }
search_match = { bg = "yellow", fg = "black" }
prompt, message, url, title, position, scrollbar, border

[style.status]                # the status bar itself
fg = "black"
bg = "dark_gray"
[style.status.loading]        # while fetching
fg = "black"
bg = "yellow"
[style.status.ok]             # 2xx
fg = "black"
bg = "green"
[style.status.redirect]       # 3xx
fg = "black"
bg = "cyan"
[style.status.client_error]   # 4xx
fg = "white"
bg = "red"
[style.status.server_error]   # 5xx
fg = "white"
bg = "light_red"
[style.status.error]          # network failure
fg = "white"
bg = "red"
bold = true
[style.status.codes."404"]    # an exact code beats its class
fg = "white"
bg = "magenta"

[behavior]
home_url = "https://example.com"
user_agent = "w3b/0.1 (text mode browser)"
timeout_secs = 15
max_redirects = 10
max_response_bytes = 8388608
scroll_margin = 3
tab_width = 4
wrap = true
show_title_in_status = true
show_scrollbar = false
mouse = true
case_insensitive_search = true
history_size = 200
page_padding_x = 1
yank_to_clipboard = true
max_stylesheets = 4
show_load_time = true
```

Colours accept any ratatui colour name (`red`, `dark_gray`, `light_blue`), a 256
colour index (`208`), a hex value (`#2b2b3b`), or `default` / `reset` / `none`
for the terminal's own colour. `bold`, `italic`, `underline`, `reverse` and
`dim` are booleans.

## CSS

w3b has no box model, so most CSS cannot be applied. Two decisions it does take
come from the stylesheet, and they are taken while parsing:

* **Is this element on the page?** `display: none` and `visibility: hidden`
  remove the element and everything inside it, as does the `hidden` attribute —
  a child cannot rescue itself with `display: block`. A hidden link is not a
  link: it cannot be tabbed to or followed. Declarations cascade the way CSS
  does (specificity, later wins, inline above the sheet, `!important` above all
  of it), and property names and keyword values ignore case, so
  `style="DISPLAY: NONE"` hides just as well.
* **Which `@media` blocks count?** w3b is `screen`, so `@media screen`,
  `@media all`, `@media not print` and a bare `@media {` apply, `@media print`
  and `@media speech` do not, and a query with feature conditions
  (`@media (max-width: 600px)`) is skipped because w3b has no viewport to
  measure. Rules from an applicable block join the sheet in place, so they
  cascade against plain rules like any other rule.`
* **Is this text monospace?** `font-family: monospace` (or a family named after
  one: Courier, Menlo, JetBrains Mono, ...) marks the element; so do the tags the
  browser already draws monospaced (`code`, `pre`, `kbd`, `samp`, `tt`,
  `textarea`). `[style.monospace]` paints a dark slab under that text and carries
  it to the end of the line, so a `<pre>` block reads as a block; the slab also
  outranks the colour set for `code`. Lines without monospace text are left
  alone, so no empty line below a block is shaded.

Everything else in CSS — colours, sizes, positioning, `float`, `grid` — is
ignored on purpose.

Sheets are read from `<style>` embeds, inline `style="..."` attributes, and
`<link rel=stylesheet>` (only when `media` is `all` or `screen`), and `@import`
inside those is followed too, in document order, resolved against the sheet that
named it. `behavior.max_stylesheets` (default 4) caps how many linked sheets are
fetched; `0` means never fetch any. A stylesheet request that answers with HTML
(an error page pretending to be CSS) is discarded. Selectors are matched over
`*`, `tag`, `#id`, `.class`, compounds (`div.note#x`), descendant and `>`
combinators, with real (id, class, tag) specificity and later-rules-win ties, so
`.sidebar .widget { display: none }` behaves the way you expect.

## How it works

```
fetch (ureq, blocking, on a worker thread)
  -> charset decode (Content-Type charset, <meta>, then chardetng)
  -> sanitize (strip C0/C1/ESC so a page cannot write to your terminal)
  -> CSS harvest (<style>, <link rel=stylesheet>, @import) -> parse (src/css)
  -> html5ever via scraper -> flow blocks (paragraphs, headings, items, pre,
     tables), with hidden elements dropped, monospace ones marked and form
     fields carrying their editable values
  -> layout: wrapped rows, box-drawing tables + link placements (unicode widths)
  -> viewport window -> ratatui buffer
```

The intermediate representation is a vector of laid-out rows, never HTML fed to
a widget. Link hints, search highlighting, the cursor, `--dump`, horizontal
scrolling and re-flowing on resize all read that buffer. Resizing and `w`
relayout the already-parsed document; they never re-fetch.

Modules: `net` (fetcher, content types, typed-target rules) · `charset` ·
`sanitize` · `css` (selector matching, the cascade, `@import`) · `dom` (HTML →
blocks) · `layout` (blocks → rows, tables → grids) · `page` (link resolution,
stylesheet gathering, error pages) · `app` (cursor, history, search) · `keys` · `render`
(status line, styles) · `status` · `theme`/`config` · `cli`/`dump` · `tui`.

## Tests

```sh
cargo test                    # unit + integration, hermetic
cargo test --lib              # logic only
cargo test --test http_server # against a local std::net server
INSTA_UPDATE=always cargo test  # re-accept layout/render snapshots
```

* Unit tests cover the config parser, colour parsing, status text, percent,
  sanitizing, charset detection, HTML → blocks, wrapping and link placement.
* `tests/browser.rs` drives the real key handling against a `FakeFetcher`.
* `tests/http_server.rs` exercises the real ureq path against a scripted local
  server: 200/404/503, redirects, `Content-Type` charsets, `text/plain`,
  binaries, oversized bodies, DNS failure, and linked/imported stylesheets being
  fetched, capped, and applied.
* `tests/render_tui.rs` asserts on the actual ratatui buffer (styles included)
  and keeps snapshots of the whole screen, prompt and help overlay.
* `tests/dump_snapshots.rs` keeps snapshots of the plain-text layout, so any
  change to wrapping is visible as a diff.
* `tests/cli.rs` runs the built binary and checks flags and exit codes.
* `python3 scripts/pty-smoke.py` (also `make pty-smoke`) drives the real TUI
  through a pseudo-terminal: it checks the alternate screen is entered and
  restored, that a page renders, that Enter on a link issues a request, and that
  `q` exits 0.

There is no external network in the tests, and no test needs an interactive
terminal.

## Not implemented (on purpose, for now)

JavaScript, CSS colours and layout (only visibility and `font-family` are
consulted, see [CSS](#css)), frames, form submission (fields are typed into,
never sent), cookies,
`mailto:`/`tel:` handlers, image display, HTTPS client certificates, and tabs.
