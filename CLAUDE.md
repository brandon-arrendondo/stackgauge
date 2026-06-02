# stackgauge — developer notes

## Build & test

```bash
cargo build --release        # binary at target/release/stackgauge
cargo test                   # unit + integration tests
cargo clippy -- -D warnings  # lint (CI enforces this)
cargo fmt                    # formatter (pre-commit hook runs this automatically)
```

Pre-commit hooks are active (`pre-commit install` already done). `cargo fmt`
runs on every commit; don't bypass with `--no-verify`.

## Architecture

```
src/
  main.rs          CLI (clap), config loading, dispatch
  config.rs        stackgauge.toml schema + CLI merge
  analysis.rs      frame/depth aggregation across all parsers
  su.rs            .su file parser (GCC -fstack-usage output)
  cgraph.rs        .cgraph parser (GCC -fdump-ipa-cgraph output)
  elf.rs           ELF/DWARF + Thumb-2 instruction scanner (--elf flag)
  map/
    mod.rs         MapData / Symbol types, format detection, parser dispatch
    gnu_ld.rs      GNU ld map parser
    arm_keil.rs    ARM/Keil armlink map parser (call graph embedded)
    keil_lx51.rs   Keil C51/LX51 (8051) map parser
    lld.rs         Rust/lld map parser (symbols sparse due to LTO)
```

## ELF / Rust + lld path

`--elf <binary>` enables ELF-based frame analysis for ARM Thumb-2 targets.
The approach (in `src/elf.rs`):

1. Parse `DW_TAG_subprogram` entries from `.debug_info` → function name +
   address range (uses `gimli`; names demangled with `rustc-demangle`).
2. For each function, slice the corresponding bytes from `.text` and scan
   for Thumb-2 prologue instructions:
   - `PUSH {rlist}` (`0xB4xx`) — popcount(rlist) × 4 bytes
   - `PUSH {rlist, LR}` (`0xB5xx`) — (popcount(rlist) + 1) × 4 bytes
   - `SUB SP, SP, #imm7×4` (`0xB080–0xB0FF`) — imm7 × 4 bytes

**Why not `.debug_frame` / DWARF CFA?**  LLVM sets the CFA base to `r7`
(frame pointer) on Cortex-M, so SUB SP allocations after `ADD r7, sp, #0`
are invisible in the unwind tables. Instruction scanning gives correct
results where CFA gives zero.

**Requirement**: binary must be compiled with `debug = 2` (or at least
`debug = 1`) so `.debug_info` is present. Strip must be off, or DWARF
sections must be in a separate `.dwarf` file.

## Adding a new map format

1. Add a parser module under `src/map/` implementing `parse(content: &str, format: MapFormat) -> Result<MapData>`.
2. Register a new `MapFormat` variant in `src/map/mod.rs`.
3. Add format detection heuristics to `detect_format()` in `src/map/mod.rs`.
4. Wire the variant into the `match` in `parse_map()`.
5. Add a test fixture under `tests/` and an integration test.
