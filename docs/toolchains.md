# Toolchains and input formats

This page describes the parsers currently shipped, rather than every report
a toolchain can generate.

| Mode | Inputs actually read | Frame/depth interpretation |
| --- | --- | --- |
| GNU ld | map + GCC `.su`; optional `.cgraph` | matched frames; cumulative chain from supplied static edges |
| ESP-IDF / Xtensa GCC | GNU-style map + `.su`; optional `.cgraph`, or ELF | map mode matches symbols; ELF mode scans ENTRY instructions |
| ARM/Keil MDK | one text file with recognized symbol/Call Graph sections; optional `.su` | parses maximum-stack and chain text if present in that file |
| Keil C51/LX51 | overlay-map text | XDATA overlay allocation and chain, not the 8051 hardware stack |
| Rust/lld | lld map + frames, or ELF | sparse LTO map symbols; ELF instruction-derived frames, no ELF-only graph |

For GCC, compile with `-fstack-usage` and `-fdump-ipa-cgraph`, then pass the
build directory with both `--su-dir` and `--cgraph-dir`. Rebuild after changing
flags. Graph dumps contain static edges only; function-pointer dispatch is
not complete, and a missing frame currently contributes zero.

The ARM parser looks for literal `Call Graph`, `Maximum Stack Usage` and
`Call chain for Maximum Stack Usage:` text within its single map input.
It has no separate armlink report option or HTML reader. A conventional
armlink map alone must not be assumed to provide cumulative stack data;
armlink's separate `.htm`/`.txt` report is not automatically discovered.
The combined fixture tests a parser shape, not full toolchain interoperability.

For ELF, retain full DWARF (`debug = 2` in Rust release profiles, with stripping
disabled). ARM scanning recognizes selected PUSH and SUB SP forms, not every
stack adjustment. Xtensa scanning reads the initial windowed-ABI ENTRY;
other ABIs and RISC-V ESP targets are not supported. Zero or unrecognized
frames are omitted. Do not interpret the result as complete coverage of all
functions or a cumulative bound. With both a map and ELF, map symbols filter
frames when symbols were parsed. ELF replaces `.su` data.

Stackgauge does not identify RTOS task entry points, allocate task budgets or
model interrupt nesting. Compare results against each runtime context's own
budget using that platform's documented units; do not infer them from frames.
