# stackgauge

Static stack analysis for embedded firmware. Reads linker maps, GCC stack
usage and call-graph dumps, or ELF/DWARF plus instruction bytes to report
per-function frames and, where available, cumulative chains.

```sh
cargo install --path . --locked
stackgauge build/firmware.map --su-dir build/ --cgraph-dir build/
stackgauge --elf build/firmware.elf --stack-threshold 2048
```

Build the firmware first. `--stack-threshold` checks the reported maximum
chain when available, otherwise the largest frame. A current PASS means no
configured threshold was exceeded by the available data; missing or partial
data can still pass. It does not establish a complete stack bound.
See [usage](docs/usage.md) before using it as a build gate.

## Supported inputs

| Toolchain | Inputs actually read | Result |
| --- | --- | --- |
| GCC / GNU ld | map + `.su`, optional `.cgraph` | frames and optional static chain |
| ESP-IDF / Xtensa GCC | GNU-style map + `.su`/`.cgraph`, or ELF | matched frames/chain, or instruction-derived frames |
| ARM/Keil MDK | text containing recognized map and Call Graph sections | frames with `.su`, maximum/chain if present in the same input |
| Keil C51/LX51 | overlay map | XDATA overlay usage and chain |
| Rust/lld | lld map + frame inputs, or ELF | instruction-derived ARM frames via ELF; no ELF-only chain |

The ARM parser does not discover or accept a separate armlink call-graph
report through a dedicated option. A conventional map alone is insufficient
to assume a cumulative result. ELF needs `.debug_info` and a supported ISA;
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

Stackgauge was developed with assistance from [Claude](https://claude.ai)
(Anthropic) for implementation, tests and project setup. From October 2026,
[Codex](https://openai.com/codex/) (OpenAI) also contributed the agent and
commit guideline tooling and documentation. Brandon Arrendondo is the
accountable BISSELL Associate responsible for review and publication.

Many earlier commits carry AI co-author trailers, but those trailers are not
a complete record of AI assistance. From October 2026, assistance is
acknowledged once here rather than through per-commit AI trailers.

## License

MIT. Copyright 2026 BISSELL Homecare, Inc. See [LICENSE](LICENSE).
