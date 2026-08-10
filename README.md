# Pardosa

EDA storage layer implementing [fiber semantics](https://github.com/acje/fiber-semantics)
in Rust. Pardosa enforces event-driven correctness, auditability, and deletion
policy for Event Carried State Transfer (ECST).

Each domain entity's history is a **fiber** — a singly linked list of immutable
events — interleaved into an append-only **line** (dragline). A per-fiber state
machine governs the lifecycle.

## Where the code lives

The working library is developed in-tree at
[Mattilsynet/gh-report](https://github.com/Mattilsynet/gh-report) under
`crates/pardosa*` — eleven crates, ring-layered, ~874 tests. This repo is the
destination for the extracted, standalone library, and the home of the decisions
that define it.

`origin-prototype/` holds the April 2026 Rust port that started all this,
preserved unchanged apart from its location. It is history, not a dependency —
nothing builds against it.

## Lineage

| Stage | Where | What it contributed |
|-------|-------|---------------------|
| Conceptual model | [acje/fiber-semantics](https://github.com/acje/fiber-semantics) | Fibers, lines, draglines, migrations, the per-fiber state machine |
| Go prototype | [acje/web-service-gin](https://github.com/acje/web-service-gin) | `pardosa.Server[T]`, `Dragline[T]`, `map[DomainIdentity]Fiber` |
| Rust port | `origin-prototype/` in this repo | 5-state / 10-transition state machine, DOT visualisation, design notes |
| Production library | gh-report `crates/pardosa*` | The eleven-crate family as it stands today |
| 1.0 charter | this repo | Scope boundary and frozen public surface — in progress |

## Status

Charting. This repo holds a [wayfinder](https://github.com/mattpocock/skills)
map — a bd epic whose child tickets are the open decisions between here and a
defined end state for the library.

```bash
bd show pardosa-jn1                 # the map
bd ready --parent pardosa-jn1 -u    # the frontier
```

**Destination**: a written 1.0 charter fixing pardosa's scope boundary and
freezing the documented public API of the five publish-set crates, to a standard
fit for public crates.io consumers.

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

The extracted library is dual-licensed under Apache-2.0 and MIT, matching
gh-report. `origin-prototype/` was MIT-only; reconciling that is a charter
decision.
