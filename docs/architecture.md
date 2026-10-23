# Architecture

`main.rs` parses CLI arguments, loads configuration, gathers inputs, dispatches
parsers and renders text or JSON. `config.rs` holds the TOML schema.

`map/mod.rs` detects a map format and dispatches to `gnu_ld.rs`, `arm_keil.rs`,
`keil_lx51.rs` or `lld.rs`. Parsers produce symbols and optional maximum-stack
information. GNU ld and ESP-IDF share a parser. `su.rs` reads GCC frame records;
`cgraph.rs` merges GCC IPA edges and builds a maximum chain using frame data.
Only GNU ld and ESP-IDF map modes load cgraph inputs.

`elf.rs` uses `object` and `gimli` to find DWARF subprogram address ranges,
then scans text bytes for selected ARM Thumb instructions or Xtensa ENTRY.
It demangles names with `rustc-demangle`. It does not derive sizes from
DWARF CFA tables. ELF inputs replace `.su` inputs, and ELF-only mode uses an
empty synthetic map labelled Rust/lld, including for Xtensa.

`analysis.rs` associates frames with map symbols, sorts frames and compares
thresholds. With no map symbols it includes all supplied frames. A maximum
chain takes precedence over the largest frame for stack-threshold checking.
Missing coverage is not currently a separate failure status.

See [input formats](toolchains.md), the [technical guide](../CLAUDE.md), and
[ADRs](adr/README.md) before extending the analysis contract.
