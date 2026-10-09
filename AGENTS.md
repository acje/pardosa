# Agent Instructions

Repo-specific operational notes. General agent/OODA doctrine, bash hygiene,
and the Rust no-`//`-comments house style live in the global
`~/.config/opencode/AGENTS.md` (auto-loaded) — not repeated here.

## Section 1: Canonical Fleet Doctrine

### OODA Loop Roles
- **Copernicus** (Observe): Raw evidence gathering from environment, code, and external specs. Pure sensor; produces no hypotheses.
- **Feynman** (Orient): Produces ranked hypotheses with falsifiers; stress-tests against concrete examples.
- **Moltke** (Decide): Standing mission commander. Emits executable mission contracts, sets intent, boundaries, and abort criteria.
- **Hopper** (Act): Executes missions using Kent Beck TDD (red-green-refactor) with verify-before-claim discipline.
- **Linus** (Review): Mandatory pre-merge Rust reviewer for idiom conformance, type safety, unsafe soundness, and supply chain.
- **Hamilton** (Assurance): Architectural alignment and assurance reviewer running during CI wait windows.
- **Gardener** (GC): Post-mission cleanup specialist; reclaims transient scaffolding and closes completed mission beads.

### Priority Hierarchy
Tradeoffs strictly resolve in this five-tier priority order:
1. **Maintainability**: Pure trunk development, small deployable increments, minimal cognitive overhead, low complexity.
2. **Correctness by design**: Make illegal states unrepresentable via types, explicit state machines, and private invariant constructors.
3. **Response times**: Latency-sensitive read paths and prompt fact propagation across boundaries.
4. **Energy efficiency in code**: Minimize redundant polling, hot loops, unnecessary serialization, and idle CPU/memory burn.
5. **Features**: New functionality ranks last and must never compromise the higher tiers.

### Non-Interactive Shell Commands & Bash Hygiene
Subagents execute non-interactively. Commands that prompt for user confirmation stall execution indefinitely.
- Always use non-interactive and force flags: `cp -f`, `rm -f`, `rm -rf`.
- Streaming and batch mode: use `--batch`, `-y`, or `--quiet` where available.
- Stream separation: machine-readable findings route to `stdout`; diagnostics and logs route to `stderr`.

### Zero Plain Comments
In Rust source (`*.rs`), plain comments (`//` or `/* */`) are forbidden.
- Rationale belongs in commit messages, ADRs, or bead descriptions.
- Use `///` or `//!` contract doc-comments only when defining public API documentation (with required `# Errors`, `# Panics`, `# Safety` sections).
- Suppress lints with `#[expect(lint, reason = "...")]` rather than plain comments.

### Doctrine: "Make tools fast to iterate fast"
Developer and verification tooling must be compiled, ultra-fast Rust binaries operating directly on ASTs and files rather than slow interpreted wrappers or token-heavy in-context simulation. Fast tools enable high-frequency local feedback loops (INNER cadence) without friction.

### Doctrine: "Zero compliance theatre"
High-assurance testing techniques—such as property-based testing (proptest), fuzzing (cargo-fuzz), formal model checking, or fault injection—must be applied purposefully at critical serialization, concurrency, and storage boundaries (high-risk seams), not sprayed ubiquitously as box-ticking ceremony. Where type invariants and deterministic unit tests suffice, do not add compliance overhead.

## Section 2: Target-Specific Profile

### Target Classification & Entrypoint
- Target class: `service-unattended` (as mapped in `sf-sdlc.toml`).
- Implementation workspace members: `crates/pardosa`, `crates/pardosa-derive`,
  `crates/pardosa-nats`, and `scripts`.
- Canonical verification entrypoint: `scripts/verify.sh`
- Cross-repository operational authority:
  [gh-report trunk delivery](../gh-report/docs/trunk-delivery.md) in the canonical
  sibling checkout (`Mattilsynet/gh-report`, `docs/trunk-delivery.md`).

### Resource Contracts & Bounds (FLEET-RES-01)
Changes to ingestion, buffering, concurrency, retries, recursion, or hot paths
must define and satisfy explicit resource bounds:
- **Items and bytes accounted separately**: A bounded channel alone does not bound
  memory; admit work before unbounded allocation or payload retention.
- **Permit lifetimes & RAII**: Permits and resource charges must stay alive for the
  actual resource lifetime, including error paths and cancellations. Release exactly
  once via RAII.
- **Admission control**: Work admission must be bounded. If admission waits, bound
  the number of waiters and what they retain.
- **Progress, cancellation & shutdown**:
  - Individual work units, retries, and recursion depth must be bounded.
  - Service lifetime loops require reachable, supervised shutdown and bounded work
    between shutdown checks.
  - Dropping task handles does not cancel asynchronous tasks; use explicit cancellation
    tokens and await task completion during shutdown.
- **Atomic file write protocol (CHE-0032)**:
  All durable state written to disk must use the atomic sequence:
  `write temporary file` $\rightarrow$ `fsync file` $\rightarrow$ `atomic rename` $\rightarrow$ `fsync parent directory`.

### Verification Cadences (Three-Tier Cadence)
Verification is strictly tier-scoped. A claim is backed by the tier whose scope
matches the claim: sub-missions are backed by MID; repository stable candidates
are backed by BOUNDARY. Canonical approval of a verified result repeats only per
repository stable candidate; targeted falsifiers remain allowed against any
candidate.

- **INNER** (every hopper TDD increment and targeted-reviewer falsifiers;
  changed crate ONLY; exit-code criterion: test + clippy exit 0):
  ```sh
  CARGO_TERM_PROGRESS_WHEN=never cargo test -p pardosa --locked --message-format=short
  CARGO_TERM_PROGRESS_WHEN=never cargo clippy -p pardosa --all-targets --locked --message-format=short -- -D warnings
  ```
  `--all-targets` is mandatory on clippy to catch test/bench/example lints.
  `--workspace` and `--all-features` are forbidden at this tier.

- **MID** (once at sub-mission completion before done-claim; changed crates
  plus their mechanically computed reverse-dependent closure; exit-code criterion:
  every listed package's test + clippy exit 0):
  ```sh
  cargo test -p pardosa -p pardosa-derive -p pardosa-nats --locked
  cargo clippy -p pardosa -p pardosa-derive -p pardosa-nats --all-targets --locked -- -D warnings
  cargo fmt --all -- --check
  ```
  Compute reverse dependents via `cargo metadata --format-version 1 --no-deps`.
  `--workspace` is forbidden at this tier; verify stays scoped to affected crates.

- **BOUNDARY** (once per repository stable candidate; full workspace; exit 0 across all):
  ```sh
  cargo build --workspace --all-features --locked
  timeout 900 cargo test --workspace --all-features --locked --no-fail-fast
  cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
  cargo fmt --all -- --check
  sh scripts/verify.sh
  ```
  - `timeout 900` is mandatory on the test line. Exit 124 is `Outcome::Surprise`,
    NEVER a test failure. Investigate the stall; do not fold it into a failure count.
  - `--no-fail-fast` is mandatory on the test line to ensure full blast-radius
    visibility in a single pass.

### Supply Chain Gates
`cargo deny check` and `cargo audit` are supply-chain gates; run
before publishing or bumping dependencies.

### Rustdoc Budget Gate
Run the same native check from the repository root locally and in CI:
```sh
comment-free --check-doc-budget --doc-advisory-words 80 --doc-max-words 120 --max-warning-files 0 .
```

Requires comment-free 0.2.0 at the canonical revision below:
```sh
cargo +1.98.0 install --git https://github.com/acje/comment-free --rev b4666626bbeee4e74ca41fd6ff1048b2f167dd27 --locked comment-free
```

The read-only native gate recursively scans Rust sources under `.` with the
tool's build/hidden pruning: 80 prose words is advisory; 120 is enforced.
Fenced code is excluded by the tool. Summary-only output retains full totals
while suppressing finding details; diagnostics remain visible.
Native gate exits are 0 for pass, 1 for enforced breach, and 2 for
unknown/error, including undecided payloads or empty scope. Policy and its
implementation/tests/proofs belong upstream; repository checks establish
integration only. No rewrite mode runs.
Macro-generated docs without spelled `doc` tokens remain outside detection;
this is not proof of semantic documentation coverage or process-memory bounds.

### TigerStyle Construction-Path Inventory
Invariant-bearing domain types must enforce "illegal states unrepresentable"
by design. For each changed constrained type, review all construction routes:
1. Public fields / struct literals (reject if fields allow inconsistent mutation).
2. Constructors & builders (`new()`, `builder()`).
3. `Default::default()` (must yield a valid domain state or be omitted).
4. Conversions (`From`, `TryFrom`).
5. Serde deserialization (custom validation if raw wire data could bypass invariants).
6. Mutation routes (setters, `DerefMut`).

Independent booleans remain valid booleans; genuine optionality remains `Option`.
Do not invent artificial domain restrictions where none exist.

### Closed Error Enum Policy (C4.5/C4.6)
Public error enums MUST NOT carry `#[non_exhaustive]`. Variant sets are complete
within a major semver line, making unhandled error states unrepresentable at
compile time. Enforced mechanically via `non-exhaustive-check` from the canonical
`tripwires` repository.

### Resuming the Pardosa 1.0 Spec Work
The specification roadmap is a **wayfinder map** — a bd epic whose child tickets
are the open decisions between here and a defined end state for the pardosa
library. To pick it up:

1. Load the skill: `skill({ name: "wayfinder" })`, and use its
   **"Work through the map"** mode.
2. Read the map: `bd show pardosa-jn1`. This is the low-resolution view —
   destination, standing constraints, decisions already made, fog, and what is
   out of scope. Read it before choosing anything.
3. Take the next frontier ticket: `bd ready --parent pardosa-jn1 -u`, then
   claim it with `bd assign <id> <you>` **before** doing any work.
4. Resolve one ticket per session (research tickets are the exception — those
   may be batched). Record the answer with `bd comment <id>`, close with
   `bd close <id> --reason "<gist>"`, and add a one-line entry to the map's
   Decisions-so-far by safely appending to the existing description.

**Pin `bd -C <canonical-pardosa-root>` and verify the returned prefix/path.**
The sibling gh-report store has prefix `ghr`; use its explicit root for the
cross-repository adoption contract. Ambient discovery is not store identity.

### Checking the Spec
```sh
./scripts/check.sh
```
This is the local spec-consistency gate, separate from implementation checks. It
runs the `spec-coverage` checker in `scripts/` over `docs/spec/pardosa-1.0.md`
and the RULED trace, and exits non-zero if any check fails.

### Issue Tracking & Database Discovery
This project uses **bd** (beads) for issue tracking. Run `bd prime` for full workflow context.
- Issue database: repo-local store at `.beads/embeddeddolt`.
- Pinned store discovery: Always target this repository's store directly with
  `bd -C <repo-root>` (or `BEADS_DIR`). Do not rely on ambient directory walks.
- Verify store resolution via `bd -C <repo-root> where` before performing operations.
- Strict fleet invariant: No HOME-level issue store at `~/.beads`.
- Autonomous commits follow `~/.config/opencode/AGENTS.md § Commits — agent-driven by default`.

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work atomically
bd close <id>         # Complete work
bd dolt push          # Push beads data to remote
```

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:6cd5cc61 -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

**Architecture in one line:** issues live in a local Dolt DB; sync uses `refs/dolt/data` on your git remote; `.beads/issues.jsonl` is a passive export. See https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md for details and anti-patterns.

## Agent Context Profiles

The managed Beads block is task-tracking guidance, not permission to override repository, user, or orchestrator instructions.

- **Conservative (default)**: Use `bd` for task tracking. Do not run git commits, git pushes, or Dolt remote sync unless explicitly asked. At handoff, report changed files, validation, and suggested next commands.
- **Minimal**: Keep tool instruction files as pointers to `bd prime`; use the same conservative git policy unless active instructions say otherwise.
- **Team-maintainer**: Only when the repository explicitly opts in, agents may close beads, run quality gates, commit, and push as part of session close. A current "do not commit" or "do not push" instruction still wins.

## Session Completion

This protocol applies when ending a Beads implementation workflow. It is subordinate to explicit user, repository, and orchestrator instructions.

1. **File issues for remaining work** - Create beads for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **Handle git/sync by active profile**:
   ```bash
   # Conservative/minimal/default: report status and proposed commands; wait for approval.
   git status

   # Team-maintainer opt-in only, unless current instructions forbid it:
   git pull --rebase
   git push
   git status
   ```
5. **Hand off** - Summarize changes, validation, issue status, and any blocked sync/commit/push step

**Critical rules:**
- Explicit user or orchestrator instructions override this Beads block.
- Do not commit or push without clear authority from the active profile or the current user request.
- If a required sync or push is blocked, stop and report the exact command and error.
<!-- END BEADS INTEGRATION -->
