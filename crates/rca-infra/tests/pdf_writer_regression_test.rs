#[cfg(test)]
mod tests {
    fn px_to_pt(px: f32) -> f32 {
        px * 72.0 / 96.0
    }

    #[test]
    fn px_to_pt_conversion_matches_96dpi_assumption() {
        // 96 px == 1 inch; 1 inch == 72 pt
        assert!((px_to_pt(96.0) - 72.0).abs() < 1e-6);
        assert!((px_to_pt(192.0) - 144.0).abs() < 1e-6);
    }

    #[test]
    fn scale_formula_fills_page_when_same_as_slide() {
        // If image pixel dimensions equal slide pixel dimensions,
        // the computed scale should be 1.
        let slide_w_px = 1920.0;
        let slide_h_px = 1080.0;
        let img_w_px = 1920.0;
        let img_h_px = 1080.0;

        let page_w_pt = px_to_pt(slide_w_px);
        let page_h_pt = px_to_pt(slide_h_px);

        let scale_x = page_w_pt / px_to_pt(img_w_px);
        let scale_y = page_h_pt / px_to_pt(img_h_px);

        assert!((scale_x - 1.0).abs() < 1e-6);
        assert!((scale_y - 1.0).abs() < 1e-6);
    }
}
