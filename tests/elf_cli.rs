use std::process::{Command, Output};

fn run(name: &str, json: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_stackgauge"));
    command.args(["--elf", &format!("tests/fixtures/elf/{name}.elf")]);
    if json {
        command.args(["--format", "json"]);
    }
    command.output().unwrap()
}

#[test]
fn compressed_debug_matches_plain_frames() {
    let plain = run("plain", true);
    let compressed = run("compressed", true);
    assert!(plain.status.success(), "{:?}", plain);
    assert!(compressed.status.success(), "{:?}", compressed);
    let plain: serde_json::Value = serde_json::from_slice(&plain.stdout).unwrap();
    let compressed: serde_json::Value = serde_json::from_slice(&compressed.stdout).unwrap();
    assert_eq!(plain["functions"], compressed["functions"]);
    assert!(plain["functions"].to_string().contains("24"));
    assert!(plain["functions"].to_string().contains("frame"));
}

#[test]
fn invalid_compression_fails_without_report() {
    for name in ["oversized", "damaged", "zstd"] {
        for json in [false, true] {
            let output = run(name, json);
            assert_eq!(output.status.code(), Some(2));
            assert!(output.stdout.is_empty());
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(error.contains("incomplete ELF analysis"), "{error}");
            assert!(error.contains(".debug_info"), "{error}");
        }
    }
}
