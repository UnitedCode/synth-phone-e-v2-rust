use crate::types::LcdDisplay;
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::Point,
    mono_font::MonoTextStyle,
    pixelcolor::BinaryColor,
    prelude::Dimensions,
    text::{Baseline, Text},
    Drawable,
};

pub fn draw_text<D>(
    display: &mut D,
    text: &str,
    position: Point,
    style: &MonoTextStyle<BinaryColor>,
) where
    D: DrawTarget<Color = BinaryColor>,
{
    Text::with_baseline(text, position, *style, Baseline::Middle).draw(display);
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
    Text::with_baseline(text, Point::new(draw_x, draw_y), style, Baseline::Top).draw(display);
}
