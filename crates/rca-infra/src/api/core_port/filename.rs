pub fn sanitize_filename_component(input: &str) -> String {
    let replaced = input
        .replace(['\u{0000}', '/', '\\'], "_")
        .replace([':', '*', '?', '"', '<', '>', '|'], "_")
        .trim()
        .replace(' ', "_");

    let no_dot_segments = replaced
        .split('.')
        .filter(|seg| !seg.is_empty())
        .collect::<Vec<_>>()
        .join(".");

    if no_dot_segments.is_empty() {
        "Presentation".to_string()
    } else {
        no_dot_segments
    }
}
