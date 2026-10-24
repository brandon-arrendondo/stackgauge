# ADR-0003: An incomplete or failed analysis never passes as clean

**Status:** Proposed, 2026-10-22. Behavioral target; not implemented.

## Context

Stackgauge is used as a CI and pre-commit gate. Today a passing threshold
check says only that the data stackgauge found stayed within the thresholds;
it does not establish a complete stack bound. The usage guide documents current behavior. This proposal changes
none of it.

## Decision

An incomplete or failed analysis never passes as clean. Success requires
enough frame and call-graph information to support the requested claim.
A run that finds neither frame data nor call-graph data fails; it does not PASS.

A lower-bound result must never pass silently: an unknown or untraceable
component, armlink's `+ Unknown(...)`, an unbounded or dynamic frame, or a
chain cut at an indirect call prevents an unqualified clean result. A bounded
dynamic frame needs its bound accounted for. Missing frames must not become
zero-cost callees without an explicit incomplete result.

Report the known lower bound and why it is incomplete. Distinguish a
complete result within thresholds, a complete threshold violation, and an
analysis that could not establish the bound. Define and test the exit-status
and machine-readable contracts before advertising this guarantee. An
explicit frame-only report must not imply a cumulative stack bound.

## Consequences

- Existing CI workflows may fail when they previously passed on partial data.
- The parsers, graph traversal and reporting need changes, with CLI regression tests.
- A report saved from a partial analysis remains a partial result.
- Record the shipped behavior under Changed in the changelog.

Origin: the shared incomplete-run principle in Moldy ADR-0003 and Knots
ADR-0005, restated for stack coverage rather than formatting completion.

Source record: [Moldy ADR](https://github.com/brandon-arrendondo/moldy/blob/b19b34bf1647525d29f1f9d0e20144151a33ab74/docs/adr/0003-incomplete-runs-never-pass-as-clean.md).
