# Changelog

User-visible changes are recorded here, newest first, under Added, Changed,
Fixed or Removed. See [the changelog policy](docs/adr/0002-changelog-is-for-users.md).

## [Unreleased]

### Fixed

- Debian and RPM packages include the MIT license, Markdown guides, and changelog under `/usr/share/doc/stackgauge/`.
- Reject unsupported ELF architectures with exit 2 instead of treating their instructions as ARM Thumb-2.
- Read compressed ELF debug sections correctly; reject damaged sections or DWARF data exceeding a total 64 MiB limit with exit 2.
