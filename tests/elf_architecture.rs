use std::process::Command;

#[test]
fn unsupported_isa_never_reports_pass() {
    for format in ["text", "json"] {
        let output = Command::new(env!("CARGO_BIN_EXE_stackgauge"))
            .args([
                "--elf",
                "tests/fixtures/elf-architecture/riscv.elf",
                "--format",
                format,
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("Riscv32"), "{error}");
        assert!(error.contains("ARM Thumb-2 and Xtensa LX"), "{error}");
    }
}

#[test]
fn supported_arm_still_reports_frames() {
    let output = Command::new(env!("CARGO_BIN_EXE_stackgauge"))
        .args([
            "--elf",
            "tests/fixtures/elf-architecture/arm.elf",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["functions"].to_string().contains("24"));
}

#[test]
fn supported_xtensa_still_reports_frames() {
    let output = Command::new(env!("CARGO_BIN_EXE_stackgauge"))
        .args([
            "--elf",
            "tests/fixtures/elf-architecture/xtensa.elf",
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(report["functions"].to_string().contains("32"));
}
