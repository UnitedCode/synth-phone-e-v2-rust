//! The large header font only needs musical note names and a replacement glyph.
pub const NOTE_GLYPHS: &str = " ?#ABCDEFGb";
pub const NOTE_FONT_WIDTH: u32 = 112;

use embedded_graphics::{
    image::ImageRaw,
    mono_font::{ascii::FONT_10X20, mapping::StrGlyphMapping, MonoFont},
};

/// Pixel-identical note glyphs from FONT_10X20; unsupported labels show `?`.
pub const NOTE_FONT: MonoFont<'static> = MonoFont {
    image: ImageRaw::new_binary(
        include_bytes!("../../assets/note-font-10x20.raw"),
        NOTE_FONT_WIDTH,
    ),
    glyph_mapping: &StrGlyphMapping::new(NOTE_GLYPHS, 1),
    ..FONT_10X20
};
