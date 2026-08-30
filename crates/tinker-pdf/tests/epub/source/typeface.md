---
title: A Book That Brought Its Own Face
author: The tinker-pdf authors
lang: en
date: 2026-08-29
rights: Written for this repository. See tests/epub/README.md.
---

# A face the book carries

The six books this corpus started with set every word in a face the reading
system already had. That is the ordinary case and it hides a whole path: a
book may carry its own font program, name it in a `@font-face` rule, and
expect every glyph on every page to come out of the file it shipped rather
than out of whatever the reader happened to have.

This book is short on purpose. Every paragraph of it is set in the family it
embeds, so a page that came out in Times is a page that fell back — and
falling back is invisible unless something counts glyphs.

# What a fallback looks like

A fallback is not an error. The words are all there, the lines break in
plausible places, and the only thing that moved is which file the outlines
came from. `css-fonts-4` §5.3 makes the choice **per character**, so one
paragraph can be two faces without saying so anywhere.

There is one line here the embedded face has no glyph for, and it is here to
prove that the per-character rule ran rather than that the family matched:
日本語の組版.

The heading above this paragraph is bold and this sentence is not, so the two
`@font-face` rules the producer wrote are told apart by their descriptors
rather than by their family.
