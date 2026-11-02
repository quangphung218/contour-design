//! `"$OD_NODE_BIN" "$OD_HYPERFRAMES_BIN" lint …` as the HyperFrames
//! design template writes it: `OD_NODE_BIN` is `od`, which runs the CLI
//! `OD_HYPERFRAMES_BIN` names with the rest of the line.

#![cfg(unix)]
#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

const OD: &str = env!("CARGO_BIN_EXE_od");

#[test]
fn od_runs_the_hyperframes_cli_the_variable_names() {
    let tmp = tempfile::tempdir().unwrap();
    let cli = tmp.path().join("my-hyperframes");
    std::fs::write(&cli, "#!/bin/sh\necho \"hf $*\"\nexit 3\n").unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = Command::new(OD)
        .args([OD, cli.to_str().unwrap(), "lint", "comp"])
        .env("OD_HYPERFRAMES_BIN", &cli)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hf lint comp\n");
    assert_eq!(out.status.code(), Some(3), "the CLI's exit code");
    // With it set (every agent run), any other command is `od`'s own.
    let out = Command::new(OD)
        .args([OD, "--version"])
        .env("OD_HYPERFRAMES_BIN", &cli)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("hf "));
    // Without the variable, `od` never runs a path it was handed.
    let out = Command::new(OD)
        .args([cli.to_str().unwrap(), "lint"])
        .env_remove("OD_HYPERFRAMES_BIN")
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&out.stdout).contains("hf "));
    assert_ne!(out.status.code(), Some(3));
}
