use serde_json::Value;
use std::process::Command;

#[test]
fn help_is_complete_markdown_and_usable_without_repository_documents() {
    for args in [
        vec!["--help"],
        vec!["-h"],
        vec!["doctor", "--help"],
        vec!["help", "all"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
            .args(&args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stdout)
        );
        let help = String::from_utf8(out.stdout).unwrap();
        assert!(help.contains("hostvitals"));
        assert!(!help.contains("ADR"));
        assert!(!help.contains("docs/"));
        if args[0] == "--help" || args[0] == "-h" {
            for word in ["macOS", "read", "sudo", "doctor", "JSON", "skipped"] {
                assert!(help.contains(word), "{word}: {help}");
            }
        }
        if args[0] == "doctor" {
            for key in [
                "checks",
                "is_root",
                "macos_version",
                "energy_channels",
                "app_pid",
                "peak_footprint_bytes",
            ] {
                assert!(help.contains(key), "{key}: {help}");
            }
        }
    }
}

#[test]
fn invalid_requests_are_one_json_error_with_a_fix() {
    for args in [
        vec![],
        vec!["wat"],
        vec!["doctor", "--wat"],
        vec!["help", "wat"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
            .args(&args)
            .output()
            .unwrap();
        assert!(!out.status.success(), "{args:?}");
        let answer: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(answer.as_object().unwrap().len(), 1);
        assert!(
            answer["error"]
                .as_str()
                .unwrap()
                .contains("hostvitals --help")
        );
    }
}

#[test]
fn version_identifies_the_release_and_checkout() {
    let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success());
    let version = String::from_utf8(out.stdout).unwrap();
    if version.trim() == "hostvitals 0.1.0" {
        let tag = Command::new("git")
            .args(["describe", "--exact-match", "--tags", "--match", "v0.1.0"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap();
        assert!(
            tag.status.success(),
            "a release version requires its exact tag"
        );
    } else {
        assert!(version.starts_with("hostvitals 0.1.0+"), "{version}");
        assert!(version.contains(".g"), "{version}");
    }
}
