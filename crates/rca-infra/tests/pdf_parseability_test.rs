use bytes::Bytes;
use rca_infra::api::YktApiPort;
use std::io::Cursor;

// NOTE: We intentionally test the same PDF-building logic used by the download
// feature (YktApiPort::download_presentation), but without doing any network IO.
//
// The production helper is currently implemented as an `impl` method; in tests
// we access it via a small local wrapper to avoid exposing it as part of the
// public API.
fn build_pdf_bytes(width_px: f32, height_px: f32, slides: Vec<Bytes>) -> Vec<u8> {
    // This path should remain in sync with the production implementation.
    // If this ever fails to compile, it means the production signature changed
    // and this test should be updated accordingly.
    YktApiPort::build_presentation_pdf_bytes(width_px, height_px, slides)
        .expect("build_presentation_pdf_bytes should succeed for valid images")
}

#[test]
fn production_generated_pdf_is_parseable_and_page_count_matches() {
    let mut slides: Vec<Bytes> = Vec::new();
    for i in 0..3u8 {
        // Create a deterministic PNG (the production code will re-encode to JPEG).
        let img = image::RgbImage::from_fn(64, 64, |x, y| {
            image::Rgb([(x % 255) as u8, (y % 255) as u8, i])
        });
        let mut png_bytes: Vec<u8> = Vec::new();
        {
            use image::ImageEncoder;
            let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
            encoder
                .write_image(img.as_raw(), 64, 64, image::ExtendedColorType::Rgb8)
                .unwrap();
        }
        slides.push(Bytes::from(png_bytes));
    }

    let pdf_bytes = build_pdf_bytes(64.0, 64.0, slides);
    assert!(pdf_bytes.starts_with(b"%PDF-"));

    let doc = lopdf::Document::load_from(Cursor::new(pdf_bytes)).unwrap();
    assert_eq!(doc.get_pages().len(), 3);
}

#[test]
fn truncated_pdf_is_not_parseable() {
    let img = image::RgbImage::from_fn(64, 64, |x, y| image::Rgb([x as u8, y as u8, 0]));
    let mut png_bytes: Vec<u8> = Vec::new();
    {
        use image::ImageEncoder;
        let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
        encoder
            .write_image(img.as_raw(), 64, 64, image::ExtendedColorType::Rgb8)
            .unwrap();
    }

    let pdf_bytes = build_pdf_bytes(64.0, 64.0, vec![Bytes::from(png_bytes)]);
    let truncated = &pdf_bytes[..pdf_bytes.len().saturating_sub(20)];
    assert!(lopdf::Document::load_from(Cursor::new(truncated)).is_err());
}
