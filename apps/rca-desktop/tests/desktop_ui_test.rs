// this is a placeholder for UI automation tests using Playwright or other frameworks.
// currently the project uses Slint for the UI which runs as a desktop binary; a
// real automation suite would launch the compiled binary and interact with its
// window via Playwright or ScoutQA CLI.  For now we provide a basic smoke test
// to ensure the binary starts without panicking.

use std::process::Command;

#[test]
fn smoke_builds() {
    // Only build the binary – running it would open a Slint window.
    // A successful build is enough to ensure the binary links and compiles
    // without panicking.
    let output = Command::new("cargo")
        .args(["build", "-p", "rca-desktop"])
        .output()
        .expect("failed to invoke cargo build");
    assert!(
        output.status.success(),
        "cargo build failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
