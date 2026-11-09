# Note-name font

`note-font-10x20.raw` contains eleven unchanged glyphs from the public-domain
X.Org 10×20 font shipped as `embedded_graphics::mono_font::ascii::FONT_10X20`
in embedded-graphics 0.7.1. The original glyphs are from
<https://gitlab.freedesktop.org/xorg/font/misc-misc>;
upstream identifies them as public domain in `fonts/src/README.md`.

The 112×20 one-bit atlas occupies 280 bytes, including row padding. Glyph order
is shared with the generator and renderer in `src/display/note_font.rs`.
Character size, spacing, baseline and pixels are preserved. The large note header
uses this subset; other screen text retains its original fonts.

Rebuild from the repository root with the host target:

```sh
CARGO_TARGET_DIR="$PWD/target" cargo run --release --locked \
  --manifest-path tools/audio-lab/Cargo.toml --target HOST_TARGET \
  --example compact_note_font -- assets/note-font-10x20.raw
```

Pixel-comparison tests cover all keypad notes in all 24 keys, both foreground/
background polarities, space, accidentals and the `?` replacement glyph.
