# PAR-0001. Documentation and Release Judgement

Date: 2026-09-10
Last-reviewed: 2026-09-10
Tier: B
Status: Accepted
Crates: pardosa, pardosa-derive, pardosa-nats

## Related

References: PAR-0001

## Context

RULED 230-232 establish the PAR ADR venue for recording standing, unmechanisable
release and documentation judgements:
- RULED 230 sites surviving unmechanisable judgements in the PAR ADR venue.
- RULED 231 sets the documentation-bar judgement as one standing ADR edited in place,
  per release, with git holding prior state.
- RULED 232 preserves record-without-a-gate where an ADR venue exists.

Milestone M8 establishes 0.5.1 release packaging, metadata, and publication readiness
per `docs/plans/pardosa-0.5.1.md` and `docs/spec/pardosa-1.0.md`. This record documents
the required release judgements for release 0.5.1.

### 1. Dragline-Definition Citation and Landing-Page Orientation Judgement

Pardosa separates normative specification prose (`docs/spec/pardosa-1.0.md`) from
consumer-facing `docs.rs` landing-page orientation.

Dragline definitions are cited per specification:
- C5.39: Pardosa contracts event ordering within one fiber and one total order per
  artefact, but contracts no cross-fiber order. Dragline multi-fiber ordering reflects
  observed behaviour, not an cross-fiber contractual guarantee.
- C6.37: An artefact's ownership record carries the identity of its own dragline and
  no other dragline; establishing which draglines form one logical identity requires
  reading each artefact's ownership record.
- C6.38: A consumer reads an event's identifier and fiber identifier from the event
  value; dragline identity is reported to an operator and is not exposed in event values.

The crate-level `lib.rs` documentation across `pardosa`, `pardosa-derive`, and
`pardosa-nats` provides high-signal orientation covering:
- Native positive definitions: binary container framing (`PARDOSA\x01`), rolling BLAKE3
  commitments, frame CRC32C, fiber lifecycle states, ownership fencing, and migration.
- Per-condition remedies: unambiguous operational guidance for `UnreadableRecord`,
  `SchemaMismatch`, `EnvelopeMismatch`, `StaleEpoch`, `OwnershipUnestablished`, and
  `TerminalFailure`.
- Supported derive shapes and diagnostics: enum roots, explicit discriminants,
  tombstones, and rejection of cyclic or unbounded types.
- Major-line read limit: artefacts are read only by the major line that wrote them (C4.21).

### 2. MSRV Dependency Graph Verification (C4.23/S6)

Per C4.23, Pardosa's compiler floor is 1.89.0. Clause C4.23 and transfer register S6
require verification against every dependency's minimum supported Rust version before
publication.

An audit of the 0.5.1 release graph (`cargo metadata`) verified all declared dependency
MSRVs:
- `async-nats 0.49.1`: msrv = 1.88.0
- `time 0.3.55`, `time-core 0.1.9`, `time-macros 0.2.32`: msrv = 1.88.0
- `icu_* 2.3.0` (`icu_provider`, `icu_normalizer`, etc.): msrv = 1.88
- `idna_adapter 1.2.2`: msrv = 1.86
- `deranged 0.5.8`, `chacha20 0.10.2`, `constant_time_eq 0.4.2`: msrv = 1.85.0
- `uuid 1.26.0`, `security-framework 3.7.0`, `zeroize 1.9.0`: msrv = 1.85.0
- `tokio 1.53.1`, `bytes 1.12.1`, `syn 2.0.119`: msrv = 1.71.0
- Remaining crates declare MSRV <= 1.71 or declare no MSRV.

The highest declared dependency MSRV is 1.88.0. Therefore, Pardosa's stated compiler
floor of `rust-version = "1.89.0"` is verified and safely clears the entire transitive
dependency graph.

### 3. Truthful Seal Status (S5) and Shipped Producer Inventory (S10)

Per C4.24, C6.35, and S5:
- Pardosa truthfully discloses the limits of descriptor verification: Pardosa establishes
  that a schema descriptor was produced by a recognized producer, but whether that
  descriptor describes event payloads faithfully at runtime stands outside what Pardosa
  establishes.
- Shipped producer inventory (S10): Exactly one producer is recognized at 0.5.1:
  `pardosa_derive::PardosaSchema`. Zero hand-written descriptor implementations are
  shipped in published crates. Admitting any further producer is an addition within
  a major line.
- Storage adapter seals (C4.9, C4.10): The storage adapter traits and backend exclusion
  obligations remain internal and sealed. Third parties establish conformance against
  public obligations (C6.44) rather than implementing external adapters at 1.0.

### 4. Security Policy and Maintenance Disclosure (C5.38, C6.32)

- Single maintainer (C6.32): Pardosa has one maintainer. Issue triage and user support
  are best-effort with no response-time SLA.
- Security reporting (C5.38): Disclosures follow GitHub Security Advisories or
  `security@pardosa.dev` without SLA.
- Non-Rust dependencies (C5.38): Non-Rust dependency edges (such as cryptographic
  assembly and compression) are monitored directly for security advisories.
- Withdrawal posture (C5.38): Releases are withdrawn (yanked) only for correctness
  or safety defects.

## Decision

R1 [28]: Release 0.5.1 establishes lockstep 0.5.1 versions across `pardosa`,
`pardosa-derive`, and `pardosa-nats` using caret ranges for sibling dependencies.

R2 [26]: Every published crate archive must bundle `LICENSE-MIT` and `LICENSE-APACHE`
together with complete crate metadata matching the 0.5.1 release baseline.

R3 [32]: Crate-level rustdoc in `lib.rs` of all three crates must provide positive
definitions, per-condition remedies, truthful seal limits, shipped producer inventories,
and security disclosures.

R4 [22]: All published crates must enforce `#![deny(missing_docs)]` across their entire
public surface from release 0.5.1 onward.

R5 [27]: Pardosa's compiler floor is fixed at `rust-version = "1.89.0"` and is verified
to satisfy every crate in the transitive dependency graph.

## Consequences

+ becomes easier: Downstream consumers and packaging tooling receive complete,
  deterministic crate archives, accurate license texts, and clear docs.rs orientation.
− becomes harder: Any dependency bump requires checking MSRV compatibility against
  the 1.89.0 floor; lockstep versioning requires synchronized releases.
risks/migration: Sibling path dependencies must specify both version and path to ensure
  clean packaging and cargo resolution.
