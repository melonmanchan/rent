# rent

A simple plaintext presentation tool — a Rust port of suckless [sent](https://tools.suckless.org/sent/).

rent does not need LaTeX, PowerPoint, or a web browser. Write your slides as
plain paragraphs, run `rent`, and present. One paragraph is one slide. Text is
scaled to fit the window, so slides with little text are big and slides with
much text are small — which nudges you toward the
[Takahashi method](https://en.wikipedia.org/wiki/Takahashi_method).

## Build

```sh
cargo build --release
```

The binary ends up at `target/release/rent`. Rendering is CPU-based
(winit + softbuffer + swash), so there are no GPU or system library
requirements beyond a working windowing system.

## Usage

```
usage: rent [-v] [-o output.pdf] [file]
```

- `rent FILE` — present `FILE`
- `rent` or `rent -` — read slides from stdin
- `rent -o slides.pdf FILE` — headless export: rasterize all slides into a PDF and exit
- `rent -v` — print version

Try the included example:

```sh
cargo run --release -- example
```

### Controls

| Input | Action |
|---|---|
| `→` `↓` `Enter` `Space` `PageDown` `l` `j` `n`, left click, scroll down | next slide |
| `←` `↑` `Backspace` `PageUp` `h` `k` `p`, right click, scroll up | previous slide |
| `r` | reload the slide file |
| `q` `Escape` | quit |

## File format

- One slide per paragraph; paragraphs are separated by blank lines.
- Lines starting with `#` are comments and are ignored.
- A paragraph whose first line starts with `@FILENAME` becomes an image slide;
  the rest of the paragraph is ignored.
- A paragraph consisting of `\` is an empty slide.
- Prepend `\` to kill the special meaning of `@`, `#`, or `\` at the start of a line.

Images are handled natively in the
[farbfeld](http://tools.suckless.org/farbfeld/) format, plus png, jpeg, gif,
webp, bmp, tiff, tga, qoi, ico, pnm, hdr, and exr (decoded in pure Rust via the
`image` crate). Transparent images are composited over the configured
background color.

See [`example`](example) for a presentation that documents the format.

## Configuration

Suckless style: edit [`src/config.rs`](src/config.rs) and rebuild. It mirrors
sent's `config.def.h` and controls fonts (including per-character fallback and
color emoji faces), foreground/background colors, line spacing, usable screen
area, PDF export resolution, and key/mouse bindings.

## License

MIT


## "Author's" note

I fully understand the hypocrisy of vibecoding a copy of a suckless tool :P
