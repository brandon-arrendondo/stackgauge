# Usage

Supply a map file, an ELF file, or both:

```sh
stackgauge build/firmware.map --su-dir build/ --cgraph-dir build/
stackgauge --elf build/firmware.elf --stack-threshold 2048
stackgauge build/firmware.map --su-file build/main.su --format json
```

Use `--top-n 20` to show more frames or `-v` for all. This affects text display,
not threshold checking. JSON includes `functions`, `call_graph` and separate
stack/depth threshold flags. Check `call_graph.available`, `has_unknown` and
`unknown_factors` before interpreting a cumulative number.

`--stack-threshold` checks the reported maximum chain when available,
otherwise the largest single frame. `--depth-threshold` checks the reported
chain length. Equality is allowed. Exit 1 means a threshold was exceeded;
exit 2 means a fatal error or invalid invocation. Exit 0 means no configured
threshold was exceeded. In this version it can also result when data is absent
or incomplete, so a PASS does not establish a complete stack bound. Dynamic
frames, missing frames, indirect calls and recursion need review by other means.
[ADR-0003](adr/0003-incomplete-runs-never-pass-as-clean.md) proposes a stricter
contract; it has not shipped.

For a consuming project's pre-commit configuration:

```yaml
repos:
  - repo: local
    hooks:
      - id: stackgauge
        name: Stack analysis
        language: system
        entry: stackgauge
        args: [build/firmware.map, --su-dir=build/, --cgraph-dir=build/]
        pass_filenames: false
```

Build the firmware and install Stackgauge before running this hook. Add
thresholds appropriate to the firmware only after reviewing input coverage.
