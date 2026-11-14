use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Jpeg,
    Png,
    WebP,
    Gif,
    Bmp,
    Tiff,
}

impl ImageFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Jpeg => "jpeg",
            Self::Png => "png",
            Self::WebP => "webp",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
            Self::Tiff => "tiff",
        }
    }

    pub fn mime_type(&self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::WebP => "image/webp",
            Self::Gif => "image/gif",
            Self::Bmp => "image/bmp",
            Self::Tiff => "image/tiff",
        }
    }
}

pub fn sniff_format(bytes: &[u8]) -> Result<ImageFormat> {
    if bytes.len() < 2 {
        return Err(CoreError::UnsupportedFormat(
            "file is too small to determine format".to_string(),
        ));
    }

    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Ok(ImageFormat::Jpeg);
    }

    if bytes.len() >= 8 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(ImageFormat::Png);
    }

    if bytes.len() >= 6 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return Ok(ImageFormat::Gif);
    }

    if bytes.len() >= 2 && bytes.starts_with(b"BM") {
        return Ok(ImageFormat::Bmp);
    }

    if bytes.len() >= 4 && (bytes.starts_with(b"II*\x00") || bytes.starts_with(b"MM\x00*")) {
        return Ok(ImageFormat::Tiff);
    }

    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Ok(ImageFormat::WebP);
    }

    Err(CoreError::UnsupportedFormat(
        "unrecognized magic bytes: file is not a supported image format".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sniff_jpeg() {
        let data = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        assert_eq!(sniff_format(&data).unwrap(), ImageFormat::Jpeg);
    }

    #[test]
    fn test_sniff_png() {
        let data = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";
        assert_eq!(sniff_format(data).unwrap(), ImageFormat::Png);
    }

    #[test]
    fn test_sniff_gif() {
        let data_87 = b"GIF87a\x01\x00\x01\x00";
        let data_89 = b"GIF89a\x01\x00\x01\x00";
        assert_eq!(sniff_format(data_87).unwrap(), ImageFormat::Gif);
        assert_eq!(sniff_format(data_89).unwrap(), ImageFormat::Gif);
    }

    #[test]
    fn test_sniff_bmp() {
        let data = b"BM\x36\x00\x00\x00";
        assert_eq!(sniff_format(data).unwrap(), ImageFormat::Bmp);
    }

    #[test]
    fn test_sniff_tiff_le_and_be() {
        let le = b"II*\x00\x08\x00\x00\x00";
        let be = b"MM\x00*\x00\x00\x00\x08";
        assert_eq!(sniff_format(le).unwrap(), ImageFormat::Tiff);
        assert_eq!(sniff_format(be).unwrap(), ImageFormat::Tiff);
    }

    #[test]
    fn test_sniff_webp() {
        let mut data = Vec::new();
        data.extend_from_slice(b"RIFF");
        data.extend_from_slice(&[0, 0, 0, 0]);
        data.extend_from_slice(b"WEBP");
        assert_eq!(sniff_format(&data).unwrap(), ImageFormat::WebP);
    }

    #[test]
    fn test_sniff_reject_unknown() {
        let data = b"hello world this is text";
        assert!(sniff_format(data).is_err());
    }
}
