use crate::display::text::{
    draw_centered_text, draw_text, header_text, inverted_text, menu_highlight, menu_normal,
    normal_text, small_text,
};
use crate::state_machine::ProcessingProfile;
use crate::types::LcdDisplay;
use core::fmt::Write;
use embedded_graphics::{
    image::ImageRawBE,
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Line, PrimitiveStyle},
    text::{Alignment, Baseline, Text},
};
use heapless::String;

use super::sprites::{
    draw_crush, draw_effects_bg, draw_formant, draw_key_controls, draw_octave,
    draw_process_indicator, draw_processing_bg, draw_splash, draw_waveform,
};
use synthphone_e_vocal_dsp::audio::{get_key, get_key_name, get_mode_name, get_note_name};

pub fn draw_splash_screen(display: &mut LcdDisplay, atlas: &ImageRawBE<BinaryColor>) {
    draw_splash(display, atlas);
}

pub fn draw_processing_screen(
    process: ProcessingProfile,
    key: i32,
    octave: i32,
    note: i32,
    volume: i32,
    display: &mut LcdDisplay,
    atlas: &ImageRawBE<BinaryColor>,
) {
    // Load the background image
    draw_processing_bg(display, atlas);

    let inverted_style = inverted_text();
    let header_style = header_text();
    let small_style = small_text();

    // Process profile name
    let process_profile = match process {
        ProcessingProfile::Autotune => "Pitch Ctrl",
        ProcessingProfile::Vocode => "Vocode",
        ProcessingProfile::Dry => "Synth+Vox",
        ProcessingProfile::Harmony => "Harmony",
        ProcessingProfile::Phone => "Phone",
    };

    // Create text buffers
    let mut key_buffer: String<2> = String::new();
    write!(&mut key_buffer, "{}", get_key_name(key)).expect("Failed converting key to string");

    let mut mode_buffer: String<5> = String::new();
    write!(&mut mode_buffer, "{}", get_mode_name(key)).expect("failed converting mode to string");

    let mut note_buffer: String<2> = String::new();
    write!(&mut note_buffer, "{}", get_note_name(note, get_key(key)))
        .expect("Failed converting note to string");

    let mut oct_buffer: String<1> = String::new();
    write!(&mut oct_buffer, "{octave}").expect("Failed converting octave to string");

    let mut vol_buffer: String<3> = String::new();
    write!(&mut vol_buffer, "{volume}").expect("Failed converting volume to string");

    // Draw text
    draw_text(display, &key_buffer, Point::new(26, 3), &inverted_style);
    draw_text(display, &mode_buffer, Point::new(80, 3), &inverted_style);
    draw_centered_text(display, &note_buffer, Point::new(62, 15), header_style);
    draw_text(display, &oct_buffer, Point::new(14, 28), &inverted_style);
    draw_centered_text(display, &vol_buffer, Point::new(120, 28), inverted_style);
    draw_centered_text(display, process_profile, Point::new(62, 28), small_style);
}

#[allow(clippy::too_many_arguments)]
pub fn draw_effects_screen(
    process: ProcessingProfile,
    key: i32,
    octave: i32,
    formant: i32,
    crush: i32,
    volume: i32,
    key_down_pressed: bool,
    process_cycle_pressed: bool,
    key_up_pressed: bool,
    waveform: i32,
    display: &mut LcdDisplay,
    atlas: &ImageRawBE<BinaryColor>,
) {
    // Draw all sprite elements using the safe convenience functions
    draw_effects_bg(display, atlas);
    draw_octave(display, atlas, octave);
    draw_crush(display, atlas, crush);

    // Draw formant or waveform controls depending on processing profile
    if process == ProcessingProfile::Vocode || process == ProcessingProfile::Dry {
        draw_waveform(display, atlas, waveform);
    } else {
        draw_formant(display, atlas, formant);
    }

    draw_key_controls(display, atlas, key_down_pressed, key_up_pressed);
    draw_process_indicator(display, atlas, process, process_cycle_pressed);

    // Draw text elements
    draw_effects_text(display, key, process, volume);
}

fn draw_effects_text(display: &mut LcdDisplay, key: i32, process: ProcessingProfile, volume: i32) {
    let normal_style = normal_text();
    let inverted_style = inverted_text();

    // Format strings - TODO: Cache these later
    let mut mode_buffer: String<8> = String::new();
    write!(
        &mut mode_buffer,
        "{} {}",
        get_key_name(key),
        get_mode_name(key)
    )
    .expect("Failed converting mode to string");

    let mut vol_buffer: String<3> = String::new();
    write!(&mut vol_buffer, "{volume}").expect("Failed converting volume to string");

    let process_profile = match process {
        ProcessingProfile::Autotune => "Pitch Ctrl",
        ProcessingProfile::Vocode => "Vocode",
        ProcessingProfile::Dry => "Synth+Vox",
        ProcessingProfile::Harmony => "Harmony",
        ProcessingProfile::Phone => "Phone",
    };

    // Draw text elements using cached styles - no more style creation!
    Text::with_alignment(
        &mode_buffer,
        display.bounding_box().center() + Point::new(18, 3),
        normal_style,
        Alignment::Center,
    )
    .draw(display)
    .expect("Draw mode text");

    Text::with_alignment(
        process_profile,
        display.bounding_box().center() + Point::new(16, 14),
        normal_style,
        Alignment::Center,
    )
    .draw(display)
    .expect("Draw process text");

    draw_centered_text(display, &vol_buffer, Point::new(120, 28), inverted_style);
}

pub fn draw_menu_screen(
    prev: (&str, i32),
    current: (&str, i32),
    next: (&str, i32),
    is_editing: bool,
    display: &mut LcdDisplay,
) {
    display.clear();

    let menu_highlight = menu_highlight();
    let menu_normal = menu_normal();

    // Draw items
    let options = [prev, current, next];

    if is_editing {
        Line::new(Point::new(108, 22), Point::new(123, 22))
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 3))
            .draw(display)
            .expect("Failed to draw value underline");
    } else {
        Line::new(Point::new(2, 22), Point::new(103, 22))
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 3))
            .draw(display)
            .expect("Failed to draw name underline");
    }

    // Draw all menu items (previous, current, next)
    for (i, &option) in options.iter().enumerate() {
        let style = if i == 1 { menu_highlight } else { menu_normal };
        let y = 4 + (i as i32 * 12);

        // Draw option name
        Text::with_baseline(option.0, Point::new(5, y), style, Baseline::Middle)
            .draw(display)
            .expect("Failed to draw option name");

        // Draw option value
        let mut option_value_buffer: String<3> = String::new();
        write!(&mut option_value_buffer, "{}", option.1)
            .expect("Failed converting option value to string");

        Text::with_baseline(
            &option_value_buffer,
            Point::new(110, y),
            style,
            Baseline::Middle,
        )
        .draw(display)
        .expect("Failed to draw option value");
    }
}
