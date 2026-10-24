# Configuration

The default file is `stackgauge.toml` in the current working directory.
Select another with `-c FILE`. All relative paths are relative to the working
directory, including paths in that file.

```toml
stack_threshold = 2048
depth_threshold = 12
top_n = 20
su_dirs = ["build"]
exclude_dirs = ["probe"]
# toolchain = "gnu-ld"
# elf_path = "build/firmware.elf"
```

CLI scalar values override configuration: thresholds, top count, toolchain
and ELF path. CLI `.su` directories are combined with `su_dirs`; they do not
replace them. Exclusions combine CLI and configuration values with the
built-in names `CompilerIdC`, `CompilerIdCXX` and `CompilerIdASM`.

The schema also accepts `format`, but the CLI currently ignores that config
field: use `--format json`. There are no config keys for cgraph paths or
explicit `.su` files; use their CLI options. An unknown key produces a parse warning,
and the whole file is then ignored in favor of the defaults. A missing or
unreadable file, even one named with `-c`, also falls back to the defaults.
Review stderr to confirm the intended configuration was used.

Canonical toolchain hints are `gnu-ld`, `esp-idf`, `arm-keil`, `keil-c51`
and `rust-lld`. An unrecognized hint falls back to content detection.
