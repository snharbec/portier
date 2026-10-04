//! Pictures in the terminal: the sender's picture on a mail, and image attachments.
//!
//! Only terminals with a graphics protocol (kitty, iTerm2, sixel) show them. Others get
//! none, unless `PORTIER_TUI_IMAGES=blocks` asks for a coarse rendition from block characters.
//! `PORTIER_TUI_IMAGES=on` tries the protocols in a terminal this program does not know,
//! `off` never shows pictures.

use ratatui::layout::{Rect, Size};
use ratatui_image::{
    Resize,
    picker::{Picker, ProtocolType},
    protocol::Protocol,
};

/// Terminals known to draw pictures, by what they put into the environment.
fn draws_pictures() -> bool {
    let set = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
    let program = std::env::var("TERM_PROGRAM").unwrap_or_default().to_lowercase();
    let term = std::env::var("TERM").unwrap_or_default().to_lowercase();
    set("KITTY_WINDOW_ID")
        || set("WEZTERM_EXECUTABLE")
        || set("GHOSTTY_RESOURCES_DIR")
        || set("KONSOLE_VERSION")
        || ["iterm.app", "wezterm", "ghostty"].contains(&program.as_str())
        || term.contains("kitty")
        || term.contains("ghostty")
        || term.starts_with("foot")
}

/// How pictures are drawn, or `None` when they are not.
///
/// `ask` may question the terminal about its picture protocol, which only works once it is
/// in raw mode. The question is only put to terminals known to draw pictures (or when
/// `PORTIER_TUI_IMAGES=on` insists): a terminal that never answers would leave the asking
/// part waiting on the keyboard, where it would swallow the reader's keys.
pub fn picker(ask: bool) -> Option<Picker> {
    let wanted = std::env::var("PORTIER_TUI_IMAGES").unwrap_or_default();
    match wanted.as_str() {
        "off" => None,
        "blocks" => Some(Picker::halfblocks()),
        "on" | "" if ask && (wanted == "on" || draws_pictures()) => Picker::from_query_stdio()
            .ok()
            // Block characters are no picture; they are only used when asked for.
            .filter(|picker| picker.protocol_type() != ProtocolType::Halfblocks),
        _ => None,
    }
}

/// A decoded picture and its rendition for the size it was last drawn in.
pub struct Picture {
    image: image::DynamicImage,
    drawn: Option<(Size, Protocol)>,
}

impl Picture {
    /// `None` when the bytes are no image this program reads.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .ok()?;
        // A picture that claims an enormous size is not decoded.
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(12_000);
        limits.max_image_height = Some(12_000);
        limits.max_alloc = Some(256 * 1024 * 1024);
        reader.limits(limits);
        Some(Self {
            image: reader.decode().ok()?,
            drawn: None,
        })
    }

    /// The picture fitted into `area`, made anew only when the area's size changed.
    pub fn fitted(&mut self, picker: &Picker, area: Rect) -> Option<&Protocol> {
        let size = Size::new(area.width, area.height);
        if size.width == 0 || size.height == 0 {
            return None;
        }
        if self.drawn.as_ref().is_none_or(|(drawn, _)| *drawn != size) {
            let protocol = picker.new_protocol(self.image.clone(), size, Resize::Fit(None)).ok()?;
            self.drawn = Some((size, protocol));
        }
        self.drawn.as_ref().map(|(_, protocol)| protocol)
    }
}

/// A file name as it came with a mail, made safe to use as a name in a folder of this
/// computer: no path, nothing hidden, nothing empty.
pub fn safe_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = last.chars().filter(|c| !c.is_control()).collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() {
        "attachment".to_string()
    } else {
        cleaned
    }
}

/// `dir/name`, or `dir/name (2).ext` and so on when that exists already.
pub fn free_path(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
        _ => (name, String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){extension}")))
        .find(|path| !path.exists())
        .expect("a free name")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_from_mail_cannot_leave_the_folder() {
        assert_eq!(safe_name("report.pdf"), "report.pdf");
        assert_eq!(safe_name("../../etc/passwd"), "passwd");
        assert_eq!(safe_name("C:\\Users\\x\\evil.exe"), "evil.exe");
        assert_eq!(safe_name(".bashrc"), "bashrc");
        assert_eq!(safe_name("a\u{0}b\n.txt"), "ab.txt");
        assert_eq!(safe_name(".."), "attachment");
        assert_eq!(safe_name(""), "attachment");
    }

    #[test]
    fn saving_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("portier-tui-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_path(&dir, "a.txt"), dir.join("a.txt"));
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        assert_eq!(free_path(&dir, "a.txt"), dir.join("a (2).txt"));
        std::fs::write(dir.join("a (2).txt"), "x").unwrap();
        assert_eq!(free_path(&dir, "a.txt"), dir.join("a (3).txt"));
        std::fs::write(dir.join("noext"), "x").unwrap();
        assert_eq!(free_path(&dir, "noext"), dir.join("noext (2)"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_picture_is_decoded_and_fitted_once_per_size() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(40, 20, image::Rgba([200, 30, 30, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let mut picture = Picture::decode(&png.into_inner()).unwrap();
        let picker = Picker::halfblocks();
        let area = Rect::new(0, 0, 8, 4);
        let protocol = picture.fitted(&picker, area).unwrap();
        // Drawn with block characters, it colours cells (a plain red picture: red cells).
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        ratatui::widgets::Widget::render(ratatui_image::Image::new(protocol), area, &mut buffer);
        let coloured = buffer
            .content()
            .iter()
            .filter(|cell| cell.bg != ratatui::style::Color::Reset || cell.fg != ratatui::style::Color::Reset)
            .count();
        assert!(coloured > 0, "nothing drawn");
        assert!(picture.fitted(&picker, Rect::new(0, 0, 0, 4)).is_none());
        assert!(Picture::decode(b"not a picture").is_none());
    }
}
