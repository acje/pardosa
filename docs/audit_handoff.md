# Pardosa v0.5.5 Senior Engineering Audit Handoff

> **STATUS: DEPLOYED BUT NOT INDEPENDENTLY APPROVED**  
> **Linus Round 10 Verdict (`pardosa-ev4t`)**: `Standard: NEEDS WORK / Security: FAIL` (`Critical=0, High=0, Medium=4, Low=1, Info=1`).  
> **Gate Violation Disclosure**: Deployed to production under explicit user operational authorization to establish live feedback on fresh stream namespace `v21`, but the final independent Linus review gate was NOT approved.
> While typed NATS error mappings (`R9-M1a`, `R9-M1b`) and Store batch length validation (`R8-M1`) were implemented and tested, Linus flagged carried Medium issues: `M4` (public raw DTO vs. validated domain outcome conflation), `M9` (resource contract documentation gap), `M10` (absence of historical TDD execution ledger), and `M11` (unrecorded formal 4-step mutation proof ledger in review request).

---

## 1. Candidate Identity, Commits & Production Deployments

| Repository | Branch | Tag | Head Commit | Production Status |
|---|---|---|---|---|
| **`pardosa`** | `main` | - | `d1fd21a` | Pushed to `https://github.com/acje/pardosa.git` |
| **`gh-report`** | `main` | `v0.1.81` | `199ed8f` (tagged) / `aa94fed` (`main`) | Built to GAR and deployed to Cloud Run |

### Live Deployment Facts (2026-09-12)
- **Cloud Run Service**: `ghreport` in region `europe-north1`, project `ghreport-d302`.
- **Active Revision**: `ghreport-00102-2ln` (built from `v0.1.81` commit `199ed8f`).
- **Container Image Digest**:
  `europe-north1-docker.pkg.dev/artifacts-352708/stabsec/gh-report@sha256:02d8084b3669e8bfe5a1aa2ac5bf37ab7d8071de1eba597a712bdbdc9e98c2de`
- **Traffic Routing**: 100% routed to `ghreport-00102-2ln` via workflow run `34707037320` (`Cutover Traffic`).
- **Previous Revision**: `ghreport-00101-bzs` (v0.1.80 on `v20`) serving 0% traffic; preserved for instant rollback if needed.
- **Service Ingress**: Restricted behind Google IAP / Cloud Load Balancer (`https://ghreport-nbd4qrr5oa-lz.a.run.app` returns 403 to unauthenticated requests).

### Live NATS JetStream State (connect.nats.mattilsynet.io:4222)
- **Fresh `v21` Namespace**:
  - `gh-report-org_4d617474696c73796e6574-v21_data`: 654 messages (427 KiB). First sequence 1, last sequence 654 written at 19:07:58 UTC. Initial collection sweep completed.
  - `gh-report-org_4d617474696c73796e6574-v21-team-team_data`: 38 messages (16 KiB).
  - `gh-report-org_4d617474696c73796e6574-v21-org-org_data`: 1 message (151 B).
  - `gh-report-org_4d617474696c73796e6574-v21_meta`: 2 messages (366 B).
- **Archival `v20` Namespace (Untouched & Preserved)**:
  - `gh-report-org_4d617474696c73796e6574-v20_data`: 812 messages (532 KiB). Zero messages purged, mutated, or deleted.
  - `gh-report-org_4d617474696c73796e6574-v20_meta`: 2 messages (366 B).
- **Rollback Divergence Note**: Rolling back to `ghreport-00101-bzs` (`v20`) is possible, but will NOT carry the 654 events written to `v21`. Forward reconciliation would be required.

---

## 2. Key Candidate Commits in `pardosa`
- `7806125`: Replace substring matching with typed downcast checks in NatsEngine (`is_wrong_last_sequence`).
- `d1fd21a`: Resolve Linus round 9 findings:
  - Match `RawMessageErrorKind::NoMessageFound` and `JetStream(NO_MESSAGE_FOUND/SEQUENCE_NOT_FOUND)` directly (`R9-M1a`).
  - Match `GetStreamErrorKind::JetStream(STREAM_NOT_FOUND)` directly; zero Display string scanning (`R9-M1b`).
  - Enforce `LandedAll` and `PreAttemptRefusal` batch length validation in `Store::append_batch_detailed` (`R8-M1`).
  - Use `saturating_add` for `total_count()` and add `checked_total_count()` returning `Option<usize>` (`M4`).
  - Remove duplicate ErrorCode numeric literals (`R9-L1`).
  - Add unit and integration tests with guard assertions (`M10`, `M11`).

---

## 2. Release Scope vs. Deferred Scope

### In v0.5.5 Candidate Release Scope
1. **Synchronous Storage Primitives**: `append_block` and `append_to_fiber` returning discrete `WriteLandingVerdict<T>` (`Landed` vs `Undetermined`) and typed `OperationFailure` per C5.16 and PGN-0010:R5.
2. **Sequential Batch Convenience**: `append_batch` and `append_batch_envelopes_detailed` returning `BatchLandingVerdict<T>` (`LandedAll`, `PreAttemptRefusal`, `PartialProgress`). Commits strictly the contiguous landed prefix `0..landed_count` to in-memory `fiber_index` and `rolling_commitment`. *(Caveat: Store boundary validates partition counts, but public enum fields permit arbitrary external construction; M4 carried).*
3. **Durability Truthfulness**:
   - `FileEngine`: Each block is written and synchronized via `append_block`. On sync failure, halts before subsequent blocks; zero unattempted items touch disk (`R6-H1` resolved).
   - `NatsEngine`: Sequential single-message execution with OCC expected sequence matching (`expected_last_subject_sequence`). Sequence overflow checked pre-attempt at `u64::MAX`.
4. **Error Taxonomy & Closed Enum Doctrine**:
   - Spec C4.5 and C4.6 closed enum doctrine strictly upheld (no `#[non_exhaustive]` on public failure enums; breaking changes require major bump). Linus `R5-L1` finding formally withdrawn.
   - `TransportUnavailable` condition separates transient network/transport errors from positive precursor corruption (`PrecursorChainBroken`).
   - NATS error 10070 separated from missing data; filesystem metadata I/O errors and file seek errors map to `TransportUnavailable`. *(Caveat: single-event publish initiation and raw recovery error mappings still retain diagnostic string scanning; M1 carried).*
5. **Reader Integrity & Corruption Preservation**:
   - Reader corruption decoupled from consumer callback aborts (`M3`).
   - Session index refuses operations when reader retains broken state (`M14`).
   - Discovered precursor chain corruption on reader recovery is preserved across subsequent transport errors via `fiber_index.has_broken_fibers()` (`R4-M2` source-reasoned; forced schedule test unexecuted).
6. **Recovery Bounds**:
   - Bounded recovery reading: `take(file_len)` in `FileEngine` bounds the file volume read from disk (does NOT guarantee bounded process heap memory); operation-local immutable snapshot `(first, last)` in `NatsEngine`.
7. **Downstream Integration**:
   - Downstream integration verified in `gh-report` (`868d38b` against `d386fae`): 1,366 / 1,366 unit, integration, and doctests pass cleanly.
8. **Benchmark Instrumentation Cleanup**:
   - In `gh-report` (`crates/gh-report/tests/bench_projection.rs` @ `868d38b`): Removed handwritten unsafe `extern "C" fn getrusage` and hardcoded `< 60s` performance assertions. Made 100K/1M benchmarks explicit opt-ins (`BENCH_SCALE=1`, `BENCH_1M=1`). This cleans up fragile test instrumentation, not a claim that performance optimization is complete.

### Deferred Post-v0.5.5
1. **NATS In-Flight Pipelining**: 64-item concurrent pipelined publishing deferred due to multi-future timeout and drainage complexity.
2. **TigerBeetle Queue Coalescing**: Demand-driven queue coalescing deferred due to background worker state-machine complexity (not facade violation).
3. **NATS ADR-50 Atomic Publishing**: Requires NATS Server v2.12+/v2.14+ streams with `AllowAtomicPublish=true` and custom header mapping in `async-nats`.
4. **Unconstrained Per-Item Receipt Vectors**: Fine-grained per-item receipt vectors deferred in favor of sound contiguous prefix progress.
5. **1M Event Performance Targets**: Exploratory 1M stress runs deferred; standard integration suite focuses on correctness and fault injection at modest volumes.

---

## 3. Independent Linus Review Audit Status

### Round 8 Review Verdict: `pardosa-lvdk`
- **Verdict**: `Standard: NEEDS WORK / Security: FAIL` (`Critical = 0, High = 0, Medium = 6, Low = 0, Info = 1`).
- **Quality Gates**: `check.sh`: PASS:0 (9/9) | `cargo clippy`: PASS:0 (0 warnings) | `cargo test`: PASS:0 (289/289) | `comment-free`: PASS:0.

### High-Severity Findings Resolution Ledger
- `R4-H1` / `R4-H2`: RESOLVED. Sequential NATS execution eliminates in-flight pipelining ambiguity and drains all sent futures.
- `R4-H3`: RESOLVED. Written blocks are never reported `Landed` unless `sync_data()` succeeds.
- `R5-H1`: RESOLVED. `Store::append_batch_detailed` commits strictly the contiguous landed prefix `0..landed_count`, preserving causal hash chain integrity.
- `R5-H2`: RESOLVED. Compatibility wrappers exhaustively handle `PartialProgress`; false success on unattempted batches is eliminated.
- `R5-L1`: WITHDRAWN. Linus confirmed spec C4.5/C4.6 closed enum doctrine; no `#[non_exhaustive]` on domain enums.
- `R6-H1`: RESOLVED. `FileEngine` inherits sequential `append_block` write+sync; zero unattempted items touch disk on sync failure.

### Open Residual Findings (Medium)
1. `M4` (Medium): While `BatchLandingVerdict` carries intrinsic counts and Store boundary validates partition counts, public enum fields permit arbitrary external construction that could overflow `total_count()` arithmetic.
2. `M1` (Medium): NATS single-event publish initiation and raw recovery error mappings still retain diagnostic string scanning rather than pure typed enum matches across all error paths.
3. `M10` / `M11` (Medium): Formal four-step guard proofs (plant/fail/revert/clean) and historical TDD chronology were not retained for every incremental branch.
4. `M9` (Medium): In-memory caller batch buffers (cloned block bytes, decoded envelopes) do not carry an explicit application-level byte quota.

---

## 4. Packaging, Topo Ordering & Registry Verification

### Registry Absence Disclosures
- `cargo search pardosa`, `cargo search pardosa-derive`, and `cargo search pardosa-nats` return empty.
- *Caveat*: Search emptiness is non-conclusive proof of namespace availability. Exact registry API and authentication checks remain pending.

### Topological Publication Order
1. **`pardosa-derive` (v0.5.5)**: Leaf proc-macro crate. Packaged cleanly in historical dry-run (`cargo package -p pardosa-derive` exit 0).
2. **`pardosa` (v0.5.5)**: Declares `pardosa-derive = { version = "0.5.5", path = "../pardosa-derive" }`. Cannot be packaged until `pardosa-derive` is indexed on crates.io.
3. **`pardosa-nats` (v0.5.5)**: Declares `pardosa = { version = "0.5.5", path = "../pardosa" }`. Cannot be packaged until `pardosa` is indexed on crates.io.

### MSRV Verification Disclosure
- Manifests declare `rust-version = "1.89.0"`.
- Pinned workspace toolchain is `1.98.0`.
- Execution on actual rustc 1.89.0 has **NOT** been performed locally (oldest installed toolchain is 1.95.0).
- True MSRV remains an unverified open item prior to registry publication.

---

## 5. Downstream Verification Commands (`gh-report`)

To independently verify candidate commit `d386fae` against `gh-report` (`868d38b`) without committing sibling path dependencies:

```bash
cd /Users/anders.jensen/Documents/github/Mattilsynet/gh-report
cargo test -p gh-report \
  --config 'patch."https://github.com/acje/pardosa".pardosa.path="/Users/anders.jensen/Documents/github/Mattilsynet/pardosa/crates/pardosa"' \
  --config 'patch."https://github.com/acje/pardosa".pardosa-nats.path="/Users/anders.jensen/Documents/github/Mattilsynet/pardosa/crates/pardosa-nats"'
```

All 1,366 unit, integration, and doctests pass cleanly (exit 0). Recorded as candidate execution evidence for `868d38b` on `d386fae`.
