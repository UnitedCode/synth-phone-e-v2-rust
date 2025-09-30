use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    mono_font::{
        ascii::{FONT_10X20, FONT_5X7, FONT_6X13, FONT_6X9},
        MonoTextStyle, MonoTextStyleBuilder,
    },
    pixelcolor::BinaryColor,
    prelude::Dimensions,
    text::{Baseline, Text},
    Drawable,
};

// Style creation functions - called once and reused
pub fn normal_text() -> MonoTextStyle<'static, BinaryColor> {
    MonoTextStyleBuilder::new()
        .font(&FONT_6X9)
        .text_color(BinaryColor::On)
        .background_color(BinaryColor::Off)
        .build()
}

pub fn inverted_text() -> MonoTextStyle<'static, BinaryColor> {
    MonoTextStyleBuilder::new()
        .font(&FONT_6X9)
        .text_color(BinaryColor::Off)
        .background_color(BinaryColor::On)
        .build()
}

pub fn small_text() -> MonoTextStyle<'static, BinaryColor> {
    MonoTextStyleBuilder::new()
        .font(&FONT_5X7)
        .text_color(BinaryColor::On)
        .background_color(BinaryColor::Off)
        .build()
}

pub fn header_text() -> MonoTextStyle<'static, BinaryColor> {
    MonoTextStyleBuilder::new()
        .font(&FONT_10X20)
        .text_color(BinaryColor::Off)
        .background_color(BinaryColor::On)
        .build()
}

pub fn menu_normal() -> MonoTextStyle<'static, BinaryColor> {
    MonoTextStyleBuilder::new()
        .font(&FONT_6X9)
        .text_color(BinaryColor::On)
        .background_color(BinaryColor::Off)
        .build()
}

pub fn menu_highlight() -> MonoTextStyle<'static, BinaryColor> {
    MonoTextStyleBuilder::new()
        .font(&FONT_6X13)
        .text_color(BinaryColor::On)
        .background_color(BinaryColor::Off)
        .build()
}

// Text drawing functions
pub fn draw_text<D>(
    display: &mut D,
    text: &str,
    position: Point,
    style: &MonoTextStyle<BinaryColor>,
) where
    D: DrawTarget<Color = BinaryColor>,
{
    let _ = Text::with_baseline(text, position, *style, Baseline::Middle).draw(display);
}

pub fn draw_centered_text<D>(
    display: &mut D,
    text: &str,
    center: Point,
    style: MonoTextStyle<BinaryColor>,
) where
    D: DrawTarget<Color = BinaryColor>,
{
    // 1) Create a Text at (0,0) just to measure it
    let text_obj = Text::with_baseline(text, Point::zero(), style, Baseline::Top);

    // 2) bounding_box() gives us the width/height of this text
    let bbox = text_obj.bounding_box();
    let text_width = bbox.size.width as i32;
    let text_height = bbox.size.height as i32;

    // 3) Compute a new top-left so that the text is centered on `center`
    let draw_x = center.x - text_width / 2;
    let draw_y = center.y - text_height / 2;

    // 4) Draw the text at the adjusted position
    let _ =
        Text::with_baseline(text, Point::new(draw_x, draw_y), style, Baseline::Top).draw(display);
}
