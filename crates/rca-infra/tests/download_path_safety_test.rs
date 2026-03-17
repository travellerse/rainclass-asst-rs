use std::path::Path;

// We keep these tests simple and local to `rca-infra`.
// The implementation lives in `YktApiPort` but we test the behavior via
// small helper wrappers here to avoid coupling tests to HTTP/network.

fn sanitize_filename_component(input: &str) -> String {
    // Mirror the production behavior (keep in sync with core_port.rs).
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

fn is_single_component(file_name: &str) -> bool {
    let p = Path::new(file_name);
    p.file_name().is_some() && p.components().count() == 1
}

#[test]
fn sanitize_filename_removes_separators_and_nul() {
    let s = "a/b\\c\u{0000}d:e*?\"<>|  end";
    let out = sanitize_filename_component(s);
    assert!(!out.contains('/'));
    assert!(!out.contains('\\'));
    assert!(!out.contains('\u{0000}'));
    assert!(!out.is_empty());
}

#[test]
fn sanitized_filename_is_single_component() {
    let out = sanitize_filename_component("../etc/passwd");
    // Even if input looks like traversal, output must not be a path.
    let file_name = format!("{out}_20260101010101.pdf");
    assert!(is_single_component(&file_name));
    assert!(!file_name.contains(".."));
}

#[test]
fn rejects_obvious_traversal_components_for_filename() {
    assert!(!is_single_component("../a.pdf"));
    assert!(!is_single_component("a/b.pdf"));
    assert!(!is_single_component("/abs.pdf"));

    // NOTE: On Unix, backslash is a valid filename character, not a separator.
    // So we don't assert it as traversal here. The production sanitizer still
    // replaces backslashes to keep cross-platform safety.
}
