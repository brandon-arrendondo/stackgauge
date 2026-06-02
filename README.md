# stackgauge

Static stack depth analysis for embedded firmware. Parses linker map
files and GCC `.su` stack-usage files to report per-function frame sizes
and, where call-graph data is available, worst-case cumulative stack depth.

Designed to run as a pre-commit hook so stack overflows are caught before
they reach hardware.

---

## Supported toolchains

| Toolchain | Map format | Per-function frames | Call graph / depth |
|-----------|-----------|--------------------|--------------------|
| GCC + GNU ld | auto-detected | `.su` files (`-fstack-usage`) | `.cgraph` files (`-fdump-ipa-cgraph`) |
| ARM/Keil MDK (armlink) | auto-detected | `.su` files | Native (built into map) |
| ESP-IDF (Xtensa/GCC) | auto-detected | `.su` files (`-fstack-usage`) | `.cgraph` files (`-fdump-ipa-cgraph`) |
| Rust + lld | auto-detected or `--elf` | Thumb-2 instruction scan via `--elf` | not yet supported |

---

## Installation

```bash
cargo install --path .
```

Or build and copy the binary to somewhere on `$PATH`:

```bash
cargo build --release
cp target/release/stackgauge ~/.local/bin/
```

---

## Quick start

```bash
# GNU ld — report top-10 largest frames
stackgauge build/firmware.map --su-dir build/

# GNU ld — with depth analysis via call graph
stackgauge build/firmware.map --su-dir build/ --cgraph-dir build/

# ARM/Keil — call graph is embedded in the map file
stackgauge build/firmware.map --su-dir build/

# Fail if worst-case stack > 2 KB or call depth > 12
stackgauge build/firmware.map --su-dir build/ \
    --stack-threshold 2048 \
    --depth-threshold 12
```

Exit codes: **0** = pass, **1** = threshold exceeded, **2** = fatal error.

---

## Usage

```
stackgauge [OPTIONS] <MAP_FILE>

Arguments:
  <MAP_FILE>    Linker map file to analyse

Options:
  -s, --su-dir <DIR>          Search DIR recursively for .su files (repeatable)
      --su-file <FILE>        Add a single .su file explicitly (repeatable)
      --cgraph-dir <DIR>      Search DIR recursively for *.cgraph dumps (repeatable)
      --cgraph-file <FILE>    Add a single .cgraph file explicitly (repeatable)
      --exclude-dir <NAME>   Skip directories named NAME during .su / .cgraph search (repeatable)
  -t, --stack-threshold <N>  Fail if worst-case stack exceeds N bytes
      --depth-threshold <N>  Fail if call depth exceeds N (ARM/Keil or cgraph)
      --top-n <N>            Functions to show (default 10; see also -v)
  -v, --verbose              Show all functions, not just top N
  -f, --format <fmt>         Output format: text (default) or json
      --toolchain <name>     Force format: gnu-ld | arm-keil | esp-idf
  -c, --config <FILE>        Config file (default: stackgauge.toml)
  -h, --help                 Print help
  -V, --version              Print version
```

### Examples

```bash
# Show top 20 instead of default 10
stackgauge firmware.map --su-dir build/ --top-n 20

# Show every function
stackgauge firmware.map --su-dir build/ -v

# JSON output (useful for CI dashboards)
stackgauge firmware.map --su-dir build/ --format json | jq .call_graph

# Force ARM/Keil format when auto-detection fails
stackgauge build/firmware.map --toolchain arm-keil

# Mix explicit files and directory search
stackgauge firmware.map \
    --su-file build/src/main.c.su \
    --su-dir  build/lib/
```

---

## Configuration file

Place a `stackgauge.toml` in your project root (or pass `-c path/to/file`).
CLI flags always override config values.

```toml
# stackgauge.toml

# Fail if worst-case stack exceeds this many bytes
stack_threshold = 2048

# Directory names to exclude from .su / .cgraph search.
# CompilerIdC, CompilerIdCXX, CompilerIdASM are excluded by default.
# exclude_dirs = ["my_probe_dir"]

# Fail if call depth exceeds this (ARM/Keil or GNU ld + cgraph)
depth_threshold = 12

# Default number of functions to show
top_n = 20

# Directories to search for .su files (relative to cwd)
su_dirs = ["build/CMakeFiles", "build"]

# Force toolchain format: "gnu-ld" | "arm-keil" | "esp-idf"
# toolchain = "gnu-ld"
```

---

## Toolchain notes

### GCC / GNU ld

Add to your `CFLAGS` (or CMake `target_compile_options`):

```cmake
target_compile_options(${PROJECT_NAME} PRIVATE
    -fstack-usage        # generates *.su alongside *.o
    -fdump-ipa-cgraph    # generates *.cgraph for depth analysis
)
```

GCC writes files next to the object files:

```
build/CMakeFiles/myproject.dir/src/
    main.c.o
    main.c.su           ← per-function frame sizes
    main.c.001i.cgraph  ← call graph dump
```

Point stackgauge at the build directory and it finds them recursively:

```bash
stackgauge build/myproject.map \
    --su-dir    build/ \
    --cgraph-dir build/
```

Without `--cgraph-dir`, you get per-function frame sizes but no
cumulative depth analysis. Add it once your build supports
`-fdump-ipa-cgraph`.

#### Worked example: CMake ARM GCC project

This walks through enabling cgraph on a real Cortex-M23 project
(`d_leo_main`, GD32C231K8T6, 2 KB stack budget). The same steps apply to
any CMake + ARM GCC build.

**Step 1 — edit `app/CMakeLists.txt`**

Locate the `target_compile_options` call for your main target and add the
two flags:

```cmake
target_compile_options(d_leo_main PRIVATE
    # ... existing flags ...
    -fstack-usage
    -fdump-ipa-cgraph
)
```

**Step 2 — rebuild**

```bash
cmake --build build/Debug
```

GCC writes one `.su` and one `.cgraph` file per translation unit, placed
next to each `.o` in the CMake object directory:

```
build/Debug/CMakeFiles/d_leo_main.dir/app/src/
    main.c.o
    main.c.su
    main.c.001i.cgraph
build/Debug/CMakeFiles/d_leo_main.dir/app/src/lin/
    lin_bg.c.o
    lin_bg.c.su
    lin_bg.c.001i.cgraph
```

**Step 3 — run stackgauge**

```bash
stackgauge build/Debug/d_leo_main.map \
    --su-dir     build/Debug/CMakeFiles/d_leo_main.dir \
    --cgraph-dir build/Debug/CMakeFiles/d_leo_main.dir \
    --stack-threshold 2048
```

If you already ran stackgauge before adding cgraph support (`.su` files
present, no `.cgraph` files), the output will include per-function frame
sizes but report no depth. Re-running after the rebuild with `--cgraph-dir`
adds worst-case depth to every chain that cgraph can see.

#### Indirect calls and function pointers

`-fdump-ipa-cgraph` only captures static call edges that GCC can see at
compile time. Calls through function pointers are absent from the dump, so
any call chain that crosses them will have its depth silently
underestimated.

For `d_leo_main` two edges are not captured by cgraph:

| Caller | Callee | Reason |
|--------|--------|--------|
| `lin_bg_publish_response` | `main_Status_serialize` | function pointer dispatch |
| `lin_bg_handle_frame_complete` | `ui_Status_deserialize` | function pointer dispatch |

Workaround: write a hand-crafted `.cgraph` snippet that describes those
edges and pass it with `--cgraph-file`:

```bash
stackgauge build/Debug/d_leo_main.map \
    --su-dir     build/Debug/CMakeFiles/d_leo_main.dir \
    --cgraph-dir build/Debug/CMakeFiles/d_leo_main.dir \
    --cgraph-file extra_edges.cgraph \
    --stack-threshold 2048
```

The `.cgraph` format mirrors GCC's output; the parser accepts partial
files, so you only need to list the missing edges.

### ARM/Keil MDK (armlink)

The Keil map file already contains a symbol table and a call graph with
the worst-case stack calculation. No extra compiler flags are needed — just
point stackgauge at the `.map` file. Optionally add `.su` files for
frame-type annotations (static vs dynamic):

```bash
stackgauge build/firmware.map
stackgauge build/firmware.map --su-dir build/  # with .su annotations
```

### Rust / lld

Rust firmware built with `lto = true` (the default for release profiles) does
not emit per-function symbols in the linker map — all functions are merged
during link-time optimisation.  Use `--elf` instead: stackgauge reads
`DW_TAG_subprogram` entries from `.debug_info` to get function names and
address ranges, then scans the corresponding `.text` bytes for ARM Thumb-2
PUSH and SUB SP instructions to compute the actual frame size.

This instruction-scan approach is used instead of the DWARF CFA (`.debug_frame`)
because LLVM anchors the CFA to the frame pointer (`r7`) on Cortex-M targets,
making SUB SP allocations that occur after `ADD r7, sp, #0` invisible to the
unwind tables.

**Requirement**: the release profile must keep DWARF data:

```toml
# Cargo.toml
[profile.release]
lto = true
debug = 2   # preserve full DWARF; use "line-tables-only" for a smaller binary
```

**Usage**:

```bash
stackgauge --elf target/thumbv8m.base-none-eabi/release/firmware \
    --stack-threshold 2048
```

No map file is needed.  If you want map-based symbol filtering, provide both:

```bash
stackgauge target/thumbv8m.base-none-eabi/release/firmware.map \
    --elf target/thumbv8m.base-none-eabi/release/firmware \
    --stack-threshold 2048
```

Function names are automatically demangled using `rustc-demangle`; the raw
`DW_AT_linkage_name` Rust symbol (e.g. `_ZN8firmware4main17h…E`) is
converted to the human-readable form (e.g. `firmware::main`).

**Pre-commit hook for Rust projects**:

```yaml
repos:
  - repo: local
    hooks:
      - id: stackgauge
        name: Stack depth analysis (Rust)
        language: system
        entry: stackgauge
        args:
          - --elf=target/thumbv8m.base-none-eabi/release/firmware
          - --stack-threshold=2048
        pass_filenames: false
```

Or via `stackgauge.toml`:

```toml
elf_path = "target/thumbv8m.base-none-eabi/release/firmware"
stack_threshold = 2048
```

### ESP-IDF (Xtensa/GCC)

Same as GNU ld. Add to your component's `CMakeLists.txt`:

```cmake
idf_component_register(...)
target_compile_options(${COMPONENT_LIB} PRIVATE
    -fstack-usage
    -fdump-ipa-cgraph
)
```

Then run from your project root after `idf.py build`:

```bash
stackgauge build/project.map \
    --su-dir    build/ \
    --cgraph-dir build/
```

---

## Pre-commit integration

Add to your `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: local
    hooks:
      - id: stackgauge
        name: Stack depth analysis
        language: system
        entry: stackgauge
        args:
          - build/firmware.map
          - --su-dir=build/
          - --cgraph-dir=build/
          - --stack-threshold=2048
          - --depth-threshold=12
        pass_filenames: false
```

Or use `stackgauge.toml` for thresholds and keep the hook entry minimal:

```yaml
repos:
  - repo: local
    hooks:
      - id: stackgauge
        name: Stack depth analysis
        language: system
        entry: stackgauge
        args: [build/firmware.map, --su-dir=build/]
        pass_filenames: false
```

The hook exits 1 when a threshold is exceeded, which blocks the commit.
Build the binary first (`cargo build --release`) or install it globally so
the `system` language hook can find it on `$PATH`.
