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
  elf.rs           ELF/DWARF + instruction scanner (ARM Thumb-2 and Xtensa LX)
  map/
    mod.rs         MapData / Symbol types, format detection, parser dispatch
    gnu_ld.rs      GNU ld map parser
    arm_keil.rs    ARM/Keil armlink map parser (call graph embedded)
    keil_lx51.rs   Keil C51/LX51 (8051) map parser
    lld.rs         Rust/lld map parser (symbols sparse due to LTO)
```

## ELF path (`--elf`)

`--elf <binary>` enables ELF-based frame analysis. ISA is detected from the
ELF `e_machine` field; ARM Thumb-2 and Xtensa LX (ESP32) are supported.
The approach (in `src/elf.rs`):

1. Parse `DW_TAG_subprogram` entries from `.debug_info` → function name +
   address range (uses `gimli`; names demangled with `rustc-demangle`).
2. Collect all `SectionKind::Text` sections so functions in any code segment
   are reachable (ESP32 splits code across `.iram0.text`, `.flash.text`, etc.).
3. For each function, read the relevant bytes and extract the frame size:

### ARM Thumb-2

Scans the full function body for prologue instructions:
- `PUSH {rlist}` (`0xB4xx`) — popcount(rlist) × 4 bytes
- `PUSH {rlist, LR}` (`0xB5xx`) — (popcount(rlist) + 1) × 4 bytes
- `SUB SP, SP, #imm7×4` (`0xB080–0xB0FF`) — imm7 × 4 bytes

**Why not `.debug_frame` / DWARF CFA?**  LLVM sets the CFA base to `r7`
(frame pointer) on Cortex-M, so SUB SP allocations after `ADD r7, sp, #0`
are invisible in the unwind tables. Instruction scanning gives correct
results where CFA gives zero.

### Xtensa LX (ESP32 / ESP-IDF)

Every non-leaf Xtensa function starts with an `ENTRY a1, imm12*8` instruction
as its very first 3 bytes. The frame size is read from the first instruction
only — no full-body scan needed.

24-bit little-endian encoding:
- byte 0 = `0x36` (op0=6 CALL group, op1=3 ENTRY opcode)
- byte 1 = `(imm12[3:0] << 4) | 0x1` (low nibble = a1/sp register)
- byte 2 = `imm12[11:4]`
- frame size = `imm12 * 8`

Leaf functions that omit ENTRY have no local frame and report 0 bytes.

**Requirement**: binary must include `.debug_info` (compiled with
`debug = 2` or at least `debug = 1`; strip must be off).

## ESP-IDF map + `.su` path

ESP-IDF linker maps use a two-line section format in the linked output:

```
 .text.function_name
                 0xADDRESS       0xSIZE  esp-idf/component/libxxx.a(file.c.obj)
                 0xADDRESS                function_name
```

`gnu_ld.rs` handles this: a section name on its own line is held as a
"pending" header; the address/size/obj line that follows resolves it.

To enable `.su` file generation for ESP-IDF GCC builds, add to
`src/CMakeLists.txt` (user component only):

```cmake
target_compile_options(${COMPONENT_LIB} PRIVATE -fstack-usage)
```

Or to instrument all ESP-IDF components, add to the root `CMakeLists.txt`
between `include($ENV{IDF_PATH}/tools/cmake/project.cmake)` and `project()`:

```cmake
idf_build_set_property(COMPILE_OPTIONS "-fstack-usage" APPEND)
```

After rebuilding, `.su` files land alongside `.obj` files under `build/esp-idf/`.
Run stackgauge with:

```bash
stackgauge build/firmware.map --su-dir build/
```

## Adding a new map format

1. Add a parser module under `src/map/` implementing `parse(content: &str, format: MapFormat) -> Result<MapData>`.
2. Register a new `MapFormat` variant in `src/map/mod.rs`.
3. Add format detection heuristics to `detect_format()` in `src/map/mod.rs`.
4. Wire the variant into the `match` in `parse_map()`.
5. Add a test fixture under `tests/` and an integration test.
