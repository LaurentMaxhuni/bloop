//! bloop's icon as RGBA pixels, decoded once from the embedded PNG.

use std::sync::OnceLock;

pub const PNG: &[u8] = include_bytes!("../../../installer/linux/bloop.png");

/// `(rgba, width, height)`, or `None` if the PNG can't be decoded.
pub fn rgba() -> Option<&'static (Vec<u8>, u32, u32)> {
    static PIXELS: OnceLock<Option<(Vec<u8>, u32, u32)>> = OnceLock::new();
    PIXELS.get_or_init(|| decode(PNG)).as_ref()
}

fn decode(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    buffer.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buffer,
        png::ColorType::Rgb => buffer
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        _ => return None,
    };
    Some((rgba, info.width, info.height))
}

/// The window icon for Iced window settings.
pub fn window_icon() -> Option<iced::window::Icon> {
    let (rgba, width, height) = rgba()?;
    iced::window::icon::from_rgba(rgba.clone(), *width, *height).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_icon_decodes_to_256_square() {
        let (rgba, width, height) = super::rgba().unwrap();
        assert_eq!((*width, *height), (256, 256));
        assert_eq!(rgba.len(), 256 * 256 * 4);
    }
}
