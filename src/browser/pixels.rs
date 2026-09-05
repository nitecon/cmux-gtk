//! Bounded CPU image decoding; GTK only receives ready-to-display RGBA pixels.
use image::{ImageDecoder, ImageReader};
use std::io::Cursor;

const MAX_EDGE: u32 = 8192;
const MAX_PIXELS: u64 = 16 * 1024 * 1024;
static DECODERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

/// Own a tightly packed, unpremultiplied RGBA image without copying its pixel buffer.
pub(super) struct Pixels {
    pub width: i32,
    pub height: i32,
    pub bytes: glib::Bytes,
}

/// Admit at most two blocking decoders globally; overload drops this preview frame without queuing.
/// A running decoder retains its permit even if the awaiting widget is destroyed.
pub(super) async fn decode(runtime: &tokio::runtime::Handle, bytes: glib::Bytes) -> Option<Pixels> {
    let Ok(permit) = DECODERS.try_acquire() else {
        super::metrics::decode_overload();
        return None;
    };
    runtime
        .spawn_blocking(move || {
            let _permit = permit;
            let started = std::time::Instant::now();
            let result = decode_pixels(&bytes);
            super::metrics::decoded(started.elapsed(), result.is_ok());
            result.ok()
        })
        .await
        .ok()
        .flatten()
}

/// Reject oversized headers before pixel allocation and decode only supported JPEG/PNG formats.
fn decode_pixels(bytes: &[u8]) -> image::ImageResult<Pixels> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_EDGE);
    limits.max_image_height = Some(MAX_EDGE);
    // Decoder allocation accounting is best-effort; dimension and pixel checks are strict.
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(image::ImageError::Limits(
            image::error::LimitError::from_kind(image::error::LimitErrorKind::DimensionError),
        ));
    }
    let mut output_budget = image::Limits::default();
    output_budget.max_alloc = Some(128 * 1024 * 1024);
    output_budget.reserve(decoder.total_bytes())?;
    let rgba = image::DynamicImage::from_decoder(decoder)?.into_rgba8();
    Ok(Pixels {
        width: width as i32,
        height: height as i32,
        bytes: glib::Bytes::from_owned(rgba.into_raw()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;

    /// Saturated decoder capacity rejects immediately and released capacity admits a real image.
    #[tokio::test]
    async fn decode_admission() {
        let permits = DECODERS.acquire_many(2).await.unwrap();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode(&[255, 0, 0], 1, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        let bytes = glib::Bytes::from_owned(jpeg);
        let runtime = tokio::runtime::Handle::current();
        assert!(tokio::time::timeout(
            std::time::Duration::from_secs(1),
            decode(&runtime, bytes.clone())
        )
        .await
        .unwrap()
        .is_none());
        drop(permits);
        let pixels = decode(&runtime, bytes).await.unwrap();
        assert_eq!((pixels.width, pixels.height), (1, 1));
        assert_eq!(DECODERS.available_permits(), 2);
    }

    /// Decode an actual PNG and preserve dimensions, colors and straight alpha.
    #[test]
    fn png_pixels() {
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                &[255, 0, 0, 128, 0, 255, 0, 255],
                2,
                1,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        let pixels = decode_pixels(&png).unwrap();
        assert_eq!((pixels.width, pixels.height), (2, 1));
        assert_eq!(pixels.bytes.as_ref(), &[255, 0, 0, 128, 0, 255, 0, 255]);
    }

    /// A valid JPEG header with excessive dimensions fails its limit before scan decoding.
    #[test]
    fn oversized_jpeg() {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode(&[255, 0, 0], 1, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        assert!(decode_pixels(&jpeg).is_ok());
        let frame = jpeg
            .windows(2)
            .position(|bytes| bytes == [0xff, 0xc0])
            .unwrap();
        for (width, height) in [(8193u16, 1u16), (8192, 8192)] {
            jpeg[frame + 5..frame + 7].copy_from_slice(&height.to_be_bytes());
            jpeg[frame + 7..frame + 9].copy_from_slice(&width.to_be_bytes());
            assert!(matches!(
                decode_pixels(&jpeg),
                Err(image::ImageError::Limits(_))
            ));
        }
        assert!(decode_pixels(b"invalid image").is_err());
    }
}
