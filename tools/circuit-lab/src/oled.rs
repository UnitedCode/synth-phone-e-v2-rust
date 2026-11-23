//! SSD1306 GDDRAM and addressing subset exercised by the real 128x32 firmware.
use crate::Result;
pub struct Oled {
    pub ram: [u8; 512],
    pub writes: u64,
    pub data_bytes: u64,
    pub enabled: bool,
    mode: u8,
    column: u8,
    page: u8,
    col_range: [u8; 2],
    page_range: [u8; 2],
    pending: Option<(u8, usize, Vec<u8>)>,
}
impl Default for Oled {
    fn default() -> Self {
        Self {
            ram: [0; 512],
            writes: 0,
            data_bytes: 0,
            enabled: false,
            mode: 0,
            column: 0,
            page: 0,
            col_range: [0, 127],
            page_range: [0, 3],
            pending: None,
        }
    }
}
impl Oled {
    /// Match documented effect-page text against the actual GDDRAM, without inspecting firmware state.
    #[cfg(feature = "device")]
    pub fn has_profile_label(ram: &[u8; 512], label: &str) -> bool {
        use embedded_graphics::{
            mono_font::{ascii::FONT_6X9, MonoTextStyleBuilder},
            pixelcolor::BinaryColor,
            prelude::*,
            text::{Alignment, Text},
        };
        struct Match<'a> {
            ram: &'a [u8; 512],
            matches: bool,
            pixels: usize,
        }
        impl OriginDimensions for Match<'_> {
            fn size(&self) -> Size {
                Size::new(128, 32)
            }
        }
        impl DrawTarget for Match<'_> {
            type Color = BinaryColor;
            type Error = core::convert::Infallible;
            fn draw_iter<I>(&mut self, pixels: I) -> std::result::Result<(), Self::Error>
            where
                I: IntoIterator<Item = Pixel<BinaryColor>>,
            {
                for Pixel(p, color) in pixels {
                    self.pixels += 1;
                    if !(0..128).contains(&p.x) || !(0..32).contains(&p.y) {
                        self.matches = false;
                        continue;
                    }
                    let on =
                        self.ram[p.y as usize / 8 * 128 + p.x as usize] & (1 << (p.y % 8)) != 0;
                    if on != (color == BinaryColor::On) {
                        self.matches = false;
                    }
                }
                Ok(())
            }
        }
        let mut target = Match {
            ram,
            matches: true,
            pixels: 0,
        };
        let style = MonoTextStyleBuilder::new()
            .font(&FONT_6X9)
            .text_color(BinaryColor::On)
            .background_color(BinaryColor::Off)
            .build();
        let _ = Text::with_alignment(label, Point::new(79, 29), style, Alignment::Center)
            .draw(&mut target);
        target.matches && target.pixels > 0
    }
    pub fn write(&mut self, packet: &[u8]) -> Result<()> {
        let (&control, payload) = packet.split_first().ok_or("empty OLED write")?;
        if control != 0x00 && control != 0x40 {
            return Err("unsupported OLED control framing".into());
        }
        self.writes += 1;
        for &value in payload {
            if control == 0x40 {
                self.data(value)?;
            } else {
                self.command(value)?;
            }
        }
        Ok(())
    }
    fn command(&mut self, value: u8) -> Result<()> {
        if let Some((cmd, left, mut parameters)) = self.pending.take() {
            parameters.push(value);
            if left > 1 {
                self.pending = Some((cmd, left - 1, parameters));
                return Ok(());
            }
            match cmd {
                0x20 => {
                    if parameters[0] > 2 {
                        return Err("invalid OLED address mode".into());
                    }
                    self.mode = parameters[0];
                }
                0x21 => {
                    if parameters[0] > parameters[1] || parameters[1] > 127 {
                        return Err("OLED column range".into());
                    }
                    self.col_range = [parameters[0], parameters[1]];
                    self.column = parameters[0];
                }
                0x22 => {
                    if parameters[0] > parameters[1] || parameters[1] > 3 {
                        return Err("OLED page range".into());
                    }
                    self.page_range = [parameters[0], parameters[1]];
                    self.page = parameters[0];
                }
                _ => {}
            }
            return Ok(());
        }
        match value {
            0x20 | 0x81 | 0xa8 | 0xd3 | 0xd5 | 0xd9 | 0xda | 0xdb | 0x8d => {
                self.pending = Some((value, 1, Vec::new()))
            }
            0x21 | 0x22 => self.pending = Some((value, 2, Vec::new())),
            0xae => self.enabled = false,
            0xaf => self.enabled = true,
            0x00..=0x0f => self.column = (self.column & 0xf0) | value,
            0x10..=0x1f => self.column = (self.column & 0x0f) | ((value & 0xf) << 4),
            0xb0..=0xb3 => self.page = value & 3,
            0x40..=0x7f | 0xa0 | 0xa1 | 0xa4..=0xa7 | 0xc0 | 0xc8 | 0xe3 | 0x2e => {}
            _ => return Err(format!("unmodeled OLED command {value:#04x}").into()),
        }
        Ok(())
    }
    fn data(&mut self, value: u8) -> Result<()> {
        if self.column > 127 || self.page > 3 {
            return Err("OLED write outside GDDRAM".into());
        }
        self.ram[self.page as usize * 128 + self.column as usize] = value;
        self.data_bytes += 1;
        match self.mode {
            0 => {
                self.column += 1;
                if self.column > self.col_range[1] {
                    self.column = self.col_range[0];
                    self.page += 1;
                    if self.page > self.page_range[1] {
                        self.page = self.page_range[0];
                    }
                }
            }
            1 => {
                self.page += 1;
                if self.page > self.page_range[1] {
                    self.page = self.page_range[0];
                    self.column += 1;
                    if self.column > self.col_range[1] {
                        self.column = self.col_range[0];
                    }
                }
            }
            2 => self.column = (self.column + 1) & 127,
            _ => unreachable!(),
        }
        Ok(())
    }
    pub fn pbm(&self) -> String {
        let mut out = String::from("P1\n128 32\n");
        for y in 0..32 {
            for x in 0..128 {
                out.push(if self.ram[y / 8 * 128 + x] & (1 << (y % 8)) != 0 {
                    '1'
                } else {
                    '0'
                });
                out.push(' ');
            }
            out.push('\n');
        }
        out
    }
    pub fn svg(&self) -> String {
        let mut out=String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 128 32\" width=\"768\" height=\"192\" shape-rendering=\"crispEdges\"><rect width=\"128\" height=\"32\" fill=\"#09121c\"/><g fill=\"#a8f2ee\">\n");
        for y in 0..32 {
            for x in 0..128 {
                if self.ram[y / 8 * 128 + x] & (1 << (y % 8)) != 0 {
                    out.push_str(&format!(
                        "<rect x=\"{x}\" y=\"{y}\" width=\"1\" height=\"1\"/>\n"
                    ));
                }
            }
        }
        out.push_str("</g></svg>\n");
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_addressing_writes_pixels_and_wraps() {
        let mut oled = Oled::default();
        oled.write(&[0, 0x20, 0, 0x21, 0, 1, 0x22, 0, 1, 0xaf])
            .unwrap();
        oled.write(&[0x40, 1, 2, 4, 8, 16]).unwrap();
        assert!(oled.enabled);
        assert_eq!(
            (oled.ram[0], oled.ram[1], oled.ram[128], oled.ram[129]),
            (16, 2, 4, 8)
        );
        assert!(oled.write(&[0, 0xff]).is_err());
    }
}
