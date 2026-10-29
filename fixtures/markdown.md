---
title: A file that is not HTML
tags: [markdown, w3b]
---

# Reader

A paragraph that wraps onto the next line and is joined into one row of text
again. Two spaces at the end of a line are a break.  
Those were two lines.

An [inline link](/one), one with a title and a query [two](/two?a=1&b=2 "second"),
a [reference][ref], a [shortcut] and an [implicit][] reference, an
<https://example.org/autolink> address, a <nobody@example.org> one, `inline code`
including <angle> brackets, **both kinds of strong**, *both kinds of emphasis* and
***all at once***. A star that is \*not emphasis\* stays a star, `snake_case_names`
stay themselves, and 5 < 6 && R&D is escaped rather than applied.

Entities arrive as themselves: &amp; &#39; &copy; while &nope; stays literal.
Raw HTML is text here: <b>bold</b> shows its own tags.

![A cat](/cat.png "A cat, sitting")

## Counting

* bullets
* with a nested list
  - deeper
  - still deeper
* `code`, a [link](/three) and a period after the closing mark.

1. ordered
2. from one
10. and a list that says where it starts

- [ ] a task that is not done
- [x] a task that is

## Quoting

> A quote holds a paragraph
> and continues lazily.
>
> - a list inside the quote
> - with a [link](/four)
>
> > A quote inside the quote

## Coding

```rust
fn main() {
    println!("hello");
}
```

A list item with a fence of its own:

* run this:
  ```sh
  cargo run -- --dump https://example.org/README.md
  ```
* and then this.

    an indented block is code too
    with    its    spacing

## Tables

| Column | Another | Third |
|:-------|:-------:|------:|
| left   | centre  | right |
| `1`    | [link](/five) | **bold** |

## Rules

---

The end: a reference definition lives at the bottom of the file, where a
[a link][ref] can find it.

  [ref]: /the-reference-target "And a title"
  [shortcut]: /shortcut
  [implicit]: /implicit
