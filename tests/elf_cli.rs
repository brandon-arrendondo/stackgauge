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
    for name in [
        "oversized",
        "damaged",
        "zstd",
        "trailing",
        "overstated",
        "understated-one",
    ] {
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

#[test]
fn understated_size_cannot_hide_a_second_compilation_unit() {
    for format in ["text", "json"] {
        for (name, code) in [("two-units", 1), ("understated", 2)] {
            let output = Command::new(env!("CARGO_BIN_EXE_stackgauge"))
                .args([
                    "--elf",
                    &format!("tests/fixtures/elf/{name}.elf"),
                    "--format",
                    format,
                    "--stack-threshold",
                    "50",
                ])
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(code), "{output:?}");
            if code == 2 {
                assert!(output.stdout.is_empty());
                assert!(String::from_utf8_lossy(&output.stderr)
                    .contains("cannot decompress .debug_info"));
            } else {
                assert!(String::from_utf8_lossy(&output.stdout).contains("large_frame"));
                assert!(String::from_utf8_lossy(&output.stdout).contains("72"));
            }
        }
    }
}

#[test]
fn large_plain_debug_strings_are_not_charged_to_decompression() {
    // Add a 65 MiB .debug_str to the synthetic plain ELF. Its zero-filled
    // data is created as a sparse temporary file rather than a large fixture.
    let original = std::fs::read("tests/fixtures/elf/plain.elf").unwrap();
    let get = |offset: usize| {
        u32::from_le_bytes(original[offset..offset + 4].try_into().unwrap()) as usize
    };
    let table = get(32);
    let names_offset = get(table + 4 * 40 + 16);
    let names_size = get(table + 4 * 40 + 20);
    let mut bytes = original[..table].to_vec();
    let names_start = bytes.len();
    bytes.extend_from_slice(&original[names_offset..names_offset + names_size]);
    bytes.extend_from_slice(b".debug_str\0");
    let new_table = bytes.len();
    bytes.extend_from_slice(&original[table..table + 5 * 40]);
    let put = |bytes: &mut Vec<u8>, offset: usize, value: usize| {
        bytes[offset..offset + 4].copy_from_slice(&(value as u32).to_le_bytes());
    };
    put(&mut bytes, 32, new_table);
    bytes[48..50].copy_from_slice(&6u16.to_le_bytes());
    put(&mut bytes, new_table + 4 * 40 + 16, names_start);
    put(&mut bytes, new_table + 4 * 40 + 20, names_size + 11);
    let data_start = new_table + 6 * 40;
    let size = 65 * 1024 * 1024;
    for value in [names_size, 1, 0, 0, data_start, size, 0, 0, 1, 0] {
        bytes.extend_from_slice(&(value as u32).to_le_bytes());
    }
    let path =
        std::env::temp_dir().join(format!("stackgauge-plain-dwarf-{}.elf", std::process::id()));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    use std::io::Write;
    (&file).write_all(&bytes).unwrap();
    file.set_len((data_start + size) as u64).unwrap();
    drop(file);
    let outputs: Vec<_> = ["text", "json"]
        .iter()
        .map(|format| {
            Command::new(env!("CARGO_BIN_EXE_stackgauge"))
                .arg("--elf")
                .arg(&path)
                .args(["--format", format])
                .output()
                .unwrap()
        })
        .collect();
    std::fs::remove_file(path).unwrap();
    for output in outputs {
        assert!(output.status.success(), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stdout).contains("frame"));
    }
}
