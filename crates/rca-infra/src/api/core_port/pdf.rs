use image::GenericImageView;
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref};
use rca_core::app::ports::ApiPortError;

fn px_to_pt(px: f32) -> f32 {
    px * 72.0 / 96.0
}

fn build_resource_names(count: usize) -> Vec<Box<[u8]>> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        out.push(format!("Im{}", i).into_bytes().into_boxed_slice());
    }
    out
}

pub fn build_presentation_pdf_bytes(
    width_px: f32,
    height_px: f32,
    slide_images: impl IntoIterator<Item = bytes::Bytes>,
) -> Result<Vec<u8>, ApiPortError> {
    let slide_images = slide_images.into_iter().collect::<Vec<_>>();
    let names = build_resource_names(slide_images.len());

    let page_width_pt = px_to_pt(width_px);
    let page_height_pt = px_to_pt(height_px);

    let mut pdf = Pdf::new();

    let catalog_id = Ref::new(1);
    let pages_id = Ref::new(2);
    pdf.catalog(catalog_id).pages(pages_id);
    let mut next_id = 3;

    let mut page_ids: Vec<Ref> = Vec::new();

    for (idx, bytes) in slide_images.into_iter().enumerate() {
        let dyn_img = image::load_from_memory(&bytes)
            .map_err(|e| ApiPortError::request("decode image", e))?;
        let (img_w, img_h) = dyn_img.dimensions();

        let mut jpeg_bytes: Vec<u8> = Vec::new();
        {
            use image::ImageEncoder;
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_bytes, 90);
            let rgb8 = dyn_img.to_rgb8();
            encoder
                .write_image(rgb8.as_raw(), img_w, img_h, image::ExtendedColorType::Rgb8)
                .map_err(|e| ApiPortError::request("encode jpeg", e))?;
        }

        let image_id = Ref::new(next_id);
        next_id += 1;
        let mut image_stream = pdf.stream(image_id, &jpeg_bytes);
        image_stream.pair(Name(b"Type"), Name(b"XObject"));
        image_stream.pair(Name(b"Subtype"), Name(b"Image"));
        image_stream.pair(Name(b"Width"), img_w as i32);
        image_stream.pair(Name(b"Height"), img_h as i32);
        image_stream.pair(Name(b"ColorSpace"), Name(b"DeviceRGB"));
        image_stream.pair(Name(b"BitsPerComponent"), 8);
        image_stream.pair(Name(b"Filter"), Name(b"DCTDecode"));
        image_stream.finish();

        let content_id = Ref::new(next_id);
        next_id += 1;
        let mut content = Content::new();
        content.save_state();
        content.transform([page_width_pt, 0.0, 0.0, page_height_pt, 0.0, 0.0]);
        let xobj_name = Name(&names[idx]);
        content.x_object(xobj_name);
        content.restore_state();
        let content_bytes = content.finish();
        pdf.stream(content_id, &content_bytes).finish();

        let resources_id = Ref::new(next_id);
        next_id += 1;
        let mut resources_dict = pdf.indirect(resources_id).dict();
        {
            let xobj = resources_dict.insert(Name(b"XObject"));
            let mut xobj_dict = xobj.dict();
            xobj_dict.pair(xobj_name, image_id);
            xobj_dict.finish();
        }
        resources_dict.finish();

        let page_id = Ref::new(next_id);
        next_id += 1;
        page_ids.push(page_id);

        let mut page_dict = pdf.indirect(page_id).dict();
        page_dict.pair(Name(b"Type"), Name(b"Page"));
        page_dict.pair(Name(b"Parent"), pages_id);
        page_dict.pair(
            Name(b"MediaBox"),
            Rect::new(0.0, 0.0, page_width_pt, page_height_pt),
        );
        page_dict.pair(Name(b"Resources"), resources_id);
        page_dict.pair(Name(b"Contents"), content_id);
        page_dict.finish();
    }

    pdf.pages(pages_id)
        .kids(page_ids.iter().copied())
        .count(page_ids.len() as i32);

    Ok(pdf.finish())
}
