# Identity resolution toward Pardosa 1.0

Written 2026-10-03. This roadmap evaluates and refines a boundary before
separately authorized implementation. It selects no identity architecture,
changes no specification and authorizes no migration or publication.
Only **cherry-pit** and **gh-report** are in scope; controlled consumers permit
clean breaking changes, not automatic destruction of their data.

## Current evidence and gaps

Paths and line numbers below describe the inspected local checkouts on this date,
not an audit of every dependency-pinned implementation.

| Boundary | Observed source / API | Meaning and remaining gap |
| --- | --- | --- |
| Core key derivation | `crates/pardosa/src/schema.rs:1155-1162`, `derive_fiber_id(&str) -> [u8; 16]` | BLAKE3 context `pardosa.fiber_id.v1`, first 16 bytes; no generation argument. Namespace, canonicalization, rename/reuse and collision policy remain undecided. |
| Core keyed access | `crates/pardosa/src/store/pipeline.rs:283-348`, `fiber([u8;16])`, `fiber_with_key(&str)`, `get_latest([u8;16]) -> Result<Option<&EventEnvelope>, OperationFailure>`, `get_latest_with_key(&str)` | Keyed helpers call the derivation helper. Latest is an envelope, not a complete consumer view; errors are not absence. |
| Blueprint tension | `docs/consumer_adapter_blueprint.md:7,19-21,79` | Prescribes an in-memory association and `store.begin(initial_event)`. Inspected `CreationPlan::begin(ArtefactPresence)` at `store.rs:1867-1879` creates an artefact, not a fiber. Neither the example nor memory-only durability settles the design; inventory actual minting paths. |
| Cherry consumer | `../cherry-pit/Cargo.toml:56` pins Pardosa 0.5.5 at `d1b9ca45bb539db9c46330b40a636d97a5c7cf38` | `crates/pardosa-cherry-pit-projection/src/lib.rs:47-52,323-415,432-508,533-605` constructs snapshot/checkpoint keys, derives IDs and persists/loads/deletes records. Snapshot reads decode the latest payload; checkpoints additionally validate record identity. Support `crates/pardosa-cherry-pit-test-support/src/lib.rs:273` also derives IDs; its complete flow remains to inventory. Neutral Cherry crates must remain free of normal/build Pardosa dependencies. |
| gh-report consumer | `../gh-report/Cargo.toml:111-113` pins direct Pardosa/NATS 0.5.5 at `08fcd290694553baa7fd5c3408bc18e9bd5eafb5`, Cherry support at `1ecc7b118bf5bf2ccb75c8d6a992fb65351d735e` | Lockfile also retains Cherry's `d1b9ca45` Pardosa revision. `crates/gh-report/src/store/mod.rs:359-453,467-509,615-680` derives IDs for keyed record/detach, folds history or retains latest per fiber. `app/state/mod.rs:2103-2277` and `event/mod.rs:1524-1563` provide repo/org/team callers and team keys. Exact pinned-source behavior, complete caller set and persisted datasets remain unverified. |

Inspection evidence lives in the pinned site store: **Current callers and
article/code comparison** (`bd show acje_github_io-tw1`, including its closing
correction), **Domain-key architectural constraints** (`bd show acje_github_io-ab5`),
and **Identity-seam orientation** (`bd show acje_github_io-e98`). These are planning
observations, not executed conformance evidence.

## Binding boundaries

Desired outcome: consumer integration/adapter owns domain-key namespaces,
canonicalization and resolution to generation-local storage identity; core
storage operates on validated storage identities. This ownership boundary chooses
neither derivation nor persistent associations, nor a mint-ownership API (future
ticket 3). Use fully controlled **cherry-pit** and **gh-report** to remove or
replace core key-policy helpers and update both consumers atomically when required;
prefer a clean break over unnecessary legacy shims. Final exact API cuts await
the future decisions and must satisfy the binding specification constraints;
no implementation is authorized now.

The [canonical specification](../spec/pardosa-1.0.md) remains the commitment.
C5.20 requires fresh event and fiber identity at each generation boundary;
C5.22 makes resume cursors generation-local. C6.18 accepts a payload-only
migration transform and assigns envelope fields in Pardosa; C6.20 requires
fresh dense chains. A permanent key-only hash across generations cannot meet
those obligations. Adding generation input is a candidate, not proof that
derivation can recover the IDs minted by migration.

C3.2 and C5.44 leave snapshot meaning outside core; C8.3's application schema
responsibilities do not mandate a particular identity map. PAR-0001 distinguishes
sealed storage adapters from consumer integration and records documentation/release
judgements; it selects no resolution algorithm. The affected decision must reconcile
the blueprint with demonstrated APIs rather than treat it as a working contract.

The existing [0.5.1 plan](pardosa-0.5.1.md#pre-handoff-exactly-two-clean-inputs)
retains its clean-room wall: clean transfer implementers receive exactly the
specification and a spec-derived suite. This roadmap, map, evidence beads and
sibling implementation observations are not a third clean input. Ordinary
consumer adapter maintenance is not silently reclassified as clean transfer.

## Separate future decision map

**Pardosa identity resolution toward 1.0** (`bd show pardosa-vqbq`) and its
children carry `mission:pardosa-identity-1`, separate from delivery epic
`acje_github_io-86k`. They remain open when drafting and article correction close.
No ticket is answered or claimed by chartering.

Use the pinned Pardosa store:

```sh
bd -C /Users/anders.jensen/code/pardosa show pardosa-vqbq
bd -C /Users/anders.jensen/code/pardosa ready --parent pardosa-vqbq -u
```

| Named ticket | Dependency | Decision-sized output |
| --- | --- | --- |
| **Inventory consumer keys, pinned callers and persisted data** (`bd show pardosa-vqbq.1`) | None; initial frontier | Evidence for exact pins/callers, datasets, key lifecycle cases and view completeness; unknowns explicit. AFK prerequisite, not design. |
| **Decide namespace and key lifecycle semantics** (`bd show pardosa-vqbq.2`) | Inventory | Human-decided canonicalization, namespaces, rename/alias/reuse/collision meanings with valid/invalid witnesses. |
| **Choose generation-aware resolution and mint ownership** (`bd show pardosa-vqbq.3`) | Namespace | Compare generation-qualified derivation against explicit consumer-owned associations using concrete lifecycle/remint cases; decide ownership and demonstrated API boundary. Neither is adopted here. |
| **Decide reconstruction, ambiguity and resource contract** (`bd show pardosa-vqbq.4`) | Resolution | Durable reconstruction inputs, restart/stale-generation behavior, missing/multiple/inconsistent outcomes, unknown distinct from absent; bounded items/bytes/tasks and lifecycle policy. |
| **Decide data disposition and live migration readiness** (`bd show pardosa-vqbq.5`) | Inventory | Authorized retain/export/migrate/discard policy per dataset, operator readiness and rollback. Live migration remains TODO in this ticket, not a prerequisite for current article corrections. |
| **Assess implementation handoff and production-informed 1.0 readiness** (`bd show pardosa-vqbq.6`) | Reconstruction and disposition | Independent affected-slice sufficiency assessment, lawful clean inputs where applicable, missing release/production evidence; no implementation dispatch by map closure alone. |

Future HITL decisions require the human; chartering does not launch interviews,
research or implementation. Fog includes exact API cuts, constructor witnesses,
workload limits and production evidence that earlier answers must sharpen.

## Evaluate → refine → implement gates

Resolve tradeoffs in the fleet priority order: maintainability and correctness
first; measure read latency and energy for real workloads before optimization.
Prefer the simpler seam only if it satisfies all witnessed lifecycle obligations.

| Milestone / sequence | Deliverable and acceptance gate | Rollback / stop |
| --- | --- | --- |
| E0 — evaluate inventory | Ticket 1 records exact revisions, both consumers' direct/support callers, key formats, datasets and complete-head/delta views. Inventory unknowns block affected decisions, not unrelated documentation. | Preserve source/data; correct evidence only. |
| E1 — evaluate alternatives | Tickets 2–3 compare derivation and associations with rename/reuse, collision and same-key G0/G1 witnesses. Require fresh target IDs and explicit ordinary/migration mint ownership; compare retained state and read cost without invented measurements. | No selected seam without evidence and human decision; return to inventory if a counterexample changes scope. |
| R0 — refine contract | Ticket 4 defines domain versus storage identity types, all construction/mutation routes, authoritative rebuild inputs, restart/remap and ambiguity outcomes. Name resource budgets, acquisition/RAII release, exhaustion, cancellation and shutdown for triggered paths. | Reopen the affected decision; never hide a missing mapping or failed probe as absence. |
| R1 — refine rollout | Ticket 5 records operator-approved data disposition separately for each consumer, rollback feasibility and retained source handling. Ticket 6 checks public APIs/errors, acceptance witnesses and the lawful handoff inputs. | Unknown data or insufficient clean inputs stops that affected slice. No implicit discard or third carrier. |
| I0 — implement core boundary, future authorization | After relevant decisions and commander approval, one bounded API/type slice with red→green evidence, external construction witnesses and adversarial review for public API/error changes. Reconcile keyed helpers and blueprint under that chosen contract. | Revert the intended local slice before consumer adoption; no data mutation in an API-only slice. |
| I1 — adopt controlled consumers, future authorization | Cherry outer adapters/support first, then gh-report direct/support paths on explicit compatible pins. Verify neutral Cherry dependency purity, resolution/rebuild and complete-head/delta view cases; no accidental duplicate incompatible Pardosa surfaces. | Return affected consumer to prior known-good pin only when its data is compatible; otherwise stop under the disposition plan. |
| I2 — migration/recovery, future authorization | Execute only after readiness/disposition approval. Witness fresh identities/cursors, payload-only transform, dense chains, restart/crash recovery, source append retirement and uncertainty behavior on applicable backends. | Use approved recovery policy; successful cutover does not license resumed source writes. Preserve evidence and stop on uncertain authority. |
| P0 — production-informed 1.0 readiness | Actual production iterations, complete supported surface and producer inventory, compatibility/freeze evidence and PAR release judgement. Consumer stabilization or map closure alone is insufficient. | Defer promotion; continue bounded 0.5.x work without waiving binding obligations. |

Sequence: E0 → E1 → R0; E0 → R1 disposition, with handoff assessment waiting
for R0 and disposition; approved handoff → I0 → I1 → I2 → P0 for affected
live-data paths. A demonstrably data-independent slice need not execute live
migration first, but cannot claim migration readiness or 1.0 promotion from that.

Each future mission defines actual commands before execution: changed-crate INNER,
changed plus reverse-dependent MID and epic BOUNDARY per `AGENTS.md:75-107`,
including the local `scripts/verify.sh` entry point. Spec changes additionally use
`./scripts/check.sh`; no spec change occurs here. Keep applicable conformance,
derive/construction, two-backend, resource and crash witnesses, exact revisions,
actual exits and measurement conditions in review evidence. Documentation checks
on this draft establish no runtime, migration or release readiness.
