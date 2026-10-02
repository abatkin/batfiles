//! Stand-ins for batfiles release binaries that are real executables, which Windows requires: one
//! program, compiled once per test run, that reads the target and version it plays from a
//! trailer appended to its own file. Shared by the test targets through `#[path]`.

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

/// The compiled stand-in program, without a trailer.
fn program() -> &'static [u8] {
    static PROGRAM: OnceLock<Vec<u8>> = OnceLock::new();
    PROGRAM.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let out = tempfile::TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).expect("a build dir");
        let exe = out
            .path()
            .join(format!("stand-in{}", std::env::consts::EXE_SUFFIX));
        // From the package root, so rustup chooses the pinned toolchain.
        let status = Command::new("rustc")
            .current_dir(root)
            .args(["--edition", "2024", "-o"])
            .arg(&exe)
            .arg(root.join("tests/common/stand_in_program.rs"))
            .status()
            .expect("rustc runs");
        assert!(status.success(), "the stand-in program compiles");
        std::fs::read(&exe).expect("the stand-in program")
    })
}

/// A stand-in for the batfiles release binary for `target` that reports `version`.
pub(crate) fn stand_in(target: &str, version: &str) -> Vec<u8> {
    let mut bytes = program().to_vec();
    bytes.extend_from_slice(format!("\nbatfiles-stand-in {target} {version}\n").as_bytes());
    bytes
}
