---
name: composing-rent-presentations
description: Use when writing, editing, or verifying a slide deck for rent, the plaintext presentation tool in this repo (a Rust port of suckless sent) — including image slides, empty slides, escaping, or checking a deck renders without opening a window
---

# Composing rent Presentations

## Overview

A rent deck is a plain text file: one paragraph = one slide. Text auto-scales
so the longest line fills at most 75% of the screen — fewer, shorter words mean
bigger text (Takahashi style). There is no markup, only the rules below.

## Format Rules

| Syntax | Meaning |
|---|---|
| blank line(s) | slide separator |
| `# ...` line | comment — skipped everywhere, even mid-paragraph or directly above one (never splits or creates a slide) |
| `@FILE` as first line of a paragraph | image slide; the rest of that paragraph is ignored |
| paragraph of exactly `\` | intentionally empty slide |
| leading `\` on a line | stripped; escapes a literal `@`, `#`, or `\` at line start |

- Escaping is only ever needed for the **first character of a line**. `\` alone
  works because the backslash is stripped, leaving an empty line — no `\\` needed.
- CRLF files work; a file with zero slides is an error.
- Image formats: farbfeld (`.ff`) natively, plus png, jpeg, gif, webp, bmp,
  tiff, tga, qoi, ico, pnm, hdr, exr. Transparency composited over background.

## Image Paths

`@FILE` is opened verbatim by the rent process — resolved against **rent's
working directory, not the slide file's location**. Keep images beside the
deck and run rent from that directory, or use absolute paths.

## Composition

- The longest line of a slide sets its font size. Target ≤ 4 short lines,
  ≤ ~25 chars per line; a one-word slide renders huge.
- Unicode and emoji render fine (per-char font fallback, color emoji).
- Appearance (colors, fonts, spacing) is compiled in: edit `src/config.rs`
  and rebuild — it is not per-deck.

## Verifying a Deck (headless)

Never needs a window: `-o` rasterizes every slide to a PDF and exits.

```sh
cargo build --release            # once
cd <deck-dir> && /path/to/rent -o check.pdf deck
```

- Exit 0 proves every slide parsed **and every image loaded** (rent dies with
  a message on a missing/broken image).
- Check PDF page count == slide count:
  `python3 -c "import re,sys;print(len(re.findall(rb'/Type\s*/Page[^s]',open(sys.argv[1],'rb').read())))" check.pdf`
  (or `pdfinfo check.pdf`). Rasterize (`pdftoppm`) only when layout matters.
- There is no parse-dump flag; `-o` export is the verification path.

## Presenting

`rent deck` (or pipe to stdin). Advance: space/enter/→/l/click/scroll.
Back: ←/h/right-click. `r` reloads the file mid-talk, `q` quits.

## Common Mistakes

| Mistake | Reality |
|---|---|
| Escaping `@`/`#` mid-line | Only line-start chars are special |
| Image path relative to the deck file | Relative to rent's cwd |
| Caption text under `@FILE` | Ignored — image slides show only the image |
| Long lines "to fit more in" | Auto-scale shrinks the whole slide |
| Verifying by launching the GUI | Use `-o check.pdf`; exit 0 + page count |
