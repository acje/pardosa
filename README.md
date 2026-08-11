# Pardosa

EDA storage layer implementing [fiber semantics](https://github.com/acje/fiber-semantics)
in Rust. Pardosa enforces event-driven correctness, auditability, and deletion
policy for Event Carried State Transfer (ECST).

Each domain entity's history is a **fiber** — a singly linked list of immutable
events — interleaved into an append-only **line** (dragline). A per-fiber state
machine governs the lifecycle.

## What this repo is

This is the **canonical home of pardosa**, in two stages:

1. **Now — the specification.** The 1.0 spec is written here: the scope
   boundary, and the frozen public surface of the crates published to crates.io.
2. **Later — the code.** When pardosa is published, the library moves here from
   its current in-tree location and this becomes the repository crates.io points
   at.

The working library today lives in-tree at
[Mattilsynet/gh-report](https://github.com/Mattilsynet/gh-report) under
`crates/pardosa*` — eleven crates, ring-layered, ~874 tests. It stays there
until the spec is settled.

No code is built from this repo yet. `docs/origin/` holds the design notes and
state-machine diagram from the April 2026 Rust prototype; the prototype's source
was removed once the spec work began, and remains in git history at `25fa1b0`.

## Lineage

| Stage | Where | What it contributed |
|-------|-------|---------------------|
| Conceptual model | [acje/fiber-semantics](https://github.com/acje/fiber-semantics) | Fibers, lines, draglines, migrations, the per-fiber state machine |
| Go prototype | [acje/web-service-gin](https://github.com/acje/web-service-gin) | `pardosa.Server[T]`, `Dragline[T]`, `map[DomainIdentity]Fiber` |
| Rust port | `docs/origin/`, source in history | 5-state / 10-transition state machine, DOT visualisation, design notes |
| Production library | gh-report `crates/pardosa*` | The eleven-crate family as it stands today |
| 1.0 spec | this repo | Scope boundary and frozen public surface — in progress |

The prototype's model is not archaeology: `FiberState { Undefined, Defined,
Detached, Purged, Locked }` and `FiberMigrationPolicy { Keep, Purge,
LockAndPrune }` still ship in `crates/pardosa/src/fiber_state.rs` unchanged.

## Status

Specifying. This repo holds a [wayfinder](https://github.com/mattpocock/skills)
map — a bd epic whose child tickets are the open decisions between here and a
defined end state for the library.

```bash
bd show pardosa-jn1                 # the map
bd ready --parent pardosa-jn1 -u    # the frontier
```

**Destination**: a written 1.0 spec fixing pardosa's scope boundary and freezing
the documented public API of the five publish-set crates, to a standard fit for
public crates.io consumers.

## Crates (as they stand in gh-report)

| Ring       | Crate                  | Publish | Purpose                                    |
|------------|------------------------|---------|--------------------------------------------|
| substrate  | `pardosa-wire`         | yes     | `no_std` canonical encode/decode           |
| substrate  | `pardosa-derive`       | yes     | proc-macros (`GenomeSafe`, schema derives) |
| substrate  | `pardosa-file`         | yes     | `.pgno` container writer/reader            |
| substrate  | `pardosa-nats`         | no      | JetStream backend                          |
| vocabulary | `pardosa-schema`       | yes     | typed payload vocabulary                   |
| runtime    | `pardosa`              | yes     | `EventStore` facade — the public surface   |
| adapter    | `pardosa-fiber-store`  | no      | sync one-key-one-fiber adapter             |
| adapter    | `pardosa-read`         | no      | read-only CLI                              |

Ring dependencies are one-directional (PGN-0001). External consumers depend only
on `pardosa`.

## License

Apache-2.0 OR MIT, matching gh-report. The April 2026 prototype was MIT-only;
reconciling that across the published tarballs is a spec decision.
