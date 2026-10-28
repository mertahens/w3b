# Changelog

Newest first. An entry says what the browser does now, in the order a reader
notices it; `git log` and the diff are the record of how. Everything under
Unreleased is work on top of `ceb19b9` (the monospace slab).

## Unreleased

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
- Trade-off worth knowing: a very fast spin whose notches land inside one burst
  counts short, because w3b cannot tell three notches from one notch reported three
  times. The alternative is the terminal's own multiplier - WezTerm's
  `wheel_scroll_multiplier` - which w3b does not touch.

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
128 lib, 46 browser, 12 cli, 2 config_defaults, 22 dump_snapshots, 21
http_server, 33 render_tui, 0 failures. `make pty-smoke` passes.

New tests, by layer:

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
