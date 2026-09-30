// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{path::Path, process::Command, sync::atomic::AtomicU64};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "greenmote-cli-errors-{name}-{}-{}",
            std::process::id(),
            NEXT_TEMP_DIR.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn a_failed_command_prints_its_message_and_exits_1() {
    let profile = TempDir::new("profile");
    std::fs::write(profile.path().join("openmw.cfg"), "").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_greenmote"))
        .arg("--openmw-cfg")
        .arg(profile.path())
        .arg("--config")
        .arg(profile.path().join("greenmote.toml"))
        .arg("unclip")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "error: unclip requires --plugin or [unclip].plugin in greenmote.toml\n"
    );
}

#[test]
fn a_multi_line_error_keeps_its_line_breaks() {
    let profile = TempDir::new("missing");

    let output = Command::new(env!("CARGO_BIN_EXE_greenmote"))
        .arg("--openmw-cfg")
        .arg(profile.path().join("missing").join("openmw.cfg"))
        .arg("convert")
        .arg("--dry-run")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8(output.stderr).unwrap();
    let Some((_prompt, error)) = stderr.split_once("error: ") else {
        panic!("no error line in {stderr:?}");
    };
    assert!(error.contains("\n\n"), "{stderr}");
    assert!(!error.contains("\\n"), "{stderr}");
    assert!(!error.contains("Custom {"), "{stderr}");
}

#[test]
fn a_max_tilt_outside_0_to_90_is_a_usage_error() {
    for max_tilt in ["0", "90.5"] {
        let output = Command::new(env!("CARGO_BIN_EXE_greenmote"))
            .args(["unclip", "--plugin", "groundcover.omwaddon", "--max-tilt"])
            .arg(max_tilt)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(2), "--max-tilt {max_tilt}");
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains("--max-tilt"), "{stderr}");
    }
}
