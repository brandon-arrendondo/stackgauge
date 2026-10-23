# Installation

Install Rust with Cargo, then build from a source checkout:

```sh
cargo build --release --locked
cargo install --path . --locked
stackgauge --version
```

The checkout pins Rust in `rust-toolchain.toml`. The installed executable is
`stackgauge` (`stackgauge.exe` on Windows). Cargo's bin directory must be on
PATH. Linux users can read the manual with `man ./docs/stackgauge.1`.
The release workflow builds Linux and Windows archives, Debian and RPM
packages, and an AppImage. Release archives carry dependency licence notices
and a CycloneDX SBOM. Build the firmware separately; Stackgauge does not
invoke its compiler or linker.
