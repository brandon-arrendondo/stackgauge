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

### ARM/Keil MDK (armlink)

The Keil map file already contains a symbol table and a call graph with
the worst-case stack calculation. No extra compiler flags are needed — just
point stackgauge at the `.map` file. Optionally add `.su` files for
frame-type annotations (static vs dynamic):

```bash
stackgauge build/firmware.map
stackgauge build/firmware.map --su-dir build/  # with .su annotations
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
