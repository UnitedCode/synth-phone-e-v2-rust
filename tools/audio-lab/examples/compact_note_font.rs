//! Rebuild the note-name atlas from embedded-graphics' public-domain X.Org font.
use audio_lab::note_font::{NOTE_FONT_WIDTH, NOTE_GLYPHS};
use embedded_graphics::{
    mock_display::MockDisplay,
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Baseline, Text},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args()
        .nth(1)
        .ok_or("Usage: compact_note_font OUTPUT.raw")?;
    let mut bitmap = vec![0_u8; NOTE_FONT_WIDTH as usize * 20 / 8];
    for (index, glyph) in NOTE_GLYPHS.chars().enumerate() {
        let mut display = MockDisplay::<BinaryColor>::new();
        Text::with_baseline(
            &glyph.to_string(),
            Point::zero(),
            MonoTextStyle::new(&FONT_10X20, BinaryColor::On),
            Baseline::Top,
        )
        .draw(&mut display)?;
        for y in 0..20 {
            for x in 0..10 {
                if display.get_pixel(Point::new(x, y)) == Some(BinaryColor::On) {
                    let bit = y as usize * NOTE_FONT_WIDTH as usize + index * 10 + x as usize;
                    bitmap[bit / 8] |= 0x80 >> (bit % 8);
                }
            }
        }
    }
    std::fs::write(output, &bitmap)?;
    println!(
        "Wrote {} bytes for {} glyphs",
        bitmap.len(),
        NOTE_GLYPHS.chars().count()
    );
    Ok(())
}
