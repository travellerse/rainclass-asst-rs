// this is a placeholder for UI automation tests using Playwright or other frameworks.
// currently the project uses Slint for the UI which runs as a desktop binary; a
// real automation suite would launch the compiled binary and interact with its
// window via Playwright or ScoutQA CLI.  For now we provide a basic smoke test
// to ensure the binary starts without panicking.

use std::process::Command;

#[test]
fn smoke_runs() {
    // build the executable and run it with `--help` to make sure it doesn't crash
    let output = Command::new("cargo")
        .args(&["run", "-p", "rca-desktop", "--", "--help"])
        .output()
        .expect("failed to spawn rca-desktop");
    assert!(output.status.success());
}
