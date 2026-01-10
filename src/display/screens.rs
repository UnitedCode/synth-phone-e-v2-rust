use crate::display::cache::DisplayCache;
use crate::display::dirty::DirtyRegions;
use crate::display::text::{
    draw_centered_text, draw_text, HEADER_TEXT, INVERTED_TEXT, MENU_HIGHLIGHT, MENU_NORMAL,
    NORMAL_TEXT, SMALL_TEXT,
};
use crate::state_machine::ProcessingProfile;
use crate::types::LcdDisplay;
use embedded_graphics::{
    image::ImageRawBE,
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle},
    text::{Alignment, Baseline, Text},
};

use super::sprites::{
    draw_crush, draw_effects_bg, draw_formant, draw_key_controls, draw_octave,
    draw_process_indicator, draw_processing_bg, draw_splash, draw_waveform,
};

pub fn draw_splash_screen(display: &mut LcdDisplay, atlas: &ImageRawBE<BinaryColor>) {
    draw_splash(display, atlas);
}

pub fn draw_processing_screen(
    process: ProcessingProfile,
    key: i8,
    octave: i8,
    note: i8,
    volume: i8,
    display: &mut LcdDisplay,
    atlas: &ImageRawBE<BinaryColor>,
    cache: &mut DisplayCache,
    dirty: &mut DirtyRegions,
    force_full: bool,
) {
    // Check what changed
    let changes = dirty.update_processing(key, octave, note, volume, process);

    // If this is a full redraw, draw the background
    if force_full {
        draw_processing_bg(display, atlas);
    }

    // Update cache (only updates strings that changed)
    cache.update_processing(key, octave, note, volume, process);

    // Only redraw elements that changed (or all if force_full)
    if force_full || changes.key_changed || changes.mode_changed {
        draw_text(
            display,
            &cache.key_buffer,
            Point::new(26, 3),
            &INVERTED_TEXT,
        );
        draw_text(
            display,
            &cache.mode_buffer,
            Point::new(80, 3),
            &INVERTED_TEXT,
        );
    }

    if force_full || changes.note_changed {
        // Clear the note area first (approximate bounds)
        Rectangle::new(Point::new(50, 5), Size::new(24, 20))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
            .draw(display)
            .ok();
        draw_centered_text(display, &cache.note_buffer, Point::new(62, 15), HEADER_TEXT);
    }

    if force_full || changes.octave_changed {
        draw_text(
            display,
            &cache.oct_buffer,
            Point::new(14, 28),
            &INVERTED_TEXT,
        );
    }

    if force_full || changes.volume_changed {
        draw_centered_text(
            display,
            &cache.vol_buffer,
            Point::new(120, 28),
            INVERTED_TEXT,
        );
    }

    if force_full || changes.process_changed {
        // Clear the process profile area first
        Rectangle::new(Point::new(40, 20), Size::new(48, 10))
            .into_styled(PrimitiveStyle::with_fill(BinaryColor::Off))
            .draw(display)
            .ok();
        draw_centered_text(
            display,
            cache.process_profile,
            Point::new(62, 28),
            SMALL_TEXT,
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw_effects_screen(
    process: ProcessingProfile,
    key: i8,
    octave: i8,
    formant: i8,
    crush: i8,
    volume: i8,
    key_down_pressed: bool,
    process_cycle_pressed: bool,
    key_up_pressed: bool,
    waveform: i8,
    display: &mut LcdDisplay,
    atlas: &ImageRawBE<BinaryColor>,
    cache: &mut DisplayCache,
    dirty: &mut DirtyRegions,
    force_full: bool,
) {
    // Check what changed
    let changes = dirty.update_effects(
        key,
        octave,
        formant,
        crush,
        volume,
        process,
        waveform,
        key_down_pressed,
        key_up_pressed,
        process_cycle_pressed,
    );

    // If this is a full redraw, draw the background
    if force_full {
        draw_effects_bg(display, atlas);
    }

    // Only redraw sprites that changed
    if force_full || changes.octave_changed {
        draw_octave(display, atlas, octave);
    }

    if force_full || changes.crush_changed {
        draw_crush(display, atlas, crush);
    }

    // Draw formant or waveform controls depending on processing profile
    if force_full || changes.formant_changed || changes.waveform_changed || changes.process_changed
    {
        if process == ProcessingProfile::Vocode || process == ProcessingProfile::Dry {
            draw_waveform(display, atlas, waveform);
        } else {
            draw_formant(display, atlas, formant);
        }
    }

    if force_full || changes.key_down_changed || changes.key_up_changed {
        draw_key_controls(display, atlas, key_down_pressed, key_up_pressed);
    }

    if force_full || changes.process_changed || changes.process_cycle_changed {
        draw_process_indicator(display, atlas, process, process_cycle_pressed);
    }

    // Draw text elements (only if changed)
    draw_effects_text(
        display,
        key,
        process,
        volume,
        cache,
        force_full || changes.key_changed || changes.volume_changed || changes.process_changed,
    );
}

fn draw_effects_text(
    display: &mut LcdDisplay,
    key: i8,
    process: ProcessingProfile,
    volume: i8,
    cache: &mut DisplayCache,
    needs_draw: bool,
) {
    if !needs_draw {
        return; // Skip if nothing changed
    }

    // Update cache only if values changed
    cache.update_effects(key, volume, process);

    // Draw text elements using cached strings and styles
    Text::with_alignment(
        &cache.mode_key_buffer,
        display.bounding_box().center() + Point::new(18, 3),
        NORMAL_TEXT,
        Alignment::Center,
    )
    .draw(display)
    .expect("Draw mode text");

    Text::with_alignment(
        cache.effects_process_profile,
        display.bounding_box().center() + Point::new(16, 14),
        NORMAL_TEXT,
        Alignment::Center,
    )
    .draw(display)
    .expect("Draw process text");

    draw_centered_text(
        display,
        &cache.effects_vol_buffer,
        Point::new(120, 28),
        INVERTED_TEXT,
    );
}

pub fn draw_menu_screen(
    prev: (&str, i8),
    current: (&str, i8),
    next: (&str, i8),
    is_editing: bool,
    display: &mut LcdDisplay,
    cache: &mut DisplayCache,
) {
    display.clear();

    // Update cache only if values changed
    cache.update_menu([prev.1, current.1, next.1]);

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
        let style = if i == 1 { MENU_HIGHLIGHT } else { MENU_NORMAL };
        let y = 4 + (i as i32 * 12);

        // Draw option name
        Text::with_baseline(option.0, Point::new(5, y), style, Baseline::Middle)
            .draw(display)
            .expect("Failed to draw option name");

        // Draw option value using cached string
        Text::with_baseline(
            &cache.menu_buffers[i],
            Point::new(110, y),
            style,
            Baseline::Middle,
        )
        .draw(display)
        .expect("Failed to draw option value");
    }
}
