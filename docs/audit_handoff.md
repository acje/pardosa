# Pardosa v0.5.5 Senior Engineering Audit Handoff

## 1. Candidate Identity & Commit Ranges

| Repository | Baseline Commit | Candidate Commit | Git Status |
|---|---|---|---|
| **`pardosa`** | `c9437c5` | `55a10fb` | 8 commits ahead of `origin/main` (local worktree clean) |
| **`gh-report`** | `a760d6f` | `868d38b` | 4 commits ahead of `origin/main` (local worktree clean) |

### Key Candidate Commits in `pardosa`
- `acd6a50`: Fix `TransportUnavailable` mapping in Cleanroom Spec & Store.
- `84efa2b`: Fix reader corruption isolation and recovery bounds (`take(file_len)` & snapshot `(first, last)`).
- `c837595`: Batch landing initial implementation (superseded by `9ad1a0e` and `668ed95`).
- `9ad1a0e`: Truthful batch receipts, file durability on sync, and drained NATS futures.
- `668ed95`: Enforce sound contiguous prefix batch semantics and sequential engines.
- `55a10fb`: Inherit sequential block write+sync in FileEngine (resolving `R6-H1`) and enforce batch partition consistency (resolving `M4` Store boundary).

---

## 2. Release Scope vs. Deferred Scope

### In v0.5.5 Candidate Release Scope
1. **Synchronous Storage Primitives**: `append_block` and `append_to_fiber` returning discrete `WriteLandingVerdict<T>` (`Landed` vs `Undetermined`) and typed `OperationFailure` per C5.16 and PGN-0010:R5.
2. **Sequential Batch Convenience**: `append_batch` and `append_batch_envelopes_detailed` returning sound `BatchLandingVerdict<T>` (`LandedAll`, `PreAttemptRefusal`, `PartialProgress`). Commits strictly the contiguous landed prefix `0..landed_count` to in-memory `fiber_index` and `rolling_commitment`.
3. **Durability Truthfulness**:
   - `FileEngine`: Each block is written and synchronized via `append_block`. On sync failure, halts before subsequent blocks; zero unattempted items touch disk.
   - `NatsEngine`: Sequential single-message execution with OCC expected sequence matching (`expected_last_subject_sequence`). Sequence overflow checked pre-attempt at `u64::MAX`.
4. **Error Taxonomy & Closed Enum Doctrine**:
   - Spec C4.5 and C4.6 closed enum doctrine strictly upheld (no `#[non_exhaustive]` on public failure enums).
   - `TransportUnavailable` condition separates transient network/transport errors from positive precursor corruption (`PrecursorChainBroken`).
   - NATS error 10070 separated from missing data; filesystem metadata I/O errors and file seek errors map to `TransportUnavailable`.
5. **Reader Integrity & Corruption Preservation**:
   - Reader corruption decoupled from consumer callback aborts (`M3`).
   - Session index refuses operations when reader retains broken state (`M14`).
   - Discovered precursor chain corruption on reader recovery survives subsequent transport errors (`R4-M2`).
6. **Recovery & Snapshot Bounds**:
   - Bounded recovery reading: `take(file_len)` in `FileEngine`; operation-local immutable snapshot `(first, last)` in `NatsEngine`.
7. **Downstream Verification**:
   - Downstream integration verified in `gh-report`: 1,366 / 1,366 tests pass cleanly against candidate `pardosa`.

### Deferred Post-v0.5.5
1. **NATS In-Flight Pipelining**: 64-item concurrent pipelined publishing deferred due to multi-future timeout and drainage complexity.
2. **TigerBeetle Queue Coalescing**: Demand-driven queue coalescing deferred due to background worker state-machine complexity.
3. **NATS ADR-50 Atomic Publishing**: Requires NATS Server v2.12+/v2.14+ streams with `AllowAtomicPublish=true` and custom header mapping in `async-nats`.
4. **Unconstrained Per-Item Receipt Vectors**: Fine-grained per-item receipt vectors deferred in favor of sound contiguous prefix progress.
5. **1M Event Performance Targets**: Exploratory 1M stress runs deferred; standard integration suite focuses on correctness and fault injection at modest volumes.

---

## 3. Independent Linus Review Audit Status

### Round 7 Review Verdict: `pardosa-0i3j`
- **Verdict**: `Critical = 0, High = 0, Medium = 5, Low = 2, Info = 1`.
- **Quality Gates**: `check.sh`: PASS:0 (9/9) | `cargo clippy`: PASS:0 (0 warnings) | `cargo test`: PASS:0 (288/288) | `comment-free`: PASS:0.

### High-Severity Findings Resolution Ledger
- `R4-H1` / `R4-H2`: RESOLVED. Sequential NATS execution eliminates in-flight pipelining ambiguity and drains all sent futures.
- `R4-H3`: RESOLVED. Written blocks are never reported `Landed` unless `sync_data()` succeeds.
- `R5-H1`: RESOLVED. `Store::append_batch_detailed` commits strictly the contiguous landed prefix `0..landed_count`, preserving causal hash chain integrity.
- `R5-H2`: RESOLVED. Compatibility wrappers exhaustively handle `PartialProgress`; false success on unattempted batches is eliminated.
- `R5-L1`: WITHDRAWN. Linus confirmed spec C4.5/C4.6 closed enum doctrine; no `#[non_exhaustive]` on domain enums.
- `R6-H1`: RESOLVED. `FileEngine` inherits sequential `append_block` write+sync; zero unattempted items touch disk on sync failure.

### Disclosed Residual Findings (Medium/Low)
1. `M4` (Medium): While Store validates `landed_count + 1 + unattempted_count == batch_len` at runtime, the public `BatchLandingVerdict` enum has independent `usize` fields constructible outside Store.
2. `M1` (Medium): NATS single-event publish initiation and raw recovery error mappings still retain diagnostic string scanning rather than pure typed enum matches across all error paths.
3. `M10` / `M11` (Medium): Formal four-step guard proofs (plant/fail/revert/clean) and historical TDD chronology were not retained for every incremental branch.
4. `M9` (Medium): In-memory caller batch buffers (cloned block bytes, decoded envelopes) do not carry an explicit application-level byte quota.
5. `L6` (Low): Single-block fault injection thresholds cast `frame_count as usize` on 32-bit targets.

---

## 4. Packaging, Topo Ordering & Registry Verification

### Registry Absence Verified
- `cargo search pardosa`: empty.
- `cargo search pardosa-derive`: empty.
- `cargo search pardosa-nats`: empty.
- Neither package currently exists on crates.io; this is an initial 0.5.5 publication.

### Topological Publication Order
1. **`pardosa-derive` (v0.5.5)**: Leaf proc-macro crate. Packages and verifies cleanly (`cargo package -p pardosa-derive` exit 0).
2. **`pardosa` (v0.5.5)**: Declares `pardosa-derive = { version = "0.5.5", path = "../pardosa-derive" }`. Cannot be packaged until `pardosa-derive` is indexed on crates.io.
3. **`pardosa-nats` (v0.5.5)**: Declares `pardosa = { version = "0.5.5", path = "../pardosa" }`. Cannot be packaged until `pardosa` is indexed on crates.io.

### MSRV Verification Disclosure
- Manifests declare `rust-version = "1.89.0"`.
- Pinned workspace toolchain is `1.98.0`.
- Execution on actual rustc 1.89.0 has not been performed locally (oldest installed toolchain is 1.95.0).
- True MSRV remains an open verification item prior to registry publication.

---

## 5. Downstream Verification Commands (`gh-report`)

To independently verify the candidate `pardosa` against `gh-report` without committing sibling path dependencies:

```bash
cd /Users/anders.jensen/Documents/github/Mattilsynet/gh-report
cargo test -p gh-report \
  --config 'patch."https://github.com/acje/pardosa".pardosa.path="/Users/anders.jensen/Documents/github/Mattilsynet/pardosa/crates/pardosa"' \
  --config 'patch."https://github.com/acje/pardosa".pardosa-nats.path="/Users/anders.jensen/Documents/github/Mattilsynet/pardosa/crates/pardosa-nats"'
```

All 1,366 unit, integration, and doctests pass cleanly (exit 0).
