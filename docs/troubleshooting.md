# Troubleshooting

**No function data:** confirm `.su` inputs were compiled with `-fstack-usage`
and match the linked build. Directory exclusions may hide probe output.
For ELF, retain `.debug_info` with subprogram ranges and executable sections;
stripped binaries and debug levels containing only line tables are unsuitable.
Only nonzero frames recognized by the instruction scanner are retained.

**No cumulative result:** GNU ld/ESP-IDF need `--cgraph-dir` or `--cgraph-file`
with GCC `-fdump-ipa-cgraph` output. ELF-only mode does not load cgraph data.
A normal armlink map is not its separate call-graph report; see the
[input-format limitations](toolchains.md).

**Unexpected PASS:** exit 0 currently means that available numbers did not
exceed a threshold. It can occur with no data or a lower bound. Check warnings,
function counts, call-graph availability and unknown factors. Missing `.su`
frames in a graph currently contribute zero; indirect calls are absent from
GCC's static graph. Do not treat that output as proof of a complete bound.

**Configuration not applied:** check the working directory and stderr. Invalid
TOML warns then discards the file's values; unreadable config falls back to
defaults. Use `--format json` rather than the currently ignored config key.
