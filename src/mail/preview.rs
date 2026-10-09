//! Previews of attachments: thumbnails for images, PDF renditions of Office documents.
//! Attachments are untrusted, so nothing here passes the original bytes through as a picture.

use std::{
    io::Cursor,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use image::{ImageFormat, ImageReader, Limits, Rgb, RgbImage, codecs::jpeg::JpegEncoder};

use crate::crypto;

const THUMBNAIL_SIDE: u32 = 480;
const MAX_IMAGE_SIDE: u32 = 12_000;
const MAX_DECODE_BYTES: u64 = 256 * 1024 * 1024;
const CONVERT_TIMEOUT: Duration = Duration::from_secs(90);

const OFFICE_EXTENSIONS: [&str; 10] = ["doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "ods", "odp", "rtf"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Image,
    Pdf,
    Office,
    Other,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Image => "image",
            Kind::Pdf => "pdf",
            Kind::Office => "office",
            Kind::Other => "other",
        }
    }
}

pub fn extension(filename: &str) -> String {
    filename
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_lowercase())
        .unwrap_or_default()
}

/// What kind of preview an attachment can get. Mail clients often send a generic MIME type, so
/// the file extension decides when the type does not.
pub fn kind(mime: &str, filename: &str) -> Kind {
    let mime = mime.to_lowercase();
    let ext = extension(filename);
    if matches!(mime.as_str(), "image/png" | "image/jpeg" | "image/gif" | "image/webp")
        || matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp")
    {
        Kind::Image
    } else if mime == "application/pdf" || ext == "pdf" {
        Kind::Pdf
    } else if OFFICE_EXTENSIONS.contains(&ext.as_str()) {
        Kind::Office
    } else {
        Kind::Other
    }
}

/// MIME type of a raster image this server is willing to show inline, judged by its content.
pub fn raster_mime(bytes: &[u8]) -> Option<&'static str> {
    match image::guess_format(bytes).ok()? {
        ImageFormat::Png => Some("image/png"),
        ImageFormat::Jpeg => Some("image/jpeg"),
        ImageFormat::Gif => Some("image/gif"),
        ImageFormat::WebP => Some("image/webp"),
        _ => None,
    }
}

/// Decodes an image and returns a JPEG no larger than 480 px on its longest side.
/// CPU-bound: call through `spawn_blocking`.
pub fn thumbnail(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .context("unreadable image")?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let small = reader
        .decode()
        .context("image cannot be decoded")?
        .thumbnail(THUMBNAIL_SIDE, THUMBNAIL_SIDE)
        .to_rgba8();

    // JPEG has no transparency: put the picture on white.
    let mut flat = RgbImage::new(small.width(), small.height());
    for (x, y, pixel) in small.enumerate_pixels() {
        let alpha = pixel[3] as u32;
        let blend = |channel: u8| ((channel as u32 * alpha + 255 * (255 - alpha)) / 255) as u8;
        flat.put_pixel(x, y, Rgb([blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]));
    }
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, 82).encode_image(&flat)?;
    Ok(out)
}

/// Finds LibreOffice: `PORTIER_SOFFICE` (a path, or `off`), then PATH, then the macOS app bundle.
pub fn find_soffice() -> Option<PathBuf> {
    if let Some(configured) = crate::config::setting("SOFFICE") {
        if configured == "off" {
            return None;
        }
        let path = PathBuf::from(configured);
        return path.is_file().then_some(path);
    }
    let on_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .flat_map(|dir| [dir.join("soffice"), dir.join("libreoffice")])
            .find(|candidate| candidate.is_file())
    });
    on_path.or_else(|| {
        let bundled = PathBuf::from("/Applications/LibreOffice.app/Contents/MacOS/soffice");
        bundled.is_file().then_some(bundled)
    })
}

/// Converts an Office document to PDF with a headless LibreOffice. The document is written under
/// a generated name into a throwaway directory that also holds LibreOffice's profile, so nothing
/// of the sender's choosing reaches the command line and no state survives the run.
pub async fn office_to_pdf(soffice: &Path, work_root: &Path, bytes: &[u8], filename: &str) -> Result<Vec<u8>> {
    let ext = extension(filename);
    if !OFFICE_EXTENSIONS.contains(&ext.as_str()) {
        bail!("not an Office document");
    }
    let dir = work_root.join(format!("convert-{}", crypto::random_token()));
    let profile = dir.join("profile");
    tokio::fs::create_dir_all(&profile).await?;
    let result = convert_in(soffice, &dir, &profile, bytes, &ext).await;
    let _ = tokio::fs::remove_dir_all(&dir).await;
    result
}

async fn convert_in(soffice: &Path, dir: &Path, profile: &Path, bytes: &[u8], ext: &str) -> Result<Vec<u8>> {
    let input = dir.join(format!("input.{ext}"));
    tokio::fs::write(&input, bytes).await?;
    let mut command = tokio::process::Command::new(soffice);
    // The document came from a stranger. The converter runs with an empty environment, so that
    // nothing this server holds — the master key above all — is there to be read if the document
    // finds a way out of it. Only what a converter needs to find its own files is put back.
    command.env_clear();
    for name in ["PATH", "HOME", "TMPDIR", "LANG", "LC_ALL"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args([
            "--headless",
            "--norestore",
            "--nolockcheck",
            "--convert-to",
            "pdf",
            "--outdir",
        ])
        .arg(dir)
        .arg(&input)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(CONVERT_TIMEOUT, command.status())
        .await
        .map_err(|_| anyhow!("LibreOffice took too long"))?
        .context("LibreOffice could not be started")?;
    if !status.success() {
        bail!("LibreOffice failed with {status}");
    }
    tokio::fs::read(dir.join("input.pdf"))
        .await
        .context("LibreOffice produced no PDF")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(width, height, image::Rgba([200, 30, 30, 128]));
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn kinds() {
        assert_eq!(kind("image/png", "a"), Kind::Image);
        assert_eq!(kind("application/octet-stream", "Scan.JPG"), Kind::Image);
        assert_eq!(kind("image/svg+xml", "logo.svg"), Kind::Other);
        assert_eq!(kind("application/pdf", "x"), Kind::Pdf);
        assert_eq!(kind("application/octet-stream", "invoice.pdf"), Kind::Pdf);
        assert_eq!(
            kind(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "a.docx"
            ),
            Kind::Office
        );
        assert_eq!(kind("application/octet-stream", "budget.xlsx"), Kind::Office);
        assert_eq!(kind("application/zip", "all.zip"), Kind::Other);
        assert_eq!(kind("text/html", "page.html"), Kind::Other);
    }

    #[test]
    fn thumbnail_is_small_jpeg() {
        let thumb = thumbnail(&png(1600, 800)).unwrap();
        assert_eq!(image::guess_format(&thumb).unwrap(), ImageFormat::Jpeg);
        let decoded = image::load_from_memory(&thumb).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (480, 240));
    }

    #[test]
    fn thumbnail_rejects_garbage_and_oversize() {
        assert!(thumbnail(b"this is not an image").is_err());
        assert!(thumbnail(&png(MAX_IMAGE_SIDE + 1, 1)).is_err());
    }

    #[test]
    fn inline_images_are_judged_by_content() {
        assert_eq!(raster_mime(&png(2, 2)), Some("image/png"));
        assert_eq!(raster_mime(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"), None);
    }
}
