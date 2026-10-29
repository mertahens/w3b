# Changelog

Newest first. An entry says what the browser does now, in the order a reader
notices it; `git log` and the diff are the record of how. Everything under
Unreleased is work on top of `ceb19b9` (the monospace slab).

## Unreleased

### Dragging the mouse marks text, and `v` marks it without one (#6)

- Dragging the left button across a page marks the text it passed over: a band of
  light in the new `[style.selection]`, and the status line counting it
  (`11 selected`) in place of the hover hint. Letting go somewhere other than where
  the button went down leaves the mark standing, ready to be taken.
- `y` takes what is marked: the text as it reads on screen, the lines the mark crossed
  joined, into the same OSC 52 clipboard request as everything else, with the note
  `yanked 11 characters`. A mark covering nothing says `the selection is blank` rather
  than putting empty text on the clipboard. With nothing marked, `y` is what it was:
  the link, picture or page the cursor stands on.
- `v` marks from where the cursor stands, and the far end travels with the cursor: the
  cursor keys, `w` `b` `e`, `0` `^` `$`, `j` `k`, `Home` `End` and the page keys all
  move it, `V` takes whole lines rather than characters, `v` and `V` switch between the
  two, and `Esc` drops the mark and leaves the cursor where it stopped. A drag made
  first is taken over by the keys, so a mark can start under the mouse and finish
  under the keyboard.
- The end the button went down on stays where it was while the window scrolls: the
  wheel moves the far end (the cursor is parked one scroll margin inside the window, as
  it always is), and `y` still takes from the press to wherever the cursor was left.
  Walking the cursor past the other end is not a special case: the ends swap.
- A press is only a click when the release comes back to the cell it started in, so
  following a link happens on the release. Left-click still follows links and opens
  `<details>`, and a drag that starts on a link marks its text instead of navigating
  away halfway through the gesture.
- While a mark is open the keys that would leave the page (`o`, `:`, `/`, `n`, `N`,
  `r`, `[`, `]`, `?`, `i`, `Enter`) do nothing, so a mis-hit cannot lose it, and the
  `g` menu keeps only `g g` and `g e`, which stretch the mark to the top and the
  bottom of the page.
- A mark belongs to the text it was made in: a new page, a resize, `w`, opening a
  `<details>` or moving into a field drops it, because the rows it names are no longer
  those rows.
- The help overlay says `v V drag  mark text` and `y  what is marked, else link, else
  page` (snapshot re-accepted). README has the `v` / `V` key row, the reworded `y`
  row, the drag in the mouse paragraph and a paragraph on marking; the sample config
  has `selection = { bg = "light_gray", fg = "black" }`.

### A `<details>` is a disclosure you open

- The summary line of a `<details>` shows with a `+` in front of it and nothing
  below it; `Enter` on that line puts the element's content in the lines below,
  indented one level, and turns the marker into a `-`. `Enter` again closes it
  back down.
- Opening does not re-fetch and does not move the cursor: the summary keeps its
  row, so a long page stays where you were. The status line says `opened <label>`
  / `closed <label>`.
- A summary is something to open rather than somewhere to go, so it does not wear
  the link colour: ordinary text colour with an underline, from the new
  `[style.disclosure]`. The two columns in front of the marker are not
  underlined, and a link written inside a summary still looks like a link.
- The whole summary line is a target: `Tab` / `Shift-Tab` stop there, `Enter`
  anywhere on the line (indent included) toggles, and `[style.hovered_link]`
  highlights the whole line under the cursor. A left-click opens it. Walking past
  the last target now says `no more links, fields or disclosures`.
- A `<details>` that does not name a `<summary>` gets one saying `details`. A
  `<details open>` is laid out already open. An empty `<summary></summary>` still
  leaves the `+` row, so it can be opened. A nested `<details>` stays hidden until
  its parent is open, and its nested summary line is hidden with it.
- A `<details>` inside a table cell is plain text: a marker there would be a knob
  with nothing to turn, since a cell has no per-block hiding.

### A table cell says how many columns it covers (#289)

- `colspan` on `<td>`/`<th>` is honoured: the grid is counted in columns rather
  than cells, so a spanning cell takes the room of the columns it covers.
- The value is clamped to what a terminal can hold: `colspan=0`, a bare
  `colspan`, and `colspan=abc` are one column; a parseable huge value tops out at
  1000 columns rather than asking for the world.
- A wide spanning cell widens the columns it sits on instead of overflowing, and
  wraps inside its own cell when the table gets narrow.
- No vertical rule is drawn across a span, and a horizontal rule loses its `┬ ┼ ┴`
  junction wherever a cell covers that boundary - the line stays unbroken under a
  spanning first-row cell and over a spanning last-row cell.
- Cells written past the last column are dropped; trailing columns nobody covers
  get an empty cell. A table without `colspan` draws exactly as before.

### `Enter` in a field sends its form (#499)

- Pressing `Enter` after typing in a field is that form's submit button, when the
  form asks for a `GET` and names an `action`. The action is resolved against the
  page url, its fragment dropped, and every **named** field of that form goes into
  the query: `<form action=/search><input name=q value=ann><input name=sort
  value=new></form>` plus typed `nab` fetches `/search?q=annnab&sort=new`.
- Query encoding is form-urlencoded, so a space becomes `+` and `&` becomes
  `%26`. Fields with no `name` attribute are left out the way a browser leaves
  them out. An action that already carries a query keeps it. A GET form with
  nothing to send still goes to the action with an empty query.
- `password` fields are sent, as browsers send them. Untouched fields are sent at
  their attribute value.
- `[` comes back to the form page after a submit, the same history a link follow
  gets. The typed text is lost on going back.
- A form w3b will not send is reported instead of ignored: `w3b sends GET forms
  only, this one wants post`, or `unsupported form action: mailto`. The page stays
  put and the typed text is kept. A form with no action is done quietly, because
  many forms have none.
- The typing hint says what `Enter` does: `(Enter submits, Esc to finish)` when the
  form can be submitted, `(Enter or Esc to finish)` when it cannot.
- `Field.name` now holds the `name` attribute exactly, empty when the control has
  none; the status hint resolves name, then placeholder, then `field`.
- Not-implemented list changed accordingly: form submission is out, **form methods
  other than GET** are the limit now.

### A link to another site has its own colour (#643)

- An `href` whose host is not the page's own is drawn with the new
  `[style.external]`, cyan and underlined by default, so a glance says whether
  `Enter` carries you away.
- Hosts are compared as the url spells them, and the url parser lowercases them:
  `HTTP://ELSEWHERE.example/y` counts. Relative links, `#anchors`, `mailto:` /
  `tel:`, an unparseable href, and the same host on another port stay in
  `[style.link]`.
- An external link keeps its colour after you have read it, where a local one
  would take `[style.visited]`: what the colour says is the destination, not the
  visit.
- A link that is a heading still keeps the heading's colours, and an image label
  inside a link still stays black-and-underlined.

### Empty list items leave no trace (#1)

- An `<li>` with nothing to say adds no marker row and no blank line, and it does
  not use up a number in an `<ol>`.
- Nothing to say means: no text, only separator spaces, only whitespace, only
  `&nbsp;`, only a `<br>`, only an empty `<p>`, `<pre>`, `<img alt="">` or
  `<blockquote>`, or only a nested list of empty items. `<ul></ul>` and
  `<ol>  </ol>` render as nothing at all - `dump::render_html` returns a single
  newline.
- Content hidden by `display: none` counts as nothing, so
  `<ul class=nav><li><a href=/x></a></li><li><a href=/y>y</a></li></ul>` is just
  `• y`.

### Standing on a picture, `y` copies where it came from

- `<video>` and `<audio>` are on the page now, labelled `[video: …]` and
  `[audio: …]` the way an image is labelled `[img: …]`. w3b still cannot show or
  play them - the label is the point - and the label is built the same way: `alt`,
  else `title`, else `aria-label`, else the file name out of the address. A `<video>`
  that offers its clip through `<source>` children takes the first of those as its
  address, and its fallback content (usually a "download it" link) is not put on
  the page as well.
- `y` on any of those labels copies the address the tag was written with, resolved
  against the page url, so a relative `src="logo.png"` leaves the terminal as
  `https://example.com/logo.png`. A `<video>` with no `src` of its own reports the
  `<source>` it was given; a `data:` address comes back as written, however long it
  is; an image that names no address falls back to yanking the page, as before.
- A label written inside a link yanks the **link**, not the image: the link is what
  a click would use, and it is what the cursor is standing on.
- Image labels keep their old look - `[style.image]`, and `[style.linked_image]`
  inside a link - because the flag that drives that colour is now "this piece names
  a piece of media" rather than "this piece is an image".
- An image inside a table cell is yankable too: the word-by-word layout of a cell
  now carries the media index along with the link and field it already carried.

### The wheel is one line, however the terminal reports it

- A wheel notch that reached the screen as three events used to move the cursor
  three lines: WezTerm multiplies the wheel by three by default, and Windows
  Terminal is similar, so one physical notch arrives as a burst of three scroll
  events. The event loop now reads a whole burst at a time and folds a run of the
  same wheel event in that burst into the one step it is, so one notch is one line
  again.
- Anything else in the burst survives in order: a key, a resize, or the wheel
  rolling the other way (which is its own step).
- Checked through a real pty with the release binary: three `ESC[<65;col;rowM`
  wheel reports written in one go move the cursor exactly one line, where the same
  three reports move it three lines with the fold switched off.
- The notch no longer moves the cursor either: `App::scroll_by` slides the window
  and leaves the cursor on the line it was standing on, so the highlight travels
  with the text instead of chasing the wheel. When the window slides that line out
  of view the cursor is parked one `scroll_margin` inside the edge - the same band
  the keys keep it in - so a cursor left at the bare edge cannot make the next `j`
  drag the page back up by the margin.
- Trade-off worth knowing: a very fast spin whose notches land inside one burst
  counts short, because w3b cannot tell three notches from one notch reported three
  times. The alternative is the terminal's own multiplier - WezTerm's
  `wheel_scroll_multiplier` - which w3b does not touch.

### `i` says what the load cost, and what was left out

- New pane, and the keys were split to make room: `?` is now the only key for the
  help overlay and `i` opens the page info box (`Mode::Info`). The help text says
  so on its own first line and names `i` in its list.
- The timings are measured, not estimated. `net` is the fetch thread's own elapsed
  time, sent back with the reply; `css` is the whole stylesheet gather including its
  serial HTTP round trips; `parse` is decode + DOM + link resolution; `layout` is
  `relayout`. The total is the same `load_ms` the status line already showed, so the
  parts can sum to less than it: the wait between a reply landing and the main loop
  picking it up (up to the 60 ms poll) belongs to no stage.
- The pane lists what a browser would have asked for and w3b did not: the DOM walk
  tallies `<img>`, `<video>`/`<audio>`, `<link rel=stylesheet>`, `<script>`,
  `<iframe>`/`<frame>`/`<object>`/`<embed>`, the markup families it skips (`<svg>`,
  `<math>`, `<canvas>`, `<picture>`, `<source>`, `<track>`, `<template>`,
  `<noscript>`, `<map>`) and favicons or other non-stylesheet `<link>`s. `<style>`
  is not counted, because embedded CSS really is read, and a `<meta>` is not,
  because it asks for nothing. Elements inside an already skipped element are never
  reached, so the tally is what the walk saw, not what the page contains.
- Stylesheets are reported from two passes on purpose: `css::extract` counts the
  `<link rel=stylesheet>`s in the raw html (found) and `gather_css` counts how many
  were fetched inside `max_stylesheets` (read), which is the only place that knows
  both numbers. A page asking for 10 stylesheets with `max_stylesheets = 4` says
  `4 of 10` read and puts 6 on the left-out list.
- `Page::bytes` records the body as it was sent, which the pane reports in
  B / kB / MB. The status line picked up the same figure further down this list.
- The centred box is one function now: `render_box` holds the clear, border, title
  and centring, and `render_help` and `render_info` are callers of it.

### A JSON body is read rather than refused

- `application/json` used to fall through the content type table into
  `Kind::Binary`, so a JSON response answered `N bytes of binary data - not
  displayed`. There is a `Kind::Json` now, and the body is laid out: one entry per
  line, two spaces a level, an empty object or array kept whole on its own line.
- The parts are coloured - `[style.json_key]`, `[style.json_string]`,
  `[style.json_number]`, `[style.json_literal]`, with braces and commas in
  `[style.muted]` - over the monospace slab a `<pre>` already gets. Nothing about
  the body is rewritten, so `/` and `n` search a JSON document like any other text
  and `y` still copies its address.
- Key order survives. There is no JSON parser in the dependency tree, and the
  scanner w3b grew instead keeps slices of the original text, which is what lets a
  server's own order reach the screen (a map would have sorted it) and lets a body
  with a stray byte be shown as it arrived instead of half-shown.
- A `Content-Type` that lies is caught: `application/octet-stream` gets its bytes
  scanned, and a JSON body is laid out anyway. The scan gives up on the first
  character of a real download, so the check is free where it is not needed.
- A body the header called JSON and the scanner disagrees with (`{"a":1,}`) is
  shown exactly as sent, with `json that does not parse (N bytes) - shown as it was
  sent` as the notice rather than an empty page.
- Two rules inherited from preformatted text, worth knowing: a JSON line longer
  than the terminal is not wrapped (it is a pre line, and pre lines scroll sideways
  in w3b today), and the body carries the usual two cells of `<pre>` indent on top
  of its own nesting.
- Bug fixed on the way, in `layout_pre`: every piece of a preformatted line started
  at the same column, so a `<pre>` with more than one piece drew its text on top of
  itself. Each piece now starts where the previous one ended.
### A JSON body is numbered down the side

- Every line of a JSON body now carries the line it is on, right aligned in as many
  columns as the last line needs, so a 300 line document reads
  `  1 {`, `  2   "name": "w3b"`, ... `300 }`. The number is in `[style.muted]`, the
  colour of a thing that is there to be read past, and it sits on the same
  monospace slab as the body.
- It is an ordinary run of text in front of the line, which buys three things for
  free: it scrolls sideways with a long line, it survives `--dump` (the plain text
  dump of a JSON body is numbered too), and the nesting still starts at the same
  column on every line because the gutter width comes from the line count.
- The dimming of `[style.muted]` now survives the monospace slab: it used to be
  patched before it, so anything muted on a monospace line came back in the slab's own
  text colour. A mark on the text says what the text is; the slab only says it is
  monospace, so the mark goes last.

- A body the header called JSON and the scanner disagreed with keeps no gutter:
  those lines are the file's own, not a layout w3b made, and numbering them would
  promise something w3b does not know.

- Second bug fixed on the way, in the monospace slab: the slab patch carried
  `[style.monospace]`'s own foreground over cells that already had a colour, so
  links, search hits and now JSON parts lost their colour inside a monospace line.
  The slab paints the background and leaves the glyph's colour alone.

### A file opened in its own window is monospace all the way across

- The monospace slab used to be the shape of the code: it traced the glyphs of each
  run, one cell of padding either side, which is right for a `<code>` inside a
  paragraph and wrong for a body that is nothing but code. A body read as a file -
  JSON, `text/plain`, a stylesheet - now gets the slab across the whole text area,
  every line, so the file's window reads as one block of monospace text instead of
  a column of short bars with the page background beside them (`Page::file_view`,
  set from the content type in `Page::from_fetched_css`).
- Pages keep the old rule: in prose the slab still stops where the code does.
- A mark on the text is painted over the slab, not under it. A search hit keeps
  `[style.search_match]` even on a monospace line, and the cell under the cursor
  keeps the cursor, which on a wall of identical characters is the one thing that
  has to stand out.

### A search hit is a grey band, not a shout

- `[style.search_match]` was a yellow background with black glyphs, which read as a
  warning and competed with the cursor - its default is a grey background (`bg =
  "gray"`) with black glyphs now. Set it back in `~/.config/w3b/config.toml` if the
  yellow was wanted.

### The status line says how big the page was

- Next to the load time the status line now says the size of the body as it arrived:
  `200 OK  https://example.com/doc - Design notes  41 ms  423 B  4%`, in
  `[behavior] show_size = true`, which can be switched off like the time. Bytes below
  a kilobyte, one decimal above it (`412 B`, `18.2 kB`, `1.4 MB`).
- It is the same number the `i` pane reports - one formatter, `status::human_size`,
  which the info pane now shares instead of keeping its own copy - and it is the
  fetched body, not the text on screen: a 400 kB page that draws 60 lines reports the
  400 kB, which is the thing a reader on a bad connection wants to know.
- Nothing is shown while a fetch is in flight (the size on screen would be the old
  page's), and nothing for a page that never came over the wire: a connect failure, a
  DNS failure, or a page w3b wrote itself. A 404 the server did send shows its bytes,
  because they were sent.
- The right hand side of the status line is now a list of parts joined with a blank
  column rather than one fixed shape, since either of the two figures can be switched
  off and the size can be simply absent.

### Help, docs and the smoke run

- The help overlay changed: `Enter follow/edit/open`, `Tab next target`, `y
  link/media`, `in a field typing edits it, Esc leaves it`, `Enter there sends the
  form, if it asks for a GET`, and `on a + or - Enter shows or hides the lines under
  it`. The screen snapshot was re-accepted.
- README: behaviour bullets for disclosures, spanning cells, form submit and
  off-site links; the `Enter` and `Tab` key rows; a paragraph on opening summaries
  without moving the cursor or re-fetching; the mouse paragraph (left-click opens
  the `<details>` under it); sample config lines `disclosure = { underline = true }`
  and `external = { fg = "cyan", underline = true }`; and a new **How it was made**
  section saying the project was vibecoded completely locally - pi as the harness,
  Qwen Flash Next (`qwen3.8-flash-next`, IQ3_S quant) served by Strata, no cloud
  endpoint and no API key.
- `scripts/pty-smoke.py` (also `make pty-smoke`) now drives a real form submit
  through the pseudo-terminal: the second page's input sits in
  `<form action=/found>`, `Enter` is sent instead of `Esc`, and the run only
  passes when the request that comes back is `/found?q=typed-inX`.
- It then opens a third page by typing its url, whose first line is an image label
  - so the cursor starts standing on it - presses `y`, and only passes when the
  status line reads `yanked: http://127.0.0.1:<port>/img/logo.png` and the OSC 52
  clipboard request carrying that same address (base64, checked byte for byte) is in
  the transcript.

## Tests

Current `make check` (fmt, clippy with `-D warnings`, all tests) after this work:
138 lib, 65 browser, 12 cli, 2 config_defaults, 23 dump_snapshots, 21 http_server,
43 render_tui, 0 failures. `make pty-smoke` passes.

New tests, by layer:

- **marking text** - browser: `dragging_the_mouse_marks_text_and_y_takes_it`,
  `a_drag_across_a_link_marks_it_instead_of_following_it`,
  `a_click_after_a_drag_drops_the_mark`,
  `the_wheel_while_marking_keeps_the_mark_and_only_moves_its_far_end`,
  `v_marks_text_and_the_cursor_keys_move_its_far_end`, `capital_v_takes_whole_lines`,
  `v_and_capital_v_switch_between_characters_and_whole_lines`,
  `a_drag_mark_turned_into_lines_takes_the_whole_rows`,
  `a_mark_that_the_cursor_walks_past_is_still_the_same_mark`,
  `gg_in_a_mark_stretches_it_to_the_top_of_the_page`,
  `a_mark_silences_the_keys_that_would_leave_the_page`,
  `esc_leaves_a_mark_and_leaves_the_cursor_where_it_stopped`,
  `a_blank_mark_says_so_rather_than_yanking_nothing`,
  `a_mark_belongs_to_the_page_it_was_made_on`, `a_re_layout_drops_the_mark`,
  `a_press_lands_after_a_note_rather_than_keeping_it`;
  `mouse_click_follows_a_link` now presses and releases in the same cell, since a
  press on its own cannot tell a click from a drag. render_tui:
  `a_mark_is_painted_over_the_cells_it_covers`,
  `a_mark_over_code_is_painted_over_the_slab`,
  `dragging_the_mouse_marks_the_cells_it_passed_over`,
  `the_status_line_says_what_is_marked_and_what_was_taken`.
- `make pty-smoke` drags a real gesture across the first line of the media page (one
  event per cell, released on the last), presses `y`, and only passes when a
  `… selected` count appears while the mark is open and the OSC 52 request carries
  exactly `MEDIA-BELOW`. What reaches the screen is a diff of the cells that changed,
  so a word can be split by whatever else moved: the notes on the status line are
  matched loosely, and only whole writes - the clipboard request - byte for byte.

- **disclosures** - dom: `a_closed_disclosure_shows_only_its_summary_line`,
  `an_open_disclosure_shows_what_it_holds`,
  `opening_a_disclosure_puts_its_content_in_the_lines_below`,
  `a_details_that_does_not_name_a_summary_gets_one`,
  `a_nested_disclosure_waits_for_its_parent`, `a_summary_line_is_the_line_that_opens`,
  `a_disclosure_inside_a_cell_is_plain_text`; layout:
  `a_summary_line_marks_its_disclosure_end_to_end`,
  `an_open_disclosure_adds_rows_below_and_closes_back_up`,
  `a_disclosure_line_wraps_under_its_own_marker`; browser:
  `enter_opens_a_details_and_closes_it_again`,
  `a_details_written_open_shows_its_content_and_enter_shuts_it`,
  `a_hidden_disclosure_is_nowhere_to_be_found`; render_tui:
  `a_summary_line_is_underlined_text_and_not_a_link`,
  `the_cursor_on_a_summary_marks_the_whole_line`; dump:
  `a_disclosure_shows_its_summary_and_opens_below_it` with the new
  `disclosure_closed` / `disclosure_open` snapshots.
- **colspan** - dom: `a_cell_that_spans_says_how_many_columns_it_covers`; layout:
  `a_spanning_cell_takes_the_room_of_the_columns_it_covers`,
  `a_spanning_cell_widens_the_columns_it_sits_on`,
  `a_spanning_cell_in_the_last_row_leaves_the_rule_unbroken`; dump:
  `table_cells_that_span_columns` with the new `table_colspan` snapshot.
- **forms** - page: `enter_in_a_field_sends_its_form_as_a_query`,
  `a_field_that_was_never_named_is_not_sent`,
  `an_action_that_already_asks_for_something_keeps_it`,
  `only_a_get_form_with_an_action_is_sent`; dom:
  `a_field_belongs_to_the_form_it_is_written_in`; browser:
  `enter_in_a_field_sends_the_get_form_it_belongs_to`,
  `a_form_w3b_will_not_send_is_reported_and_stays_put`.
- **off-site links** - page: `a_link_that_names_another_host_leaves_the_site`;
  render_tui: `a_link_off_the_site_is_coloured_by_where_it_goes`,
  `a_visited_external_link_does_not_start_looking_local`,
  `the_shipped_config_makes_an_off_site_link_cyan`.
- **the wheel** - tui: `a_burst_of_wheel_events_is_one_step`,
  `a_wheel_burst_keeps_everything_that_is_not_the_wheel`,
  `the_wheel_turning_both_ways_in_one_burst_is_both_steps`,
  `a_burst_of_nothing_is_nothing_to_do`. The app-level
  `one_wheel_notch_moves_one_line` in browser.rs is unchanged: one event, one line.
- **json** - json: `an_object_is_one_entry_a_line_with_the_nesting_shown`,
  `each_part_carries_its_own_role`, `the_keys_keep_the_order_the_server_wrote_them_in`,
  `what_is_not_json_is_said_so_and_shown_raw`, `any_json_value_is_a_body_not_only_an_object`,
  `nesting_past_the_cap_is_not_json_rather_than_a_crash` and
  `a_json_document_is_one_block_a_line_and_monospace`; dump_snapshots:
  `a_json_body_is_laid_out_by_its_nesting`; render_tui:
  `the_parts_of_a_json_body_take_their_own_colours`; browser:
  `a_json_body_can_be_searched_like_any_other_text` and
  `a_json_body_the_server_broke_is_still_shown`; json also has
  `the_gutter_numbers_the_lines_of_the_body`, and the `json_body` dump snapshot now
  shows the numbered body. render_tui also has
  `a_direct_view_of_a_file_is_monospace_across_the_whole_line`, which checks that the
  slab reaches the end of the row and that a hit inside it keeps its grey band,
  `any_body_read_as_a_file_gets_the_slab_across_the_row` for `text/plain`, and
  `code_inside_a_page_is_only_slabbed_where_the_code_is` for the other half of the
  rule; `search_matches_are_highlighted` now pins the shipped grey-and-black instead
  of yellow.
- **status line** - status: `a_size_is_said_the_way_a_status_line_says_it`;
  render_tui: `the_size_of_the_page_sits_next_to_the_load_time`, which pins the body
  size rather than the text, the kilobyte rounding, a fetched 404 that has a size and
  a page that never arrived that has none, and the switch.
- **info pane** - dom: `the_page_counts_what_w3b_leaves_out`, page:
  `the_stylesheet_count_says_found_and_read` and
  `a_load_breaks_into_fetch_css_parse_and_layout`, browser:
  `i_opens_the_page_info_pane_and_any_key_closes_it`, render_tui:
  `the_info_pane_reports_what_the_page_cost` with a snapshot of the box.
- **media and yanking** - dom:
  `a_media_label_keeps_the_address_it_was_written_with`,
  `a_media_label_is_one_line_and_carries_what_is_inside_it`; page:
  `an_image_label_says_where_the_image_is`; layout:
  `an_image_label_remembers_which_image_it_stands_for`,
  `an_image_inside_a_cell_is_still_an_image`; browser:
  `y_on_an_image_copies_where_the_image_came_from`,
  `y_on_a_video_or_a_sound_copies_the_clip`,
  `an_image_written_inside_a_link_yanks_the_link_it_was_wrapped_in`,
  `an_image_that_names_no_address_yanks_the_page_as_before`; dump:
  `media_that_cannot_be_played_is_labelled_in_its_place` with the new
  `media_labels` snapshot. The existing `images_are_labelled_and_painted_on_their_own_colour`
  and `an_image_inside_a_link_stays_black_and_underlined` still pass unchanged.
- **empty items** - dom: `an_item_that_says_nothing_is_not_on_the_page` (11
  spellings), `an_empty_item_does_not_use_up_a_number_or_a_line`; layout:
  `an_item_with_nothing_in_it_adds_no_row`; dump:
  `empty_lists_and_items_leave_no_trace`.

## Found, not fixed

- **A table inside a cell wipes the cell around it.** The `<td>`/`<th>` open handler
  clears the current cell text with no cell stack, so
  `<td>out<table><tr><td>in</td></tr></table></td>` loses `out`. This predates the
  colspan work and is unrelated to it; it deserves its own issue.
- **An inline `<details>` breaks the line it sits in.** On lobste.rs each byline has
  `<span> | </span><details class=caches><summary>caches</summary><ul>…` in the
  middle of a line of text, so the byline splits into three lines with a blank line
  on each side of the summary. The blank lines themselves are the ordinary
  between-blocks separator - it is the same for two paragraphs, a heading, a list or
  an `<hr>` - but the line break at an inline `<details>` is w3b's doing, and
  whether `<details>` should behave as inline or as a block when its parent is a run
  of text is not settled.
