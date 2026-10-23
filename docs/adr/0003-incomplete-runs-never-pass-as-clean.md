# ADR-0003: An incomplete or failed analysis never passes as clean

**Status:** Proposed, 2026-10-22. Behavioral target; not implemented.

## Context

Stackgauge is used as a CI and pre-commit gate. A passing threshold check
can currently describe only the data found, rather than a complete stack
bound. The usage guide documents current behavior. This proposal changes
none of it.

## Decision

An incomplete or failed analysis never passes as clean. Success requires
enough frame and call-graph information to support the requested claim.
No frame data and no call-graph data is failure, not PASS.

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
- Parsers, graph traversal, reporting and CLI regressions need implementation.
- Already produced output does not turn a partial analysis into a completed one.
- Record the shipped behavior under Changed in the changelog.

Origin: the shared incomplete-run principle in Moldy ADR-0003 and Knots
ADR-0005, restated for stack coverage rather than formatting completion.
