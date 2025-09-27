use crate::display::text::{draw_centered_text, draw_text};
use crate::state_machine::ProcessingProfile;
use crate::types::LcdDisplay;
use core::fmt::Write;
use embedded_graphics::{
    image::{Image, ImageRawBE},
    mono_font::{
        ascii::{FONT_10X20, FONT_5X7, FONT_6X13, FONT_6X9},
        MonoTextStyleBuilder,
    },
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Line, PrimitiveStyle, Rectangle},
    text::{Alignment, Baseline, Text},
};
use heapless::String;

use synthphone_e_vocal_dsp::audio::{get_key, get_key_name, get_mode_name, get_note_name};
use tinybmp::Bmp;

pub fn draw_splash_screen(display: &mut LcdDisplay) {
    display.clear();

    // Display the splash image
    let bmp: Bmp<BinaryColor> = Bmp::from_slice(include_bytes!("../../assets/synthophoneV2.bmp"))
        .expect("Could not load splash BMP");

    let image = Image::new(&bmp, Point::new(0, 0));
    image.draw(display).expect("Failed to display splash image");
}

pub fn draw_processing_screen(
    process: ProcessingProfile,
    key: i32,
    octave: i32,
    note: i32,
    volume: i32,
    display: &mut LcdDisplay,
) {
    display.clear();

    // Load the background image
    let bmp: Bmp<BinaryColor> =
        Bmp::from_slice(include_bytes!("../../assets/SynthphoneE_MenuBlank.bmp"))
            .expect("Could not load BMP");

    let image = Image::new(&bmp, Point::new(0, 0));
    image.draw(display).expect("Draw background");

    //TODO: move these to text?
    // Styles for text
    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X9)
        .text_color(BinaryColor::Off)
        .background_color(BinaryColor::On)
        .build();

    let text_style2 = MonoTextStyleBuilder::new()
        .font(&FONT_5X7)
        .text_color(BinaryColor::On)
        .background_color(BinaryColor::Off)
        .build();

    let h1_style = MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(BinaryColor::Off)
        .background_color(BinaryColor::On)
        .build();

    // Process profile name
    let process_profile = match process {
        ProcessingProfile::Autotune => "Pitch Ctrl",
        ProcessingProfile::Vocode => "Vocode",
        ProcessingProfile::Dry => "Synth+Vox",
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
    draw_text(display, &key_buffer, Point::new(26, 3), &text_style);
    draw_text(display, &mode_buffer, Point::new(80, 3), &text_style);
    draw_centered_text(display, &note_buffer, Point::new(62, 15), h1_style);
    draw_text(display, &oct_buffer, Point::new(14, 28), &text_style);
    draw_centered_text(display, &vol_buffer, Point::new(120, 28), text_style);
    draw_centered_text(display, process_profile, Point::new(62, 28), text_style2);
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
) {
    display.clear();

    //TODO:store these so I don't have to make this each time
    let sprite_atlas = ImageRawBE::<BinaryColor>::new(
        include_bytes!("../../assets/SynthphoneE-Full-Spritesheet.raw"),
        128,
    );

    let mut vol_buffer: String<3> = String::new();
    write!(&mut vol_buffer, "{volume}").expect("Failed converting volume to string");

    //let sprite_atlas = ImageRawBE::<BinaryColor>::new(include_bytes!("./assets/SynthphoneE-Spritesheet.raw"), 65);
    let background_image =
        sprite_atlas.sub_image(&Rectangle::new(Point::new(0, 32), Size::new(128, 64)));

    //Extract sub-images from the sprite atlas
    let low_oct_on = sprite_atlas.sub_image(&Rectangle::new(Point::new(0, 64), Size::new(13, 8)));
    let med_oct_on = sprite_atlas.sub_image(&Rectangle::new(Point::new(13, 64), Size::new(13, 8)));
    let high_oct_on = sprite_atlas.sub_image(&Rectangle::new(Point::new(25, 64), Size::new(13, 8)));

    let crush_one_on = sprite_atlas.sub_image(&Rectangle::new(Point::new(0, 72), Size::new(13, 8)));
    let crush_none_on =
        sprite_atlas.sub_image(&Rectangle::new(Point::new(13, 72), Size::new(13, 8)));
    let crush_two_on =
        sprite_atlas.sub_image(&Rectangle::new(Point::new(25, 72), Size::new(13, 8)));

    let formant_male_on =
        sprite_atlas.sub_image(&Rectangle::new(Point::new(0, 80), Size::new(13, 8)));
    let formant_none_on =
        sprite_atlas.sub_image(&Rectangle::new(Point::new(13, 80), Size::new(13, 8)));
    let formant_female_on =
        sprite_atlas.sub_image(&Rectangle::new(Point::new(25, 80), Size::new(13, 8)));

    let key_down_on = sprite_atlas.sub_image(&Rectangle::new(Point::new(0, 88), Size::new(13, 8)));
    let key_up_on = sprite_atlas.sub_image(&Rectangle::new(Point::new(25, 88), Size::new(13, 8)));

    let voice_on = sprite_atlas.sub_image(&Rectangle::new(Point::new(39, 64), Size::new(13, 8)));
    let voice_off = sprite_atlas.sub_image(&Rectangle::new(Point::new(39, 72), Size::new(13, 8)));
    let vocode_on = sprite_atlas.sub_image(&Rectangle::new(Point::new(39, 80), Size::new(13, 8)));
    let vocode_off = sprite_atlas.sub_image(&Rectangle::new(Point::new(39, 88), Size::new(13, 8)));

    let pitchctrl_off =
        sprite_atlas.sub_image(&Rectangle::new(Point::new(13, 88), Size::new(13, 8)));
    let pitchctrl_on =
        sprite_atlas.sub_image(&Rectangle::new(Point::new(13, 55), Size::new(13, 8)));
    let harmony_on = sprite_atlas.sub_image(&Rectangle::new(Point::new(52, 88), Size::new(13, 8)));
    let harmony_off = sprite_atlas.sub_image(&Rectangle::new(Point::new(52, 80), Size::new(13, 8)));
    let ringer_on = sprite_atlas.sub_image(&Rectangle::new(Point::new(65, 88), Size::new(13, 8)));
    let ringer_off = sprite_atlas.sub_image(&Rectangle::new(Point::new(65, 80), Size::new(13, 8)));

    let triangle_on = sprite_atlas.sub_image(&Rectangle::new(Point::new(52, 72), Size::new(13, 8)));
    let triangle_off =
        sprite_atlas.sub_image(&Rectangle::new(Point::new(52, 64), Size::new(13, 8)));
    let square_on = sprite_atlas.sub_image(&Rectangle::new(Point::new(65, 72), Size::new(13, 8)));
    let square_off = sprite_atlas.sub_image(&Rectangle::new(Point::new(65, 64), Size::new(13, 8)));
    let saw_on = sprite_atlas.sub_image(&Rectangle::new(Point::new(77, 72), Size::new(13, 8)));
    let saw_off = sprite_atlas.sub_image(&Rectangle::new(Point::new(77, 64), Size::new(13, 8)));

    // // Convert BMPs into Image objects
    let _bg = Image::new(&background_image, Point::new(0, 0));

    let low_oct_on_img = Image::new(&low_oct_on, Point::new(0, 0));
    let med_oct_on_img = Image::new(&med_oct_on, Point::new(13, 0));
    let high_oct_on_img = Image::new(&high_oct_on, Point::new(25, 0));

    let crush_one_on_img = Image::new(&crush_one_on, Point::new(0, 8));
    let crush_none_on_img = Image::new(&crush_none_on, Point::new(13, 8));
    let crush_two_on_img = Image::new(&crush_two_on, Point::new(25, 8));

    let formant_male_on_img = Image::new(&formant_male_on, Point::new(0, 16));
    let formant_none_on_img = Image::new(&formant_none_on, Point::new(13, 16));
    let formant_female_on_img = Image::new(&formant_female_on, Point::new(25, 16));

    let triangle_on_img = Image::new(&triangle_on, Point::new(0, 16));
    let triangle_off_img = Image::new(&triangle_off, Point::new(0, 16));
    let square_on_img = Image::new(&square_on, Point::new(13, 16));
    let square_off_img = Image::new(&square_off, Point::new(13, 16));
    let saw_on_img = Image::new(&saw_on, Point::new(25, 16));
    let saw_off_img = Image::new(&saw_off, Point::new(25, 16));

    let key_down_on_img = Image::new(&key_down_on, Point::new(0, 24));
    let key_up_on_img = Image::new(&key_up_on, Point::new(25, 24));

    let voice_on_img = Image::new(&voice_on, Point::new(13, 24));
    let voice_off_img = Image::new(&voice_off, Point::new(13, 24));
    let vocode_on_img = Image::new(&vocode_on, Point::new(13, 24));
    let vocode_off_img = Image::new(&vocode_off, Point::new(13, 24));
    let pitchctrl_on_img = Image::new(&pitchctrl_on, Point::new(13, 24));
    let pitchctrl_off_img = Image::new(&pitchctrl_off, Point::new(13, 24));

    _bg.draw(display).expect("Draw background");
    //formant_male_on_img.draw(display).expect("");
    //formant_female_on_img.draw(display).expect("");

    // Row 1: Octave
    match octave {
        0 => med_oct_on_img.draw(display).expect("Draw mid octave"),
        1 => low_oct_on_img.draw(display).expect("Draw low octave"),
        4 => high_oct_on_img.draw(display).expect("Draw high octave"),
        _ => med_oct_on_img
            .draw(display)
            .expect("Draw med octave (default)"),
    }

    // Row 2: Crush
    match crush {
        0 => crush_none_on_img.draw(display).expect("Draw no crush"),
        1 => crush_one_on_img.draw(display).expect("Draw crush 1"),
        2 => crush_two_on_img.draw(display).expect("Draw crush 2"),
        _ => crush_none_on_img
            .draw(display)
            .expect("Draw no crush (default)"),
    }

    // Row 3: Formant
    if (process == ProcessingProfile::Vocode || process == ProcessingProfile::Dry) {
        match waveform {
            0 => {
                triangle_on_img.draw(display).expect("draw triangle on");
                square_off_img.draw(display).expect("Draw square off");
                saw_off_img.draw(display).expect("Draw saw off");
            }
            1 => {
                triangle_off_img.draw(display).expect("draw triangle off");
                square_on_img.draw(display).expect("Draw square on");
                saw_off_img.draw(display).expect("Draw saw off");
            }
            2 => {
                triangle_off_img.draw(display).expect("draw triangle off");
                square_off_img.draw(display).expect("Draw square off");
                saw_on_img.draw(display).expect("Draw saw on");
            }
            _ => {
                triangle_on_img.draw(display).expect("draw triangle on");
                square_off_img.draw(display).expect("Draw square off");
                saw_off_img.draw(display).expect("Draw saw off");
            }
        }
    } else {
        match formant {
            0 => formant_none_on_img.draw(display).expect("Draw no formant"),
            1 => formant_male_on_img
                .draw(display)
                .expect("Draw formant male"),
            2 => formant_female_on_img
                .draw(display)
                .expect("Draw formant female"),
            _ => formant_none_on_img
                .draw(display)
                .expect("Draw no formant (default)"),
        }
    }

    // Row 4: Key & Voice
    if key_down_pressed {
        key_down_on_img.draw(display).expect("Draw key down");
    }

    // Process profile name
    let process_profile = match process {
        ProcessingProfile::Autotune => {
            if process_cycle_pressed {
                pitchctrl_on_img.draw(display).expect("Draw autotune on");
            } else {
                pitchctrl_off_img.draw(display).expect("Draw autotune off");
            }
            "Pitch Ctrl"
        }
        ProcessingProfile::Vocode => {
            if process_cycle_pressed {
                vocode_on_img.draw(display).expect("Draw vocode on");
            } else {
                vocode_off_img.draw(display).expect("Draw vocode off");
            }
            "Vocode"
        }
        ProcessingProfile::Dry => {
            if process_cycle_pressed {
                voice_on_img.draw(display).expect("Draw voice on");
            } else {
                voice_off_img.draw(display).expect("Draw voice off");
            }
            "Dry Vox"
        }
    };

    if key_up_pressed {
        key_up_on_img.draw(display).expect("Draw key up");
    }

    // Styles for text
    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X9)
        .text_color(BinaryColor::On)
        .background_color(BinaryColor::Off)
        .build();

    let text_style2 = MonoTextStyleBuilder::new()
        .font(&FONT_6X9)
        .text_color(BinaryColor::Off)
        .background_color(BinaryColor::On)
        .build();

    let mut mode_buffer: String<8> = String::new();
    write!(
        &mut mode_buffer,
        "{} {}",
        get_key_name(key),
        get_mode_name(key)
    )
    .expect("failed converting mode to string");

    Text::with_alignment(
        &mode_buffer,
        display.bounding_box().center() + Point::new(18, 3),
        text_style,
        Alignment::Center,
    )
    .draw(display)
    .expect("Draw process text");

    Text::with_alignment(
        process_profile,
        display.bounding_box().center() + Point::new(16, 14),
        text_style,
        Alignment::Center,
    )
    .draw(display)
    .expect("Draw key text");

    draw_centered_text(display, &vol_buffer, Point::new(120, 28), text_style2);
}

pub fn draw_menu_screen(
    prev: (&str, i32),
    current: (&str, i32),
    next: (&str, i32),
    is_editing: bool,
    display: &mut LcdDisplay,
) {
    display.clear();

    // Styles for text
    let text_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X9)
        .text_color(BinaryColor::On)
        .background_color(BinaryColor::Off)
        .build();

    let h1_style = MonoTextStyleBuilder::new()
        .font(&FONT_6X13)
        .text_color(BinaryColor::On)
        .background_color(BinaryColor::Off)
        .build();

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
        let style = if i == 1 { h1_style } else { text_style };
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
