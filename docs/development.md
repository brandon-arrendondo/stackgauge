# Development

Read [AGENTS.md](../AGENTS.md) and [CLAUDE.md](../CLAUDE.md) first.
[ADRs](adr/README.md) record policy proposals and their implementation status.

```sh
pre-commit install
pre-commit run --all-files
cargo test --all-targets
cargo clippy -- -D warnings
```

`pre-commit install` installs both the pre-commit and commit-msg hooks. Guard regression
tests run with `python3 -W error::ResourceWarning -m unittest discover -s scripts -p 'test_check_*.py'`.
CI repeats the hooks and checks new commit messages. Before relying on the Codex command
hook, review and trust it through `/hooks`.

Rust unit tests live beside the implementation; sample parser inputs live
under `tests/fixtures`. Add synthetic fixtures or public, redistributable
builds with recorded provenance. Keep build output from non-public projects
out of this repository. Tasks are maintained separately from the source tree.

Update the user guide and manual when CLI behavior changes. Record
user-visible effects in [CHANGELOG.md](../CHANGELOG.md); internal work stays
in Git history. A proposed ADR describes a target, not shipped behavior; document only
what the code does.
Before a release, curate notes from the changelog and complete the artifact
review described by the publication ADR. The current release workflow still
generates GitHub notes automatically; it does not consume the changelog.
