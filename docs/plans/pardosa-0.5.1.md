# Pardosa 0.5.1 delivery plan

Status: **Reviewed written plan — planning-side only; implementation not started.**
Authored 2026-09-07 for **Write and review the pardosa 0.5.1 implementation
plan** (`bd show pardosa-j48a`). This document authorizes no implementation,
publication, dependency selection or specification change.

## Authority, destination and boundary

The [canonical specification](../spec/pardosa-1.0.md) is the sole normative
commitment. [C4.1](../spec/pardosa-1.0.md#c41--invariant),
[C4.2](../spec/pardosa-1.0.md#c42--invariant) and
[C5.58](../spec/pardosa-1.0.md#c558--invariant) distinguish three checkpoints:

1. **Planning ready, established:** the closed **Pardosa 1.0 spec** map
   (`bd show pardosa-jn1`) and **Planning readiness** (`bd show pardosa-gudk.2`).
   Artefact, epoch, anchor and locator are adopted, not another noun ballot.
2. **0.5.1 publishable, not established:** defined supported semantics, implemented
   binding obligations, both-backend conformance and actual release evidence.
   Surface evolution during 0.5.x does not permit undefined semantics or knowingly
   unmet obligations. The first release ships the settled crate structure.
3. **1.0 promotion, not established:** production-informed judgement after 0.5.x,
   remaining pre-freeze duties and the frozen surface, not a downstream consumer's
   stabilization automatically triggering a version bump.

Planning sources are the complete old map Destination, Notes, Out-of-scope and
T1–T20/S1–S10 register; **Clean-room wall** (`bd show pardosa-jn1.78`),
**Descriptor discovery disposition** (`bd show pardosa-jn1.91`),
[Semantic reconciliation](../spec/trace/semantic-reconciliation.md), and
**Planning orientation** (`bd show pardosa-a1x1`). These are contaminated planning
inputs. The oracle advisory (`bd show pardosa-e6tl`) is not citation authority:
release lockstep is C5.37, licenses C10.4, MSRV C4.23; C5.37 says nothing about
synchronous facades. Current specification and adopted definitions take precedence
over historical postponement language. No reference implementation was read here.

### PRE-HANDOFF: exactly two clean inputs

The wall is inside transfer work, between planning and construction. **This plan,
both maps, reports, ADRs, bead bodies and planning sessions never cross it.** Clean
implementers start fresh sessions with no reference or sibling access and receive
exactly the canonical specification and a conformance suite derivable from that
specification alone. No sanitized plan, task brief, format sidecar or constructor
catalogue is an automatically authorized third input. The purpose is design quality
and specification sufficiency, not an IP claim.

[C3.4](../spec/pardosa-1.0.md#c34--invariant) and
[C3.11](../spec/pardosa-1.0.md#c311--invariant) charter format work;
[C5.2](../spec/pardosa-1.0.md#c52--invariant) preserves one normative commitment.
**Where chartered byte-format and implementation-grade behavior deliverables live
within the two authorized carriers is unresolved.** The carrier question below
must resolve their authority and sufficient availability before every affected
handoff. Calling a document nonnormative or attaching it to a suite does not solve
this. Tests may not invent missing semantics; adapter predicates may not be put
into clauses merely to move them across the wall.

For each proposed slice, planning records a semantic/format dependency closure,
the exact authorized inputs carrying it, clause-derived acceptance conditions and
an independent sufficiency assessment. Missing constructor meaning, bytes or proof
requirements block that slice. Independence from deferred S3 work must be shown,
not assumed. Required spec/suite completion is separately authorized work, not an
edit licensed by this plan. A need for a third carrier is a stop, not a waiver.

## Separate decision map

**Pardosa clean-room transfer to 0.5.1** (`bd show pardosa-jjap`) is chartered,
open, with no decisions resolved. It is a decision-only map: future construction
missions/backlog are separate and are not created here. Chartering does not launch
research or permit the agent to supply a human answer.
The map and all six children use their own `mission:transfer-051` membership,
not the drafting mission's label; completing this draft does not reclaim future work.

| Question / prerequisite | Kind | Dependency | Decision-sized output |
| --- | --- | --- | --- |
| **Which format and behavior content can the two clean carriers lawfully carry?** (`bd show pardosa-jjap.1`) | AFK research | None | Source-backed carrier reconciliation and explicit unresolved authority; targeted oracle clarification before dispatch. |
| **Assemble a representative spec-derived discovery envelope** (`bd show pardosa-jjap.3`) | AFK task prerequisite | None | Small illustrative domain-use envelope with candidate acceptance/invalid witnesses, not adopted requirements. Routine example selection needs no HUMAN interview; escalate only demonstrated material new family/constraint meaning or required-domain expressiveness loss. |
| **What evidence obligations must an authority and resource design discharge?** (`bd show pardosa-jjap.6`) | AFK task prerequisite | None | Proof-obligation inventory and measurement proposal that make later design choices answerable; no recovery algorithm or implementation. |
| **Can the byte-format design preserve unknown-tag rejection and future multi-region use?** (`bd show pardosa-jjap.4`) | AFK research | Carrier question | Lagging-reader compatibility constraints and unresolved choices; independent timestamp-placement question, no selected layout. |
| **What stronger derive seal can preserve supported external derive use?** (`bd show pardosa-jjap.5`) | AFK research | None | Feasible assurance/cost comparison and required downstream-use proofs before choosing a seal. |
| **Is any first clean slice sufficiently specified and independent?** (`bd show pardosa-jjap.2`) | AFK task prerequisite | Carrier, domain, format and proof-obligation work | One bounded coverage witness or blocked recommendation for commander dispatch; not building that slice. |

The first-slice assessment can remain blocked after its prerequisite tickets close:
their outputs may identify additional work. The seal question is not arbitrarily
a prerequisite for every first slice; its stronger result is owed before 1.0, while
current producer guarantees bind from 0.5.1. Any affected slice still needs adequate
current producer semantics. The existing bounded-discovery authority permits routine
spec-derived example selection without another interview. Such examples remain
illustrative, not new requirements; clean outputs use only canonical semantics.
Fog includes per-constructor choices exposed by discovery, specific resource/recovery
designs and production evidence not yet available. One actual
decision per future session; research uses copernicus, HITL requires the human.

## Milestones and dependency DAG

Milestones are future delivery boundaries, **not tickets to execute this session**.
Each evidence package names revision, commands, actual exits, workload and limits;
an independent reviewer can reproduce its check without trusting a summary.

```text
M0 -> M1 -> M2 -> M3
M3 -> M4
M3 -> M5
M4 -> M6
M5 -> M6
M2 -> M7
M4 -> M7
M5 -> M7
M6 -> M7 -> M8
M8 -> production iterations + stronger seal -> M9 1.0 promotion/freeze
```

M4 and M5 can proceed independently only after M3 and their own clean-input gates.
M1 repeats at each affected handoff; it is not a blanket one-time dispatch permit.
M7 requires M6, both adapters and M2; earlier conformance runs belong inside each
increment, not postponed until integration. M8's documentation and packaging
preparation may run earlier, but its acceptance waits for M7.

| Milestone | Bounded deliverable / discovery | Independently verifiable exit |
| --- | --- | --- |
| M0 — planning frontier | Resolve carrier route; assemble illustrative spec-derived domain-use envelope under existing bounded-discovery authority; enumerate authority/resource proof needs; investigate format and seal questions. | Source-backed decisions or explicit blocked items; no mandatory HUMAN example-selection interview; no third carrier; all thirty transfer rows retained. No implementation-readiness inference from map closure. |
| M1 — clean-input readiness | Complete meanings used by the slice and byte-format/behavior inputs through authorized carriers. Separate field semantics, container format and standard envelope shape; no invented layouts or pins. | Independent read-level closure witness: every required meaning, format decision and acceptance condition available to a fresh implementer in spec/suite; unresolved affected handoffs remain blocked. Initial tests cite clauses and do not add guarantees. |
| M2 — constrained foundation | Realize admitted constructor vocabulary, portable descriptors, schema/envelope identity, codecs and derive. Complete S3 within authorized bounded discovery; name actual shipped producers. | Local construction-route audit, structural descriptor and codec proofs, and boundary/invalid decode tests below; positive downstream derive examples; missing-kind/unsupported-shape diagnostics; cycle compile-fail proof; portable structural decode without parsing Rust. No integrated adapter is required to close M2: C8.2 adapter instantiations belong to M4/M5 and aggregate at M7. |
| M3 — common domain and lifecycle | Realize strict create/open, result families, valid cursor construction, epoch/ownership model, qualified reads and mandatory integrity. Choose representations only after lawful input completion; no API names invented here. | Clause-mapped valid/invalid state tests; external construction compile-fail evidence; complete named conditions without catch-alls; distinct claim loss, stale ownership, unreadable record and unknown landing; one qualified result retains coexisting facts and excludes illegal combinations. |
| M4 — file capability | Implement file creation/exclusion/durability and read-only capability under platform conditions. Internal exclusion operation, method-less marker and orthogonal seals remain distinct. | Concurrent creation has one winner; writer-session exclusion and per-landing epoch checks; unavailable exclusion distinct from held exclusion; stale durability replacement leaves history unchanged and events resident without retry permission. Qualified orphan reads pass ordinary checks. Instantiate C8.2 structural checks and applicable format vectors on the file adapter. File predicate tests are separately identified from universal conformance. |
| M5 — NATS capability | Implement the same promises using backend-appropriate enforcement and artefact-scoped descriptor/ownership storage. Feature selection adds capability, never vocabulary. | Same clause-derived facade suite as M4, including the NATS instantiation of C8.2 structural checks and applicable format vectors; live backend two-writer and stale-write evidence, indeterminate landing and diagnostic isolation. In-memory substitutes alone do not establish NATS support. |
| M6 — migration and recovery | Operator-initiated chase/freeze/cutover, caller transformation and named policies, generation records, recovery proof design and chartered legacy plus 0.5.x migration tooling. | Failure-injection matrix below passes both adapters; successful cutover permanently rejects all source appends; uncertain authority blocks renewed ordinary writes; prior landing established before permitted resubmission; output-clean erasure, fresh identities, dense chaining and pairwise order verified. Legacy acceptance and explicit broken-chain election exercised; ordinary readers retain their refusals. |
| M7 — integrated assurance | Both-backend symmetric conformance, all supported resource lifetimes, feature/platform evidence, fresh clean rebuild sufficiency. | Aggregate M4/M5 C8.2 and format evidence with M6 migration/legacy cases; all applicable clause tests run on real adapters; unsupported/unverified capability reported honestly; resource saturation/cancellation/shutdown evidence; once-at-rebuild sufficiency assessed independently. No missing run is reported as passed. |
| M8 — 0.5.1 release | Complete every supported meaning, current guarantees, docs/tooling, graph MSRV, package archives and release records. | No knowingly unmet binding clause; both adapters pass suite; missing-docs and public-surface gate verified; MSRV dependency graph established; archives contain licenses; lockstep/caret relationships and feature behavior checked; documentation judgement recorded. Actual publication remains separately authorized. |
| M9 — 1.0 freeze | Production iterations inform structure; finish stronger seal and final surface/producer inventory; enable compatibility gate. | Named production evidence and narrative promotion ADR; stronger seal proven with downstream use and adversarial construction routes; semver baseline and failure behavior verified, unstable support excluded structurally. No mechanical promotion threshold invented. |

### Bounded format acceptance packet

M1 scopes a finite format packet for each affected handoff: the container/header,
ownership-record kinds and claim fields, logical/dragline identity structure and
generation relationships, payload descriptor, envelope shape, schema/envelope
identities, field encodings and canonical identity inputs, format-version and
unknown-tag behavior, and the legacy plus 0.5.x inputs needed by migration tooling.
The scope follows [C3.4](../spec/pardosa-1.0.md#c34--invariant),
[C3.11](../spec/pardosa-1.0.md#c311--invariant),
[C4.13](../spec/pardosa-1.0.md#c413--surface),
[C4.14](../spec/pardosa-1.0.md#c414--invariant),
[C6.22](../spec/pardosa-1.0.md#c622--invariant),
[C6.29](../spec/pardosa-1.0.md#c629--invariant),
[C6.43](../spec/pardosa-1.0.md#c643--surface) and
[C10.3](../spec/pardosa-1.0.md#c103--invariant), plus T3's migration charter.
Inventory required fields, meanings, ordering/identity inputs, accepted versions and
missing decisions before assigning bytes. Exact widths, byte order, layout, tags,
canonicalization rules and legacy input coverage remain future authorized design;
this packet selects none. Timestamp placement and the 2.0 lagging-reader tension
remain questions, with no sixth standard-owned envelope field.

Only after the relevant meanings and format choices are established through the
authorized carriers, realize the following bounded evidence:

| Evidence | Acceptance and ownership |
| --- | --- |
| Golden vectors and roundtrip | M2 checks exact bytes and decoded values against the authorized format for each admitted field/constructor and representative composition; recomputes identities from the declared canonical inputs. Roundtrip alone is insufficient because encoder and decoder may share a defect. |
| Independent decode | M2 checks representative descriptors, headers and records with an independent decoder, including non-Rust interpretation without parsing Rust. Recorded values and identities agree with the golden vectors. No unimplemented language-support guarantee follows. |
| Malformed input | M2 exercises truncation, corruption, invalid lengths, discriminants and bounds against established validity rules and typed outcomes. Unknown tags reject loudly; no skip/preserve behavior or numeric limit is invented by a fixture. |
| Identity and compatibility | M2 checks local descriptor/envelope identity comparison, recorded version and canonical identity inputs. M3 tests schema-versus-envelope mismatch separation, missing-descriptor admission and ordinary read refusals; M4/M5 instantiate the applicable vectors and C8.2 checks on actual adapter storage. |
| Migration inputs | M6 exercises each explicitly supported legacy/0.5.x input class through the chartered migration path, with a docs pointer and explicit broken-chain election where required; unsupported input is not silently treated as supported. M7 aggregates this with both adapters' applicable evidence. |

Each vector/case records its semantic source, authorized format decision and expected
result. An unresolved meaning or carrier blocks the affected fixture/handoff; tests
do not become new normative definitions. No separate format sidecar is authorized
to cross the wall by naming this packet. Golden vectors, independent decoding and
malformed-input coverage are planned evidence, not tests executed by this mission.

### Type construction and semantic completion

Under [C6.23](../spec/pardosa-1.0.md#c623--surface), complete each admitted
constructor's membership, value domain, widths, bounds and units, emptiness,
parameter admissibility and supported composition. Families are not full meanings.
Parameterized bounds and finite composition are not a ban on infinite value spaces.
The prototype catalogue, float families and arbitrary application predicates are
not adopted. Material family/constraint changes or loss of required domain
expressiveness return to HUMAN. Complete definitions precede **every publication**.

For each constrained type, inventory public fields/literals, constructors/builders,
`Default`, conversions, generated code, supported decoding/deserialization and
mutation, including defining-module access. Mark absent routes explicitly; a safe
constructor does not prove decode or mutation safe. Test valid boundaries,
over-bound/empty/invalid inputs according to admitted meanings, nested enum/struct/
newtype constraint preservation, arithmetic overflow and rejected unsupported
shapes. Do not turn a custom constructor into an automatic application-predicate
guarantee. Preserve the no-public-serde 1.0 charter discipline without assuming
all internal deserialization routes absent.

[C5.22](../spec/pardosa-1.0.md#c522--surface) cursors are dragline-local and valid
by construction: prove the relevant caller boundary cannot construct/mix an invalid
cursor, including alternate construction routes; exercise regeneration on migration.
**Do not design a runtime foreign-cursor rejection test or add a cursor error.**
[C6.7](../spec/pardosa-1.0.md#c67--surface) and
[C6.8](../spec/pardosa-1.0.md#c68--surface) require constrained independent facts,
not an unrestricted product or a mandatory complete combination table.

[C4.24](../spec/pardosa-1.0.md#c424--surface),
[C6.22](../spec/pardosa-1.0.md#c622--invariant) and
[C6.35](../spec/pardosa-1.0.md#c635--invariant) keep producer assurance, acyclic
structure and faithful description separate. The derive cycle fixture is compile-fail
debt, not a facade conformance test. Stronger sealing before 1.0 does not excuse
unmet current producer guarantees. Documentation states what actual enforcement
establishes and what it does not; shipped producer names come from the rewrite.

### Concurrency and migration failure injection

The following are future test obligations, not executed results or chosen recovery
algorithms. Each fixture names its owning clauses and distinguishes failure,
indeterminate outcome and eligible qualified success.

| Injection / observation | Required evidence |
| --- | --- |
| Concurrent create; interruption before claim; record-only/data-only pair | One exclusive creator; unseeded record ordinarily claimable; ordered creation can complete; missing record prohibits writes but eligible read is unowned/generation-unknown. No auto-repair or silent create-on-open (C5.7–10, C5.62, C5.64). |
| Overlapping writers, takeover, unavailable lock and weak exclusion boundary | Epoch checked where each write lands, including every whole-content durability step; session exclusion lifetime; separate refusal causes; no identity-evidence absence becoming death or disabling the fence (C5.4–7, C5.13–14, C5.65, C8.1). |
| Unreadable ownership at fence; ambiguous acknowledgement | Refusal retains caller events and unchanged artefact where promised. Permitted resubmission requires established current authority and prior landing, never known landed events again; stale session cannot retry; no automatic retry/deduplication invented (C5.12, C5.16, C12.4). |
| Crashes at chase, freeze, target completion, each generation-record write and authority transfer | Successful cutover includes permanent source retirement, rejecting old and newly acquired source writers. Interrupted/uncertain cutover blocks renewed ordinary source/target writes until authority is established, potentially indefinitely; operator assertion and absent metadata are insufficient. Valid chase/manager writes retain their own authority rules (C5.3, C5.63, C6.16–17). |
| Read partial target; absent or one-sided pointers; superseded generation | Read named artefact without redirect; independently expose integrity, policy-relative completeness and authority, including genuine unknowns. Pointer presence proves target completeness, not authority; interruption/absent pointer proves neither completeness extreme (C5.15, C6.8, C6.15–17). |
| Policy/transformation combinations and transformation refusal | Application owns mapping/upcasting/admissibility; closure sees payload only. Keep/Purge/LockAndPrune and named rescue are distinct; no schema advancement check. Verify output-clean erasure, re-identification, dense genesis chaining and retained pair order; no claim about disposal of source or underlying media (C3.1–2, C5.20/24/50, C6.18–20, C8.3, C12.2). |
| Chain break, wrong fiber/out-of-range precursor, schema/envelope mismatch, missing descriptor | Ordinary paths refuse rather than deliver beyond break; genesis normal; no validation opt-out. Only explicit call-site migration election enters broken history; record reader's schema-identity carve-out is not an envelope/integrity waiver (C5.27–28/45/51–52, C6.12/33–34). |
| Unanchored history, anchor observation, generation boundary | Distinguish unanchored from invalid; precursor link integrity versus rolling self-consistency versus elected rewrite evidence. No completeness/provenance/cross-generation claim; external re-anchoring duty is stated, not automatically checked (C5.26/29/40, C6.30). |

### Resource and capability evidence

No capacity constant is selected here. M0 identifies measurement questions; M1
establishes any user-visible choices through authorized carriers; M2–M7 measure and
verify actual ownership. Contracts cover per request, connection/worker, artefact
and aggregate process, during admission, active work, error, cancellation and shutdown.
Account separately for queued items and bytes, waiting producers, active tasks,
retries and completed results; include retained capacity and shared backing storage.
Compose concurrent units rather than treating a bounded channel as a process bound.

For ingestion, decode, descriptor traversal, buffering, durability replacement and
migration chase, discover limits in bytes/items/tasks/attempts/depth/time, acquisition
and release sites, and the overload outcome (reject, bounded wait, drop, disconnect
or degrade as authorized). Use checked accounting and retain charges for actual
resource lifetimes. State what waits, its deadline, and retained data; indefinite
authority uncertainty does not justify unbounded queued work. Cancellation is not
rollback of external effects. Shutdown stops admission and supervises draining or
cancellation of owned work, including blocking work. Bound work units and scheduling
batches, not the service's lifetime; immediately-ready awaits do not prove fairness.

Evidence includes at/over-limit inputs, stalled consumers, concurrent producers,
retry exhaustion, cancellation at partial I/O, shutdown and arithmetic overflow.
Record high-water values with build, date, workload, concurrency and machine state.
Exclude or separately account for allocator/runtime/dependency allocations, stacks
and kernel memory; finite measurements are not universal process bounds. No
no-allocation claim, ban on allocation/recursion or invented performance guarantee.

[C6.31](../spec/pardosa-1.0.md#c631--invariant) names writer platforms; test the
stated list and actual required exclusion capability, without minting a negative
platform list. Read-only availability remains conditional on building, including
`zstd-sys` prerequisites; WASI is unverified, neither supported nor rejected by
inference. Missing safety mechanism is refusal; missing identity evidence alone is
indeterminate, not loss of writer capability. Every supported adapter receives the
same promises, with separate mechanism-specific proof. Test default, no-default,
`uuid`, `nats` and appropriate combined feature configurations under
[C4.22](../spec/pardosa-1.0.md#c422--surface); no backend feature gates vocabulary.
Investigate `block_on` reentrancy and cost under the existing conditional
affordability charter, recording rationale without inventing an async spec clause.

## Complete transfer matrix

These are planning pointers to existing carriers, not completed deliveries. Linked
clauses identify surviving normative limbs; **CHARTER**, **LAPSED** and **OUTSIDE**
are not promoted into clauses merely to fill a cell. R numbers reference the old
map's ruling register, never clean implementation instructions.

| Entry | Existing carrier and retained obligation | Clause / boundary anchor | Milestone and timing |
| --- | --- | --- | --- |
| T1 | CHARTER R46: metastream bytes/wire, separate from seven claim-field meanings. | [C3.4](../spec/pardosa-1.0.md#c34--invariant), [C6.43](../spec/pardosa-1.0.md#c643--surface) | M0–M2; sufficient before affected handoff. |
| T2 | CHARTER format/version and unresolved 2.0 lagging-reader tension; SPEC typed unknown-tag and loud older-reader rejection. | [C4.13](../spec/pardosa-1.0.md#c413--surface), [C3.11](../spec/pardosa-1.0.md#c311--invariant) | M0–M2, M7; no invented forward-skip behavior. |
| T3 | CHARTER R217: format, implementation-grade behavior, migration tool, criteria and wall; legacy plus 0.5.x migration responsibility; R128 once-at-rebuild sufficiency. | [C5.2](../spec/pardosa-1.0.md#c52--invariant), [C8.2](../spec/pardosa-1.0.md#c82--invariant) is distinct structural proof | M0/M1 every affected handoff; M6 tooling; M7 rebuild sufficiency; M8 release. |
| T4 | CHARTER D7 stronger derive seal, mechanism unchosen; current producer outcomes and S5 survive. | [C4.24](../spec/pardosa-1.0.md#c424--surface), [C6.35](../spec/pardosa-1.0.md#c635--invariant) | M2 current obligations; M9 stronger seal before 1.0. |
| T5 | SUITE R18/25/28 file predicates: exclusive creation, File::lock and epoch re-verification; R237–241 internal acquisition shape, method-less marker/orthogonal seals. Not universal conformance definitions. | [C5.4](../spec/pardosa-1.0.md#c54--invariant), [C5.64](../spec/pardosa-1.0.md#c564--invariant), [C5.65](../spec/pardosa-1.0.md#c565--invariant), [C8.1](../spec/pardosa-1.0.md#c81--invariant) | M1 carrier reconciliation; M3/M4/M7; gates first release, not deferred to 1.0. |
| T6 | SUITE both backends; existing/reference tests establish no rewrite delivery. | [C5.33](../spec/pardosa-1.0.md#c533--invariant), [C5.34](../spec/pardosa-1.0.md#c534--invariant), [C5.58](../spec/pardosa-1.0.md#c558--invariant), [C6.44](../spec/pardosa-1.0.md#c644--surface) | M4–M8, from first release. |
| T7 | CHARTER B6 timestamp choice remains open, neither removal nor retention implied; no sixth standard-owned envelope field. | [C4.19](../spec/pardosa-1.0.md#c419--surface) | M0/M1 format question, before affected encoding. |
| T8 | LAPSED R219: PGN-0006 F2 cycle-break prohibition expired, no continuing force or test. | No normative carrier | M0 disposition retained; never a completion gate. |
| T9 | SPEC public-library missing-docs enforcement, not tooling Rust in this repository. | [C8.4](../spec/pardosa-1.0.md#c84--invariant) | M2 onward; M8 first-release build proof. |
| T10 | REDUNDANT-to-positive-SPEC strict create/open and ordered creation; chartered legacy migration tool and docs pointer, no inspector/repair product. | [C5.62](../spec/pardosa-1.0.md#c562--surface), [C12.3](../spec/pardosa-1.0.md#c123--invariant), [C5.10](../spec/pardosa-1.0.md#c510--invariant) | M3/M4/M6/M8; legacy acceptance not ordinary major-line open. |
| T11 | CHARTER/process HUMAN .86: public dragline documentation cites definition; standing PAR judgement per release, no new gate or normative citation guarantee. | [Authoring note](../spec/pardosa-1.0.md#documentation-authoring--non-normative-process-note), [C2.1](../spec/pardosa-1.0.md#c21--invariant) | M8 and every release. |
| T12 | SUITE pairwise migration order, replay determinism, exclusion, dense re-chaining, precursor integrity, frontier consistency and mandatory validation. Consumer reliance refusal is not suite-assertable. | [C5.24](../spec/pardosa-1.0.md#c524--invariant), [C3.8](../spec/pardosa-1.0.md#c38--invariant), [C5.3](../spec/pardosa-1.0.md#c53--invariant), [C6.20](../spec/pardosa-1.0.md#c620--invariant), [C5.40](../spec/pardosa-1.0.md#c540--invariant), [C5.26](../spec/pardosa-1.0.md#c526--invariant), [C5.27](../spec/pardosa-1.0.md#c527--invariant), [C5.39](../spec/pardosa-1.0.md#c539--invariant) | M3–M8, from 0.5.1. |
| T13 | REDUNDANT-to-positive-SPEC: fence has no disabling setting; no prototype deletion instruction. | [C5.4](../spec/pardosa-1.0.md#c54--invariant) | M3–M8; preserve frozen surface restriction at M9. |
| T14 | REDUNDANT-to-positive-SPEC: one strict walk, mandatory validation, explicit call-site election for broken-chain migration; iter() rustdoc name. | [C6.12](../spec/pardosa-1.0.md#c612--surface), [C5.27](../spec/pardosa-1.0.md#c527--invariant), [C5.28](../spec/pardosa-1.0.md#c528--invariant) | M3/M6/M8; migration election is actual construction debt. |
| T15 | CHARTER redesign/block_on realization and ADR rationale; conditional affordability, no async clause from R112. | [C2.1](../spec/pardosa-1.0.md#c21--invariant) ordering constraint only | M0 discovery, M3/M5 cost/proof, M8 truthful docs. |
| T16 | CHARTER publishing and promotion after production validation; narrative judgement, not fog-signal gate; lockstep, licenses and graph MSRV. | [C5.37](../spec/pardosa-1.0.md#c537--invariant), [C10.4](../spec/pardosa-1.0.md#c104--invariant), [C4.23](../spec/pardosa-1.0.md#c423--invariant) | M8 release; M9 promotion only after 0.5.x use. |
| T17 | OUTSIDE test-only public scaffolding; published unstable-test-support exception survives. | [C4.22](../spec/pardosa-1.0.md#c422--surface), [C5.34](../spec/pardosa-1.0.md#c534--invariant) | Outside build scope except M7/M8 exception delivery. |
| T18 | OUTSIDE downstream gh-report adoption; production evidence does not authorize sibling migration. | No library clause carrier | Separate consumer effort, not M8 prerequisite execution. |
| T19 | OUTSIDE immutable foreign PGN edits; new PAR-side supersession only. | No library clause carrier | M8/M9 rationale records only; no foreign edits. |
| T20 | OUTSIDE/post-1.0 reader CLI as a second product; read capability and migration tool are not deferred. | [C6.14](../spec/pardosa-1.0.md#c614--invariant), [C6.18](../spec/pardosa-1.0.md#c618--surface) surviving capabilities | CLI separate later effort; M3–M8 surviving limbs. |
| S1 | Existing transfer planning .94: constrained independent facts, explicit unknowns, invariant exclusions and exhaustive alternatives; no full-product mandate. | [C6.7](../spec/pardosa-1.0.md#c67--surface), [C6.8](../spec/pardosa-1.0.md#c68--surface), [C6.15](../spec/pardosa-1.0.md#c615--invariant) | M0 proof inventory; M3/M6/M7 before publication. |
| S2 | Existing transfer proof/recovery .90/.87: actual authority and prior landing must be established, not merely represented. | [C5.12](../spec/pardosa-1.0.md#c512--invariant), [C5.16](../spec/pardosa-1.0.md#c516--invariant), [C5.63](../spec/pardosa-1.0.md#c563--invariant) | M0/M1 design inputs; M3/M6/M7 runtime proof. |
| S3 | HUMAN .91 bounded semantic completion: exact members/domains/widths/bounds/units/emptiness/parameters, composition and supported construction/decode preservation. No catalogue adoption; material change/domain loss escalates. Encoding, derive and verification remain owed. | [C6.23](../spec/pardosa-1.0.md#c623--surface), [C8.2](../spec/pardosa-1.0.md#c82--invariant) structural proof distinct from R128 sufficiency | M0 discovery; M1/M2 realization; complete definitions before EVERY release from M8 onward. |
| S4 | R223 derive cycle compile-fail fixture, not facade suite. | [C6.22](../spec/pardosa-1.0.md#c622--invariant) | M2 and regression runs thereafter. |
| S5 | R183 truthful current seal docs under T4, no fabricated enforcement or faithfulness claim. | [C4.24](../spec/pardosa-1.0.md#c424--surface), [C6.35](../spec/pardosa-1.0.md#c635--invariant) | M2/M8 every release; M9 stronger seal is separate. |
| S6 | R75 dependency-by-dependency MSRV proof; Pardosa 1.89.0 is not graph verification. | [C4.23](../spec/pardosa-1.0.md#c423--invariant) | M8 before publication, repeat on graph change. |
| S7 | R210/215 iter(), anchor_route and substrate terminology in rustdoc, not new clauses for identifiers. | [C8.4](../spec/pardosa-1.0.md#c84--invariant), [C6.12](../spec/pardosa-1.0.md#c612--surface) | M2/M3/M8 docs delivery. |
| S8 | OUTSIDE HUMAN .97: application mapping/upcasting/admissibility; no Pardosa advancement check or mapping API. Transformation/read-safety and declared version metadata survive. | [C8.3](../spec/pardosa-1.0.md#c83--invariant), [C6.18](../spec/pardosa-1.0.md#c618--surface), [C6.26](../spec/pardosa-1.0.md#c626--invariant), [C6.27](../spec/pardosa-1.0.md#c627--invariant) | M6/M7 preserve boundary; application implementation outside. |
| S9 | Existing charter/Notes: public-surface gate from 0.5.1; semver gate at 1.0; R143 polarity and unstable-feature exception. Current repo has no CI delivery of these. | [C5.30](../spec/pardosa-1.0.md#c530--invariant), [C4.5](../spec/pardosa-1.0.md#c45--surface), [C4.22](../spec/pardosa-1.0.md#c422--surface) | M8 surface policy; M9 compatibility freeze. |
| S10 | R182 actual shipped hand-written descriptor producer names; no historical inventory substitution. | [C4.24](../spec/pardosa-1.0.md#c424--surface) | M2/M8 actual inventory, fixed set at M9. |

Interior-hole retention and repeated same-generation detachment findings
(`pardosa-jn1.92/.93`) remain bounded **NO-WITNESS**, neither permission nor
prohibition; only an actual governing witness reopens them. Unsupported continuing
refresh remains retired, not a new no-refresh guarantee. Permanent snapshotting
refusal and application-owned policy are not reopened by migration implementation.

## Release, documentation and tooling duties

M8 verifies the actual three publish crates under
[C6.42](../spec/pardosa-1.0.md#c642--surface) and five modules under
[C4.20](../spec/pardosa-1.0.md#c420--surface), with migration manager inside
`pardosa`. No speculative layout, dependency pin or extra public API is selected.
Verify archives, lockstep versions/caret sibling ranges, feature defaults/additivity,
full license texts and dependency graph compiler floor. Record actual graph and
toolchain evidence, not this plan's mention of 1.89.0 as a passed check.

Publish docs.rs orientation separately from normative prose: Pardosa-native positive
definitions, per-condition remedies, supported construction and derive diagnostics,
current seal limits, distinct integrity claims, feature/platform/build conditions,
read/migration usage, legacy migration pointer and major-line read limits. Record
dragline-definition citations and landing-page judgement per release in one standing
PAR ADR edited in place, without a new automated judgement gate. Include the
one-maintainer/best-effort disclosure, security contact/process without SLA,
non-Rust-edge advisory monitoring and correctness/safety-only withdrawal posture
([C5.38](../spec/pardosa-1.0.md#c538--invariant),
[C6.32](../spec/pardosa-1.0.md#c632--invariant)).

The future public-surface gate checks backend type/path leaks (including wrappers
and cause chains), permits diagnostic strings, and checks enum attribute polarity
against actual open-domain designations. It is distinct from compatibility diffing.
Preserve named construction and closed struct fields, exhaustive enums with no
catch-all, no public serde at 1.0 and the unstable-test-support exception. Prove
future guards with plant → fail → revert → clean, including wrapper and unearned
attribute cases; no implementation or CI gate is added in this document mission.

At M9, cargo-semver-checks blocks against the prior release tag with adequate fetch
history, explicit stable-feature allow-list and unstable-test-support structurally
excluded. Tool inability to complete is failure, not compatibility success. Retain
the known feature-subset blind spot in the adopting PAR ADR; do not convert a broad
feature run into evidence for every subset. No advisory 0.5.x semver run or committed
cargo-public-api compatibility snapshot is newly authorized. ADR setup/governance
and any future release CI belong to separately scoped delivery, not to this repo's
current documentary gate. Foreign PGNs stay untouched; rationale is on the PAR side.

## What counts as done

**Written plan complete:** all thirty source rows have carriers, anchors where applicable,
timing and milestone owners; question map has verified children/dependencies; one
authored document, no implementation or source-spec edits. Independent commander
review APPROVE is recorded on `pardosa-j48a.1` (2026-09-07). This establishes
documentary completion, not clean-handoff, implementation or publication readiness.

**Clean rebuild sufficiency:** once-at-rebuild R128 evidence establishes that the
authorized clean inputs suffice, including adequate semantic meanings and format.
It is not C8.2's repeating structural-completeness check, nor an assertion that a
reference-backed implementation happened to pass tests. Every affected dispatch
still passes the pre-handoff gate; every release still carries complete definitions
and applicable conformance/release evidence.

**0.5.1:** M1–M8 accepted with actual implementation, both-backend conformance,
semantic completion, supported-route proofs, migration/rescue, resources, docs,
format and publication evidence. Stronger pre-1.0 seal remains explicit future work
only insofar as all current binding producer guarantees already hold. No unrun
backend test, unverified platform or pending dependency proof is a green result.

**1.0:** M9 plus retained release evidence; named production experience informs
the narrative decision, final surface and stronger seal. Fog signals about external
importers of encoding/file, usefulness of unstable support or framework wording
remain observations, not normative promotion gates or permission to weaken non-goals.

### Checks for this document mission

Run `git diff --check` (inner), `./scripts/check.sh` (mid), and
`git status --short` (boundary). Supplement with link/anchor and exact T1–T20/S1–S10
row checks, and live map parentage/readiness/dependency inspection. The script is
this repository's sole local gate and checks **spec/trace consistency, not plan
certification**. No implementation E2E verification is specified or performed;
documentary checks only. No push, sync, implementation execution, spec/trace edits,
dependency changes or sibling access is authorized. Preserve the pre-existing
`.beads/interactions.jsonl` modification unstaged. Rollback removes only this newly
authored plan and retires only erroneous newly created scaffolding with a reason.
