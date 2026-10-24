# stackgauge

Static stack analysis for embedded firmware. Stackgauge reads linker maps
with GCC stack-usage (`.su`) and call-graph dumps, or an ELF file's DWARF and
instruction bytes, and reports each function's stack frame and, where the
inputs allow, the cumulative stack of its call chains.

```sh
cargo install --path . --locked
stackgauge build/firmware.map --su-dir build/ --cgraph-dir build/
stackgauge --elf build/firmware.elf --stack-threshold 2048
```

Build the firmware first. `--stack-threshold` checks the reported maximum
chain when available, otherwise the largest frame. In this version, a PASS means only that the data stackgauge found
stayed within the configured thresholds. A run with missing or partial data
can still pass, so a PASS does not establish a complete stack bound.
See [usage](docs/usage.md) before using it as a build gate.

## Supported inputs

| Toolchain | Inputs actually read | Result |
| --- | --- | --- |
| GCC / GNU ld | map + `.su`, optional `.cgraph` | frames and optional static chain |
| ESP-IDF / Xtensa GCC | GNU-style map + `.su`/`.cgraph`, or ELF | matched frames/chain, or instruction-derived frames |
| ARM/Keil MDK | text containing recognized map and Call Graph sections | frames from `.su`; the maximum and its chain only if the same file contains them |
| Keil C51/LX51 | overlay map | XDATA overlay usage and chain |
| Rust/lld | lld map + frame inputs, or ELF | instruction-derived ARM frames via ELF; no ELF-only chain |

The ARM parser reads one file. It neither finds armlink's separate call-graph
report nor has an option to accept one, so do not expect a cumulative result
from an ordinary armlink map. ELF needs `.debug_info` and a supported ISA;
its scanner recognizes selected ARM Thumb and Xtensa LX instructions.
See [toolchains and limitations](docs/toolchains.md).

## Documentation

- [Installation](docs/installation.md)
- [Usage and pre-commit integration](docs/usage.md)
- [Configuration](docs/configuration.md)
- [Toolchains and input formats](docs/toolchains.md)
- [Architecture](docs/architecture.md)
- [Development](docs/development.md) and [agent guidelines](AGENTS.md)
- [Troubleshooting](docs/troubleshooting.md)
- [Architecture decisions](docs/adr/README.md)
- [Changelog](CHANGELOG.md) and [manual source](docs/stackgauge.1)

Run `stackgauge --help` for all options. Use `--format json` for structured
output, `--top-n N` for a longer table, or `-v` for all frames.

## AI Assistance

Stackgauge was developed with assistance from [Claude](https://claude.ai) (Anthropic), used for implementation, tests, bug fixes, and project setup. From October 2026, [Codex](https://openai.com/codex/) (OpenAI) also contributed the tooling that enforces the agent and commit guidelines, and documentation. Each of its changes was reviewed before it was merged.

Many earlier commits have a `Co-Authored-By: Claude` trailer, but not every AI-assisted commit does, so the trailers are not a complete record. From October 2026 the contribution is acknowledged once, here, and not with a co-author trailer on each commit.

## License

MIT. Copyright 2026 BISSELL Homecare, Inc. See [LICENSE](LICENSE).
