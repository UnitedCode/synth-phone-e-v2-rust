use audio_lab::note_font::{NOTE_FONT, NOTE_GLYPHS};
use audio_lab::state_machine::AppStateMachine;
use embedded_graphics::{
    mock_display::MockDisplay,
    mono_font::{ascii::FONT_10X20, MonoFont, MonoTextStyleBuilder},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Baseline, Text},
};
use synthphone_e_vocal_dsp::audio::{get_key, get_note_name};

fn render(font: &MonoFont<'_>, text: &str, inverted: bool) -> MockDisplay<BinaryColor> {
    let mut display = MockDisplay::new();
    let style = MonoTextStyleBuilder::new()
        .font(font)
        .text_color(if inverted {
            BinaryColor::Off
        } else {
            BinaryColor::On
        })
        .background_color(if inverted {
            BinaryColor::On
        } else {
            BinaryColor::Off
        })
        .build();
    Text::with_baseline(text, Point::zero(), style, Baseline::Top)
        .draw(&mut display)
        .unwrap();
    display
}

#[test]
fn every_note_in_every_key_renders_identical_pixels() {
    assert_eq!(NOTE_FONT.character_size, FONT_10X20.character_size);
    assert_eq!(NOTE_FONT.baseline, FONT_10X20.baseline);
    assert_eq!(NOTE_FONT.character_spacing, FONT_10X20.character_spacing);
    for key in 0..24 {
        for note in 0..=12 {
            let name = get_note_name(note, get_key(key));
            assert!(
                name.chars().all(|c| NOTE_GLYPHS.contains(c)),
                "unsupported note label {name}"
            );
            for inverted in [false, true] {
                assert_eq!(
                    render(&NOTE_FONT, name, inverted),
                    render(&FONT_10X20, name, inverted),
                    "key={key}, note={note}, inverted={inverted}"
                );
            }
        }
    }
}

#[test]
fn every_midi_note_label_fits_the_existing_font_without_missing_glyphs() {
    let mut app = AppStateMachine::new();
    for note in 0..128 {
        app.set_midi_note_for_display(Some(note));
        let name = app.snapshot().display_note_name();
        assert!(name.chars().all(|c| NOTE_GLYPHS.contains(c)));
        for inverted in [false, true] {
            assert_eq!(
                render(&NOTE_FONT, name, inverted),
                render(&FONT_10X20, name, inverted)
            );
        }
    }
}

#[test]
fn spaces_accidentals_and_replacement_glyph_match_the_original_font() {
    for glyph in NOTE_GLYPHS.chars() {
        let text = glyph.to_string();
        assert_eq!(
            render(&NOTE_FONT, &text, true),
            render(&FONT_10X20, &text, true)
        );
    }
    assert_eq!(
        render(&NOTE_FONT, "Z", true),
        render(&FONT_10X20, "?", true)
    );
}
