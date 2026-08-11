# Pardosa

EDA storage layer implementing [fiber semantics](https://github.com/acje/fiber-semantics)
in Rust. Pardosa enforces event-driven correctness, auditability, and deletion
policy for Event Carried State Transfer (ECST).

Each domain entity's history is a **fiber** — a singly linked list of immutable
events — interleaved into an append-only **line** (dragline). A per-fiber state
machine governs the lifecycle.

Target user: complex enterprise data where **correctness matters more than
scale**. Per-aggregate linearizability is non-negotiable.

## What this repo is

The **canonical home of pardosa**, in two stages:

1. **Now — the specification.** The 1.0 spec is written here.
2. **Later — the code.** Once published, this is the repository crates.io
   points at.

No code is built from this repo yet.

## pardosa 1.0 is a rewrite, not an extraction

This matters, and it is the thing most likely to be misread. The working
implementation in [Mattilsynet/gh-report](https://github.com/Mattilsynet/gh-report)
under `crates/pardosa*` (~29k lines, 8 substantive crates, ~874 tests) is
**reference material and prior art**. It is not the thing being published.

Three decisions put it there:

- **The published structure is redesigned from scratch** for strangers,
  superseding PGN-0001's in-tree ring layout. The current crate cut is an
  artefact of in-tree development.
- **1.0 uses a new format.** The rebuild is not required to read data written
  by the current implementation; a one-time migration tool carries old data
  across. PGN-0009's clean-break posture licenses this.
- **The transfer is clean-room, `code → spec → code`** — for design quality and
  spec validation, on the principle that if the library can be rebuilt from the
  spec, the spec is complete. It is *not* an IP mechanism: gh-report is public
  under `Apache-2.0 OR MIT`, which already permits the copy with attribution.

Sequencing: **reorganize first, freeze second.** Freezing surfaces that are
about to move would force a 2.0 almost immediately.

## Status

Specifying. This repo holds a [wayfinder](https://github.com/mattpocock/skills)
map — a bd epic whose child tickets are the open decisions between here and a
defined end state.

```bash
bd show pardosa-jn1                 # the map
bd ready --parent pardosa-jn1 -u    # the frontier
```

**Destination**: a written 1.0 spec that fixes the scope boundary, designs the
published crate structure, and freezes the public API of *that* structure, to a
standard fit for public crates.io consumers who are strangers.

The clean-room transfer — byte-level format design, the implementation-grade
behavioural spec, conformance criteria, migration tooling and wall discipline —
is a **second map**, charted once this one closes.

See `AGENTS.md` for how to resume the work.

## Lineage

| Stage | Where | What it contributed |
|-------|-------|---------------------|
| Conceptual model | [acje/fiber-semantics](https://github.com/acje/fiber-semantics) | Fibers, lines, draglines, migrations, the per-fiber state machine |
| Go prototype | [acje/web-service-gin](https://github.com/acje/web-service-gin) | `pardosa.Server[T]`, `Dragline[T]`, `map[DomainIdentity]Fiber` |
| Rust port, Apr 2026 | `docs/origin/`, source in history at `25fa1b0` | 5-state / 10-transition state machine, DOT visualisation, design notes |
| Reference implementation | gh-report `crates/pardosa*` | The behaviour the 1.0 spec must capture |
| 1.0 spec | this repo | Scope boundary, published structure, frozen API — in progress |

The prototype's model is not archaeology: `FiberState { Undefined, Defined,
Detached, Purged, Locked }` and `FiberMigrationPolicy { Keep, Purge,
LockAndPrune }` still ship in `crates/pardosa/src/fiber_state.rs` unchanged.

## Reference implementation — current in-tree layout

**This table describes what exists today in gh-report, not what will be
published.** The crate cut is being redesigned (`pardosa-jn1.17`) and the
publish column reflects decisions already taken on the map, not the manifests
as they stand.

| Ring       | Crate                  | Publish at 1.0 | Purpose                                    |
|------------|------------------------|----------------|--------------------------------------------|
| substrate  | `pardosa-wire`         | yes            | `no_std` canonical encode/decode           |
| substrate  | `pardosa-derive`       | yes            | proc-macros (`GenomeSafe`, schema derives) |
| substrate  | `pardosa-file`         | yes            | `.pgno` container writer/reader            |
| substrate  | `pardosa-nats`         | yes            | JetStream backend, behind a `nats` feature |
| vocabulary | `pardosa-schema`       | yes            | typed payload vocabulary                   |
| runtime    | `pardosa`              | yes            | `EventStore` facade                        |
| adapter    | `pardosa-fiber-store`  | yes            | sync one-key-one-fiber adapter             |
| adapter    | `pardosa-read`         | as a tool      | read-only CLI; no lib-semver contract      |

Test-only crates are not published — test scaffolding is not a generally
useful part.

Ring dependencies are currently one-directional per PGN-0001, and external
consumers depend only on `pardosa`. **PGN-0001 is slated for supersession** by
the structural redesign; treat the ring model as the current implementation's
shape, not as a commitment the 1.0 spec inherits.

## License

Apache-2.0 OR MIT, matching gh-report. The April 2026 prototype was MIT-only;
reconciling that across the published tarballs is a spec decision.
