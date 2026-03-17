use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref};
use std::io::Cursor;

#[test]
fn generated_pdf_is_parseable() {
    let mut jpeg_bytes: Vec<u8> = Vec::new();
    {
        use image::ImageEncoder;
        let img = image::RgbImage::from_fn(64, 64, |x, y| {
            image::Rgb([(x % 255) as u8, (y % 255) as u8, 0])
        });
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, 90);
        encoder
            .write_image(img.as_raw(), 64, 64, image::ExtendedColorType::Rgb8)
            .unwrap();
    }

    let mut pdf = Pdf::new();
    let catalog_id = Ref::new(1);
    let pages_id = Ref::new(2);
    let image_id = Ref::new(3);
    let content_id = Ref::new(4);
    let resources_id = Ref::new(5);
    let page_id = Ref::new(6);

    pdf.catalog(catalog_id).pages(pages_id);
    pdf.pages(pages_id).kids([page_id]).count(1);

    let mut image_stream = pdf.stream(image_id, &jpeg_bytes);
    image_stream.pair(Name(b"Type"), Name(b"XObject"));
    image_stream.pair(Name(b"Subtype"), Name(b"Image"));
    image_stream.pair(Name(b"Width"), 64i32);
    image_stream.pair(Name(b"Height"), 64i32);
    image_stream.pair(Name(b"ColorSpace"), Name(b"DeviceRGB"));
    image_stream.pair(Name(b"BitsPerComponent"), 8i32);
    image_stream.pair(Name(b"Filter"), Name(b"DCTDecode"));
    image_stream.finish();

    let mut content = Content::new();
    content.save_state();
    content.transform([72.0, 0.0, 0.0, 72.0, 0.0, 0.0]);
    content.x_object(Name(b"Im0"));
    content.restore_state();
    let content_bytes = content.finish();
    pdf.stream(content_id, &content_bytes).finish();

    let mut resources_dict = pdf.indirect(resources_id).dict();
    let xobj = resources_dict.insert(Name(b"XObject"));
    let mut xobj_dict = xobj.dict();
    xobj_dict.pair(Name(b"Im0"), image_id);
    xobj_dict.finish();
    resources_dict.finish();

    let mut page_dict = pdf.indirect(page_id).dict();
    page_dict.pair(Name(b"Type"), Name(b"Page"));
    page_dict.pair(Name(b"Parent"), pages_id);
    page_dict.pair(Name(b"MediaBox"), Rect::new(0.0, 0.0, 72.0, 72.0));
    page_dict.pair(Name(b"Resources"), resources_id);
    page_dict.pair(Name(b"Contents"), content_id);
    page_dict.finish();

    let pdf_bytes = pdf.finish();

    let doc = lopdf::Document::load_from(Cursor::new(pdf_bytes)).unwrap();
    assert_eq!(doc.get_pages().len(), 1);
}
