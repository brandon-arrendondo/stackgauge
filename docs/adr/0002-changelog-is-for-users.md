# ADR-0002: The changelog tells users what changed; it is not a git log or a task list

**Status:** Proposed, 2026-10-22.

## Context

A Stackgauge user needs to know when input support, reported stack values,
threshold behavior or output fields change. Implementation history does not
serve that purpose. The release workflow currently generates GitHub notes
from commits; this proposal introduces a curated CHANGELOG.md. Drawing the
release body from it remains release-process work.

## Decision

Create `CHANGELOG.md` starting with the next release as a curated record for
users of Stackgauge. Each release has one dated section, newest first, with
Unreleased on top. Draw the GitHub release body from that release's section;
a compare link may supplement the notes but does not replace them. Write the
entry in the same change that ships the user-visible effect, under Unreleased.
Use these headings only when they have entries:

1. **Added:** a new input format, CLI option or analysis
   capability. Describe what the user can now do.
2. **Fixed:** a crash, incorrect output, nondeterminism or other tool defect.
   Describe the failing construct and resulting behavior in user terms.
3. **Removed:** an option, output field, platform or supported behavior
   a user could have relied on. Even small removals must be recorded.
4. **Changed:** a change to existing output's meaning or an option's behavior,
   including changed defaults. It is not a category for internal work.

**A change to analysis meaning is listed under Changed, even when it
corrects a bug.** Explain which inputs and reported values change, so users
know to review their thresholds and saved reports. Describe the correction
there rather than duplicating it under Fixed.

Entries are short publication-ready explanations, not copied commit subjects
or work-item titles. Exclude corpus work, docs-only edits, CI and packaging
chores, refactors, tests or fixtures added for their own sake, and dependency
changes with no user-visible effect. If any of those ships a user-visible
capability or fix, describe that effect instead.

Do not publish internal tracking references or locate defects in another
project that have not been fixed upstream. The changelog is part of the public
record, and the intention is to ship it in release packages; today's packages
do not include it.

## Consequences

- Release notes require editorial review; automation can collect candidates
  but cannot decide that every completed change deserves an entry.
- Group related effects so a release remains readable. Absence of internal
  work from the changelog is correct, not missing attribution.
- Backfilling older releases is a separate maintainer decision. Git history
  remains the implementation record; this ADR authorizes no history rewrite
  or replacement of already-published archives.
- Deprecated and Security headings are not used. A security fix is a Fixed
  entry, published only once fixed; if it involves another project, wait
  until the fix has landed upstream. Nothing is deprecated yet; if something
  is, add the Deprecated heading then.
- Corpus results stay in the analysis tests. An analysis-change entry
  points readers to that evidence without turning the changelog into a
  test report.

Origin: adapted from Moldy ADR-0002 and the shared changelog policy.
The user-facing categories and publication rules are retained; examples
refer to analysis rather than formatting. Older releases are not backfilled.

Source record: [Moldy ADR](https://github.com/brandon-arrendondo/moldy/blob/b19b34bf1647525d29f1f9d0e20144151a33ab74/docs/adr/0002-changelog-is-for-users.md).
