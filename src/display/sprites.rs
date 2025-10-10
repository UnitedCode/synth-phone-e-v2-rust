use embedded_graphics::{
    image::{Image, ImageRawBE},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::Rectangle,
};

// Sprite position and source rectangle data
#[derive(Copy, Clone)]
pub struct SpriteInfo {
    pub source_rect: Rectangle,
    pub draw_position: Point,
}

// All sprite data as const - this is safe and works at compile time
pub const SPRITE_DATA: SpriteData = SpriteData::new();

pub struct SpriteData {
    // Background
    pub effects_background: SpriteInfo,
    pub processing_background: SpriteInfo,
    pub splash: SpriteInfo,

    // Octave controls (row 1)
    pub low_oct: SpriteInfo,
    pub med_oct: SpriteInfo,
    pub high_oct: SpriteInfo,

    // Crush controls (row 2)
    pub crush_one: SpriteInfo,
    pub crush_none: SpriteInfo,
    pub crush_two: SpriteInfo,

    // Formant controls (row 3)
    pub formant_male: SpriteInfo,
    pub formant_none: SpriteInfo,
    pub formant_female: SpriteInfo,

    // Waveform controls (row 3, alternative)
    pub triangle_on: SpriteInfo,
    pub triangle_off: SpriteInfo,
    pub square_on: SpriteInfo,
    pub square_off: SpriteInfo,
    pub saw_on: SpriteInfo,
    pub saw_off: SpriteInfo,

    // Key controls (row 4)
    pub key_down: SpriteInfo,
    pub key_up: SpriteInfo,

    // Process indicators (center of row 4)
    pub pitchctrl_on: SpriteInfo,
    pub pitchctrl_off: SpriteInfo,
    pub vocode_on: SpriteInfo,
    pub vocode_off: SpriteInfo,
    pub voice_on: SpriteInfo,
    pub voice_off: SpriteInfo,
    pub harmony_on: SpriteInfo,
    pub harmony_off: SpriteInfo,
    pub phone_on: SpriteInfo,
    pub phone_off: SpriteInfo,
}

impl SpriteData {
    pub const fn new() -> Self {
        Self {
            splash: SpriteInfo {
                source_rect: Rectangle::new(Point::new(0, 0), Size::new(128, 32)),
                draw_position: Point::new(0, 0),
            },

            effects_background: SpriteInfo {
                source_rect: Rectangle::new(Point::new(0, 32), Size::new(128, 32)),
                draw_position: Point::new(0, 0),
            },

            processing_background: SpriteInfo {
                source_rect: Rectangle::new(Point::new(0, 96), Size::new(128, 32)),
                draw_position: Point::new(0, 0),
            },

            // Octave controls
            low_oct: SpriteInfo {
                source_rect: Rectangle::new(Point::new(0, 64), Size::new(13, 8)),
                draw_position: Point::new(0, 0),
            },
            med_oct: SpriteInfo {
                source_rect: Rectangle::new(Point::new(13, 64), Size::new(13, 8)),
                draw_position: Point::new(13, 0),
            },
            high_oct: SpriteInfo {
                source_rect: Rectangle::new(Point::new(25, 64), Size::new(13, 8)),
                draw_position: Point::new(25, 0),
            },

            // Crush controls
            crush_one: SpriteInfo {
                source_rect: Rectangle::new(Point::new(0, 72), Size::new(13, 8)),
                draw_position: Point::new(0, 8),
            },
            crush_none: SpriteInfo {
                source_rect: Rectangle::new(Point::new(13, 72), Size::new(13, 8)),
                draw_position: Point::new(13, 8),
            },
            crush_two: SpriteInfo {
                source_rect: Rectangle::new(Point::new(25, 72), Size::new(13, 8)),
                draw_position: Point::new(25, 8),
            },

            // Formant controls
            formant_male: SpriteInfo {
                source_rect: Rectangle::new(Point::new(0, 80), Size::new(13, 8)),
                draw_position: Point::new(0, 16),
            },
            formant_none: SpriteInfo {
                source_rect: Rectangle::new(Point::new(13, 80), Size::new(13, 8)),
                draw_position: Point::new(13, 16),
            },
            formant_female: SpriteInfo {
                source_rect: Rectangle::new(Point::new(25, 80), Size::new(13, 8)),
                draw_position: Point::new(25, 16),
            },

            // Waveform controls (same positions as formant)
            triangle_on: SpriteInfo {
                source_rect: Rectangle::new(Point::new(52, 72), Size::new(13, 8)),
                draw_position: Point::new(0, 16),
            },
            triangle_off: SpriteInfo {
                source_rect: Rectangle::new(Point::new(52, 64), Size::new(13, 8)),
                draw_position: Point::new(0, 16),
            },
            square_on: SpriteInfo {
                source_rect: Rectangle::new(Point::new(65, 72), Size::new(13, 8)),
                draw_position: Point::new(13, 16),
            },
            square_off: SpriteInfo {
                source_rect: Rectangle::new(Point::new(65, 64), Size::new(13, 8)),
                draw_position: Point::new(13, 16),
            },
            saw_on: SpriteInfo {
                source_rect: Rectangle::new(Point::new(77, 72), Size::new(13, 8)),
                draw_position: Point::new(25, 16),
            },
            saw_off: SpriteInfo {
                source_rect: Rectangle::new(Point::new(77, 64), Size::new(13, 8)),
                draw_position: Point::new(25, 16),
            },

            // Key controls
            key_down: SpriteInfo {
                source_rect: Rectangle::new(Point::new(0, 88), Size::new(13, 8)),
                draw_position: Point::new(0, 24),
            },
            key_up: SpriteInfo {
                source_rect: Rectangle::new(Point::new(25, 88), Size::new(13, 8)),
                draw_position: Point::new(25, 24),
            },

            // Process controls
            pitchctrl_on: SpriteInfo {
                source_rect: Rectangle::new(Point::new(13, 55), Size::new(13, 8)),
                draw_position: Point::new(13, 24),
            },
            pitchctrl_off: SpriteInfo {
                source_rect: Rectangle::new(Point::new(13, 88), Size::new(13, 8)),
                draw_position: Point::new(13, 24),
            },
            vocode_on: SpriteInfo {
                source_rect: Rectangle::new(Point::new(39, 80), Size::new(13, 8)),
                draw_position: Point::new(13, 24),
            },
            vocode_off: SpriteInfo {
                source_rect: Rectangle::new(Point::new(39, 88), Size::new(13, 8)),
                draw_position: Point::new(13, 24),
            },
            voice_on: SpriteInfo {
                source_rect: Rectangle::new(Point::new(39, 64), Size::new(13, 8)),
                draw_position: Point::new(13, 24),
            },
            voice_off: SpriteInfo {
                source_rect: Rectangle::new(Point::new(39, 72), Size::new(13, 8)),
                draw_position: Point::new(13, 24),
            },
            harmony_on: SpriteInfo {
                source_rect: Rectangle::new(Point::new(52, 80), Size::new(13, 8)),
                draw_position: Point::new(13, 24),
            },
            harmony_off: SpriteInfo {
                source_rect: Rectangle::new(Point::new(52, 88), Size::new(13, 8)),
                draw_position: Point::new(13, 24),
            },
            phone_on: SpriteInfo {
                source_rect: Rectangle::new(Point::new(65, 80), Size::new(13, 8)),
                draw_position: Point::new(13, 24),
            },
            phone_off: SpriteInfo {
                source_rect: Rectangle::new(Point::new(65, 88), Size::new(13, 8)),
                draw_position: Point::new(13, 24),
            },
        }
    }
}

// Helper functions that create the atlas and draw sprites
// This approach creates the ImageRaw each time, but it's still much better than your original
pub fn draw_sprite(
    display: &mut crate::types::LcdDisplay,
    atlas: &ImageRawBE<BinaryColor>,
    sprite_info: &SpriteInfo,
) {
    let sub_image = atlas.sub_image(&sprite_info.source_rect);
    let image = Image::new(&sub_image, sprite_info.draw_position);
    let _ = image.draw(display);
}

pub fn draw_effects_bg(display: &mut crate::types::LcdDisplay, atlas: &ImageRawBE<BinaryColor>) {
    draw_sprite(display, atlas, &SPRITE_DATA.effects_background);
}

pub fn draw_processing_bg(display: &mut crate::types::LcdDisplay, atlas: &ImageRawBE<BinaryColor>) {
    draw_sprite(display, atlas, &SPRITE_DATA.processing_background);
}

pub fn draw_splash(display: &mut crate::types::LcdDisplay, atlas: &ImageRawBE<BinaryColor>) {
    draw_sprite(display, atlas, &SPRITE_DATA.splash);
}

pub fn draw_octave(
    display: &mut crate::types::LcdDisplay,
    atlas: &ImageRawBE<BinaryColor>,
    octave: i32,
) {
    let sprite = match octave {
        1 => &SPRITE_DATA.low_oct,
        2 => &SPRITE_DATA.med_oct,
        4 => &SPRITE_DATA.high_oct,
        _ => &SPRITE_DATA.med_oct,
    };
    draw_sprite(display, atlas, sprite);
}

pub fn draw_crush(
    display: &mut crate::types::LcdDisplay,
    atlas: &ImageRawBE<BinaryColor>,
    crush: i32,
) {
    let sprite = match crush {
        1 => &SPRITE_DATA.crush_one,
        0 => &SPRITE_DATA.crush_none,
        2 => &SPRITE_DATA.crush_two,
        _ => &SPRITE_DATA.crush_none,
    };
    draw_sprite(display, atlas, sprite);
}

pub fn draw_formant(
    display: &mut crate::types::LcdDisplay,
    atlas: &ImageRawBE<BinaryColor>,
    formant: i32,
) {
    let sprite = match formant {
        1 => &SPRITE_DATA.formant_male,
        0 => &SPRITE_DATA.formant_none,
        2 => &SPRITE_DATA.formant_female,
        _ => &SPRITE_DATA.formant_none,
    };
    draw_sprite(display, atlas, sprite);
}

pub fn draw_waveform(
    display: &mut crate::types::LcdDisplay,
    atlas: &ImageRawBE<BinaryColor>,
    waveform: i32,
) {
    for i in 0..3 {
        let sprite = match i {
            0 => {
                if waveform == 0 {
                    &SPRITE_DATA.triangle_on
                } else {
                    &SPRITE_DATA.triangle_off
                }
            }
            1 => {
                if waveform == 1 {
                    &SPRITE_DATA.square_on
                } else {
                    &SPRITE_DATA.square_off
                }
            }
            2 => {
                if waveform == 2 {
                    &SPRITE_DATA.saw_on
                } else {
                    &SPRITE_DATA.saw_off
                }
            }
            _ => continue,
        };
        draw_sprite(display, atlas, sprite);
    }
}

pub fn draw_process_indicator(
    display: &mut crate::types::LcdDisplay,
    atlas: &ImageRawBE<BinaryColor>,
    process: crate::state_machine::ProcessingProfile,
    is_pressed: bool,
) {
    use crate::state_machine::ProcessingProfile;
    let sprite = match (process, is_pressed) {
        (ProcessingProfile::PitchControl, true) => &SPRITE_DATA.pitchctrl_on,
        (ProcessingProfile::PitchControl, false) => &SPRITE_DATA.pitchctrl_off,
        (ProcessingProfile::Vocode, true) => &SPRITE_DATA.vocode_on,
        (ProcessingProfile::Vocode, false) => &SPRITE_DATA.vocode_off,
        (ProcessingProfile::Dry, true) => &SPRITE_DATA.voice_on,
        (ProcessingProfile::Dry, false) => &SPRITE_DATA.voice_off,
        (ProcessingProfile::Harmony, true) => &SPRITE_DATA.harmony_on,
        (ProcessingProfile::Harmony, false) => &SPRITE_DATA.harmony_off,
        (ProcessingProfile::Phone, true) => &SPRITE_DATA.phone_on,
        (ProcessingProfile::Phone, false) => &SPRITE_DATA.phone_off,
    };
    draw_sprite(display, atlas, sprite);
}

pub fn draw_key_controls(
    display: &mut crate::types::LcdDisplay,
    atlas: &ImageRawBE<BinaryColor>,
    key_down: bool,
    key_up: bool,
) {
    if key_down {
        draw_sprite(display, atlas, &SPRITE_DATA.key_down);
    }
    if key_up {
        draw_sprite(display, atlas, &SPRITE_DATA.key_up);
    }
}
