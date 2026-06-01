use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub stack_threshold: Option<u64>,
    pub depth_threshold: Option<usize>,
    pub top_n: Option<usize>,
    #[serde(default)]
    pub su_dirs: Vec<String>,
    #[serde(default)]
    pub exclude_dirs: Vec<String>,
    pub format: Option<String>,
    pub toolchain: Option<String>,
    /// Path to an ELF file for Rust/lld DWARF-based stack analysis.
    pub elf_path: Option<String>,
}
