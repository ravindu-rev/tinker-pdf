---
title: A Short Account of Containers
author: The tinker-pdf authors
lang: en
date: 2026-08-19
rights: Written for this repository. See tests/epub/README.md.
---

# What a container is

A container is a file that is other files. That sentence is the whole of the
idea and almost none of the difficulty, because the moment a format says *these
bytes are a document made of parts* it has to say which part is the document,
what order the parts go in, and what happens when a part names another one that
is not there.

This book exists so that a real producer has something of ours to convert. Its
words are not the point; its *shape* is. It has chapters, so a spine has more
than one entry. It has a table, a list, a quotation and a code block, so a
cascade has more than one box type to get wrong. It has an em&#8209;dash — and an
ellipsis … and a non-breaking space, so an encoder has to decide between a
named reference and a numeric one. It cross-references
[the second chapter](#the-order-of-things) and
[the third](#what-goes-wrong-quietly), so a synthesiser has to turn an internal
`href` into something a page can hold.

> A reader that has only ever opened files written by its own writer has not
> been tested. It has been agreed with.

## The order of things

Three orders exist in an archive and they are routinely confused:

1. the order the entries were **written**, which is physical offset order;
2. the order the **central directory** lists them, which a producer may choose
   freely; and
3. the order a **spine** names them, which is the only one that is the book.

An OCF container adds a fourth rule that touches only the first: the entry
named `mimetype` must be the first file *in the archive*, stored rather than
deflated, with no extra field. That is a statement about physical offset, and
an implementation that checks it against the central directory's first row is
checking something else that happens to agree most of the time.

| Order | Where it lives | Who chooses it |
| --- | --- | --- |
| Physical | Local file headers | The writer |
| Directory | Central directory | The writer |
| Reading | `spine`/`itemref` | The author |

A stylesheet can move a heading. It cannot move a chapter. The two failures
look nothing alike and a reader that loses the second one still renders
beautifully.

## What goes wrong quietly

The interesting failures in this area are not crashes. A crash is a gift: it
has a stack trace and a line number and somebody fixes it that afternoon. The
failures worth writing a book about are the ones that produce a plausible
answer.

- A page count that is the number of pictures rather than the number of pages.
- A margin dropped on one element, which moves every line below it.
- An archive read in lexicographic order, so chapter 10 arrives before
  chapter 2 and every page is perfect.

Here is what a checker sees, which is not what a reader sees:

```text
mimetype                 stored   20 bytes   offset 0
META-INF/container.xml   deflated            offset 62
content.opf              deflated
```

The three lines above are indented in the source of this file on purpose, so
that white-space processing has something to do. So is the paragraph that
follows, which begins with     several spaces that CSS is required to collapse
into one.

A line of Japanese, because a line breaker that only knows about spaces passes
every test ever written in English: 日本語の組版では行の折り返しに空白を使いません。

And one token no line box can hold:
`Donaudampfschiffahrtselektrizitaetenhauptbetriebswerkbauunterbeamtengesellschaft`.

# Where the parts are named

The package document is the only file in an EPUB that knows what the book is.
Everything else is a part that something else points at. Delete the package
document and what is left is a folder of web pages in alphabetical order —
which, as it happens, is exactly what this engine currently makes of a book
that still has one.

See [the first chapter](#what-a-container-is) for why that is a container
problem rather than a rendering one.
