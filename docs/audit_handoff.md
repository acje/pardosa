# Pardosa v0.5.5 Senior Engineering Audit Handoff

> **STATUS: NOT APPROVED / IMPLEMENTATION INCOMPLETE**  
> **Linus Round 8 Verdict (`pardosa-lvdk`)**: `Standard: NEEDS WORK / Security: FAIL` (`Critical=0, High=0, Medium=6, Low=0, Info=1`).  
> While all historical High-severity defect classes (`R4-H1`, `R4-H2`, `R4-H3`, `R5-H1`, `R5-H2`, `R6-H1`) have been resolved, repeated Medium-severity defect classes (`M4` public type binding / count overflow, `M1` typed NATS error classification, `M10`/`M11` guard proof & TDD chronology, `M9` resource contracts) remain open.

---

## 1. Candidate Identity & Commit Ranges

| Repository | Baseline Commit | Candidate Commit | Git Status |
|---|---|---|---|
| **`pardosa`** | `c9437c5` | `d386fae` | 13 commits ahead of `origin/main` (source clean; `.beads/interactions.jsonl` dirty) |
| **`gh-report`** | `a760d6f` | `868d38b` | 4 commits ahead of `origin/main` (source clean; pre-existing tools/backups untracked) |

### Key Candidate Commits in `pardosa`
- `acd6a50`: Fix `TransportUnavailable` mapping in Cleanroom Spec & Store.
- `84efa2b`: Fix reader corruption isolation and recovery bounds (`take(file_len)` & snapshot `(first, last)`).
- `c837595`: Batch landing initial implementation (superseded by `9ad1a0e` and `668ed95`).
- `9ad1a0e`: Truthful batch receipts, file durability on sync, and drained NATS futures.
- `668ed95`: Enforce sound contiguous prefix batch semantics and sequential engines.
- `55a10fb`: Inherit sequential block write+sync in FileEngine (resolving `R6-H1`) and enforce batch partition consistency (resolving `M4` Store boundary).
- `d386fae`: Make batch outcome counts intrinsic on `BatchLandingVerdict`, add suffix partition guard test (`M11`), map single-event NATS initiation to `TransportUnavailable` (`M1`), and remove 32-bit cast narrowing (`L6`).

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
