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
 200 OK  https://example.com — Design notes - w3b  41 ms  18.2 kB  0%
```

* The **cursor** is a real cursor: it moves with `j k h l`, and the page scrolls
  to keep it visible. Standing on a link and pressing `Enter` follows it.
* The **colours and layout of the page are ignored**. Style comes from your
  config file only — the web's colour schemes are noise in a terminal. Two parts
  of CSS do matter and are honoured: whether an element is on the page at all,
  and which elements ask for a monospace font (see [CSS](#css)).
* **Tables** are drawn as box-drawing grids (`┌─┬┐ │ ├┼┤ └┴┘`) with the header
  rule, wrapped cells, `align` and `colspan` respected — a cell that spans
  columns takes the room of the columns it covers and the column lines it covers
  are not drawn; when the terminal is too narrow for a grid they fall back to one
  line per row.
* **Line breaks** follow the elements: a block (`div`, `p`, `h1`, …) ends a line
  and adds an empty one, an inline element (`span`, `a`, `b`, `code`, …) never
  starts or ends a line by itself, and `<br>` breaks the line inside its block
  with no empty line. Two `<br>` in a row leave one blank line between them.
* **Everything inside a list item belongs to it**: a `<li>` whose content is written
  as blocks (`<p>`, `<h2>`, `<pre>`) still has one marker, and its content is indented
  to line up under that item's text. Nested lists indent by depth.
* **An empty list is not on the page**: an `<ul>`/`<ol>` or an `<li>` with nothing in it -
  whitespace, a lone `<br>` or an empty nested list at most - shows no marker and takes
  no line, no blank line and no number in an ordered list.
* **A `<details>` is a disclosure you open**: while it is closed only its summary line is
  on the page, marked `+` and underlined in the ordinary text colour (`[style.disclosure]`) -
  never the link colour, because it is something to open rather than somewhere to go.
  `Enter` puts what it holds into the lines below, indented one level under the summary,
  and turns the marker into `-`; `Enter` again takes those lines away. A `<details>` with
  no `<summary>` gets a line saying `details` to open it from, `<details open>` starts out
  open, and a nested one waits for its parent.
* **Where a line breaks is the `display` property's call**: `display: inline` on a
  `div` keeps it in the sentence, `display: block` on a `span` gives it a line of its
  own, `inline-block` counts as inline, and an unknown value leaves the tag's default.
  Tables, lists, `<pre>`, `<hr>` and `<br>` keep the layout their tag implies.
  Only block-level tags break a line by default, so `a`, `span`, `img`, `code`,
  `input`, `button`, `label` and a `select` with its `option`s flow with the text,
  and so does any tag w3b does not know.
* **Two elements that meet with no whitespace between them** still read as separate
  words: `<b>bold</b>word` is `bold word`, two images get a space, and a cell holding
  two paragraphs does not glue their last and first word together. No space is added
  in front of closing punctuation, so `done</a>.` stays `done.`.
* **Images** are never fetched or drawn. They leave an `[img: …]` label holding
  whatever the image says about itself: `alt`, else `title`, else `aria-label`,
  else the file name from `src`. An image that says nothing is left off the page —
  a decorative `alt=""`, or a base64 `data:` source with no name. Labels are
  painted with `[style.image]` (black on purple), and stay black-and-underlined
  inside a link thanks to `[style.linked_image]`. Links that point at a `data:` or
  `javascript:` payload are plain text, never link targets.
* **Sound and video** are labelled the same way, as `[video: …]` and `[audio: …]`:
  w3b cannot play them, so it says they are there. A `<video>` that offers its
  clip through `<source>` children is labelled and addressed by the first of them.
  Whatever the tag was, `y` on its label copies the address it was written with,
  resolved against the page so the address works on its own — see `y` under Keys.
* A **link that is a heading** keeps the heading's own colours and only gains the
  link underline, so `<h2><a href=...>` still reads as a heading.
* A **link that leaves the site** is coloured by where it goes: an `href` whose host
  is not the page's own takes `[style.external]` (cyan by default), so a glance says
  whether `Enter` carries you away. The hosts are compared as spelled, so relative
  links, `#anchors`, `mailto:` and the same host on another port stay in
  `[style.link]`. An external link keeps its colour once you have read it, where a
  local one would take `[style.visited]`: what it says is the destination, not the
  visit.
* The **status line** at the bottom shows `[status]  url — title  time  size  percent`
  - the load time can be switched off with `show_load_time = false`, the size with
  `show_size = false`. The size is the body as it arrived - `412 B`, `18.2 kB`,
  `1.4 MB` - the same figure the `i` pane reports, so a page's weight and what it cost
  to get are read side by side. It is absent while a fetch is in flight and for a page
  that never arrived at all. Status comes
  from the HTTP status code (`200 OK`, `404 NOT FOUND`) or a braille spinner and
  `LOADING` while a request is in flight, and network failures show the reason
  (`ERROR DNS`, `ERROR TLS`, `ERROR TIMEOUT`, …).
* The **page info pane** (`i`) says what the page cost: `net`, `css`, `parse`
  and `layout` in milliseconds, how many bytes came over the wire, how many
  stylesheets the page asked for against how many w3b actually read, and a count
  of every kind of thing a browser would have fetched and w3b left alone -
  images, video and audio, scripts, frames, favicons, `<svg>`/`<canvas>` and
  friends. The tallies are taken while parsing, not guessed afterwards.
* A **JSON body** is read as JSON rather than as bytes. `application/json`,
  `application/ld+json` and friends are laid out one entry per line with the
  nesting shown, and the keys, strings, numbers and `true`/`false`/`null` are
  coloured by `[style.json_key]`, `[style.json_string]`, `[style.json_number]` and
  `[style.json_literal]` over the monospace slab; braces and commas take
  `[style.muted]`. A body read as a file rather than a page has no prose to set the
  code apart from, so its slab covers the whole text area, every line, rather than
  tracing the glyphs of each one. Each line is numbered down the left in
  `[style.muted]`, wide enough for the last line so the body lines up, and the
  numbers reach `--dump` too. Keys stay in the order the server sent them, the text stays
  ordinary characters, so `/` searches a document like prose and `y` still copies
  its address. A server that calls JSON `application/octet-stream` is caught by
  looking at the body, and a body that does not actually parse is shown exactly as
  it arrived with `json that does not parse` on the notice line.
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
| `Enter` | follow the link under the cursor, type into the form field under it, or open / close the `<details>` under it |
| `g` then `g` `e` `o` `i` `u` `h` `r` | go-to menu: top, end, back (out), forward (in), enter a url, home, reload (`Esc` cancels) |
| `Tab` `Shift-Tab` | next / previous link, form field or `<details>` summary |
| `o` or `:` | open a url (or a search term) |
| `/` | search, then `n` `N` for next / previous |
| `r` | reload |
| `Backspace` or `[` | back in history |
| `]` | forward in history |
| `v` `V` | mark text: `v` marks from the cursor and the far end moves with the cursor keys, `w` `b` `e`, `0` `$`, `j` `k` and the page keys, `V` takes whole lines, `Esc` drops the mark. Dragging the left button marks too. The status line counts the mark (`11 selected`) and `y` takes it |
| `y` | yank: what is marked, else the link under the cursor, else the address of the image or clip under it, else the page url. A label written inside a link yanks the link. The text goes to the terminal clipboard (OSC 52, `yank_to_clipboard`) and a `yanked: ...` note is shown |
| `w` | toggle line wrapping |
| `?` | key help |
| `i` | page info: the timing breakdown, the size of the page, and everything that was not fetched |
| while typing in a field | `Backspace` erases, `Ctrl-u` clears, `Ctrl-w` deletes a word, `Enter` submits the form the field belongs to (or is just done), `Esc` is done |
| `q`, `Ctrl-c` | quit |

`<input>` and `<textarea>` are editable: move onto one (or press `Tab` to the
next link and then `Enter`) and type. A `password` field shows dots but keeps
what you typed, and a value is remembered for as long as you stay on the page -
navigating away starts over.

`Enter` in a field is that form's submit button: a `<form>` that asks for a GET
(the method when none is written) and names an `action` is gone to, with every
named field of that form in the query string, so
`<form action=/search><input name=q value=ann><input name=sort value=new>` and
a typed `nab` fetches `/search?q=annnab&sort=new`. A field with no `name` is
left out, an `action` that already has a query keeps it, and the page the form
was on stays in the history, so `[` comes back to it. A form that asks for
another method, or names no action, is not sent and a note says so.

A `<details>` summary is opened the same way: `Tab` reaches it, `Enter` lays its
content in below the line, `Enter` again closes it. The summary keeps its row,
so the cursor never moves and nothing else on the page shifts. Opening one never
re-fetches the page: what it holds was there all along, only not shown.

The mouse wheel scrolls the page one line per notch and leaves the cursor where it
is; left-click follows the link under it, or opens the `<details>` under it; dragging
across the text marks it, and `y` then takes what was marked. Disable the whole mouse
with `mouse = false`. A click is a press and a release that come back to the same
cell, which is what keeps a drag that started on a link from navigating away. One
notch stays one line even on a terminal that reports
it as three events - WezTerm's wheel multiplier does exactly that out of the box -
because wheel events arriving in one burst are folded into the one step they
describe. The cursor only moves when the page slides its line off the window, and
then it stops one scroll margin inside it, so `Enter` and `y` always act on a line
that is on screen. The link or
summary the
cursor stands on is highlighted with `[style.hovered_link]` — the link colour
moves from the ink to the background, with black text — and its target is shown
in the status line as `-> url`. The cursor is not tied to the length of its line:
`h`/`l` and `j`/`k` carry it anywhere across the width of the page, so it keeps
its column over short lines.

Marking text - by dragging, or with `v` from the cursor - paints it with
`[style.selection]` and counts it in the status line. `y` takes it as it reads on
screen, with the lines the mark crossed joined, and reports `yanked 11 characters`;
a mark covering nothing says so instead of putting empty text on the clipboard. The
end the button went down on stays where it was while the window scrolls, so the wheel
moves the far end and not the mark, and walking the cursor past that end is not a
special case: the two ends swap. `V` takes whole lines, `v` and `V` switch between
the two, and `Esc` drops the mark and leaves the cursor where it stopped. While a
mark is open the keys that would leave the page - `o`, `:`, `/`, `n`, `N`, `r`, `[`,
`]`, `?`, `i`, `Enter` - do nothing, so a mis-hit cannot lose it; `g g` and `g e`
still stretch it to the top or the bottom of the page. A mark belongs to the text it
was made in, so a new page, a resize, `w`, opening a `<details>` or moving into a
field drops it: the lines it names are no longer those lines.

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
external = { fg = "cyan", underline = true }
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
disclosure = { underline = true }
json_key = { fg = "light_blue" }
json_string = { fg = "green" }
json_number = { fg = "yellow" }
json_literal = { fg = "light_cyan" }
hovered_link = { fg = "black", bg = "blue" }
cursor = { bg = "yellow", fg = "black", bold = true }
cursor_line = { bg = "#2b2b3b" }
search_match = { bg = "gray", fg = "black" }
selection = { bg = "light_gray", fg = "black"}
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
show_size = true
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
  `textarea`). `[style.monospace]` paints a dark slab under that text with one blank
  cell of padding on each side, and stops there: the rest of a short line, the rows
  around a block and the rest of the terminal are left alone, so no empty line below a
  block is shaded. The slab also outranks the colour set for `code`. A body read as a
  file - JSON, `text/plain`, a stylesheet - is nothing but its own text, so there the
  slab is the whole text area rather than the shape of the glyphs, and a mark on the
  text (a search hit, the cell under the cursor) is painted over the slab instead of
  under it.

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
consulted, see [CSS](#css)), frames, form methods other than GET (a form is
sent by `Enter` in one of its fields only when it asks for a GET), cookies,
`mailto:`/`tel:` handlers, image display, HTTPS client certificates, and tabs.

## How it was made

Vibecoded, all of it, and completely local. Every line here was written by a
model at the keyboard: [pi](https://pi.dev) as the agent harness, driving Qwen
Flash Next (`qwen3.8-flash-next`, IQ3_S quant) served by Strata on this machine.
No cloud endpoint, no API key, nothing uploaded - the hand-written part stops at
the prompts. What decides that a feature is done is the checks above: fmt, clippy
with `-D warnings`, the unit and integration tests, the snapshots, and the
pseudo-terminal smoke run.
