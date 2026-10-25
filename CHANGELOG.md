# Changelog

User-visible changes are recorded here, newest first, under Added, Changed,
Fixed or Removed. See [the changelog policy](docs/adr/0002-changelog-is-for-users.md).

## [Unreleased]

### Fixed

- Debian and RPM packages include the MIT license, Markdown guides, and changelog under `/usr/share/doc/stackgauge/`.
- Reject unsupported ELF architectures with exit 2 instead of treating their instructions as ARM Thumb-2.
- Read zlib-compressed ELF debug sections with strict stream and size validation; reject damaged streams or decompression exceeding a total 64 MiB budget with exit 2. Plain sections do not count against this budget.
