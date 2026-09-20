# Agent Instructions

## Current producer and delivery authority

The implementation is in this repository: `crates/pardosa`,
`crates/pardosa-derive`, and `crates/pardosa-nats`; `scripts` is the fourth
workspace member. The compiler pin is `1.98.0`, while the three library
manifests currently declare edition 2021 and MSRV 1.89.0 (resolver 2).

Cross-repository operational authority is
[gh-report trunk delivery](../gh-report/docs/trunk-delivery.md) in the canonical
sibling checkout (`Mattilsynet/gh-report`, `docs/trunk-delivery.md`). Local
specification and source authority remain here. Active adoption work is tracked
in gh-report's `ghr-hxyqs.66`, alongside Pardosa's existing review records.

After dependency intake, run implementation checks from this repository:

```sh
cargo atest -p pardosa --locked
cargo aclippy -p pardosa --all-targets --locked -- -D warnings
cargo atest -p pardosa -p pardosa-derive -p pardosa-nats --locked
cargo aclippy -p pardosa -p pardosa-derive -p pardosa-nats --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

The first pair is INNER; the three-package pair is the current adoption MID
baseline. Record failures and live-NATS skips explicitly. Retain the root
`Cargo.lock` as a reproducible producer verification/release input. The fresh
admission baseline is recorded in gh-report bead `ghr-krvir`; historical
producer revisions did not track a lock, so this is not historical dependency
parity. Reassess intake when the lock, selected features, or execution context
changes.

## Resuming the pardosa 1.0 spec work

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
   Decisions-so-far by safely appending to the existing description; `bd update
   --stdin` replaces the body, so read and preserve it before updating.

**Pin `bd -C <canonical-pardosa-root>` and verify the returned prefix/path.**
The sibling gh-report store has prefix `ghr`; use its explicit root for the
cross-repository adoption contract. Ambient discovery is not store identity.

**On a fresh clone**, the beads data comes from the git remote, not the
worktree: `git clone` then `bd dolt pull`. `.beads/issues.jsonl` is a passive
export and is not the source of truth.

Ticket types carry a `wayfinder:<type>` label. `research` is AFK — dispatch
`copernicus` via Task. `grilling` is HITL — it needs the human, and an agent
answering its own grilling questions has broken the method.

Reference material for the spec lives in `docs/origin/`; the current producer
implementation lives in this repository's `crates/pardosa*`. The sibling
gh-report consumes revision-pinned git dependencies, not local Pardosa members.

## Checking the spec

```sh
./scripts/check.sh
```

This is the local spec-consistency gate, separate from implementation checks. It
runs the `spec-coverage` checker in `scripts/` over `docs/spec/pardosa-1.0.md`
and the RULED trace, and exits non-zero if any check fails. The nine checks
assert that the trace and the spec stay mutually consistent: every RULED row in
range is present exactly once, every SPEC-BEARING row carries a clause id, every
cited clause id resolves to a real clause heading, every clause heading is cited
by at least one row, no clause body restates a regime marker or a PGN token, the
STATUS block is well formed, and clause ids run in non-decreasing layer order
with dense per-layer numbering. Run it after any edit to the spec or the trace.

The script passes `--allow-regime-prose C5.1` and the flag is load-bearing:
C5.1 is the clause that *defines* the regime-marker scheme, so it must write
both marker tokens, and without the exemption `regime_marker_unique` fires on it.
The exemption is spelled out at the call site rather than defaulted inside the
binary, so that the one clause holding it stays visible.

At the inspected `3d94d73` baseline there is no tracked `.github` workflow.
This observation is not a prohibition on future reviewed CI work or evidence
of GitHub branch-protection state. `scripts/check.sh` remains the named local
spec entry point; it does not replace Cargo implementation checks.

### Two notes on the bd-generated sections below

**Git policy.** The managed Beads block prescribes a "Conservative (default)"
profile — no commits or pushes unless explicitly asked. The fleet doctrine in
`~/.config/opencode/AGENTS.md` (§ Commits — agent-driven by default) overrides
it: commit autonomously when the work was in scope, verification passed, and
the tree contains only intended changes. The Beads block itself grants this
precedence ("Explicit user or orchestrator instructions override this Beads
block"); this note exists so the contradiction does not have to be
re-adjudicated each session.

**Tooling.** This project uses opencode only. Claude Code and Codex config
(`CLAUDE.md`, `.claude/`, `.codex/`) and the project-local copy of the beads
skill (`.agents/`) were removed as unused — the beads skill is installed
globally at `~/.agents/skills/beads/`. Running `bd init` or `bd setup codex`
will regenerate them; delete them again rather than adopting them.

## Issue tracking

This project uses **bd** (beads) for issue tracking. Run `bd prime` for full workflow context.

> **Architecture in one line:** Issues live in a local Dolt database
> (`.beads/dolt/`); cross-machine sync uses `bd dolt push/pull` (a
> git-compatible protocol), stored under `refs/dolt/data` on your git
> remote — separate from `refs/heads/*` where your code lives.
> `.beads/issues.jsonl` is a passive export, not the wire protocol.
>
> See [SYNC_CONCEPTS.md](https://github.com/gastownhall/beads/blob/main/docs/SYNC_CONCEPTS.md)
> for the one-screen overview and anti-patterns (don't treat JSONL as the
> source of truth; don't `bd import` during normal operation; don't
> reach for third-party Dolt hosting before trying the default).

## Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work atomically
bd close <id>         # Complete work
bd dolt push          # Push beads data to remote
```

## Non-Interactive Shell Commands

**ALWAYS use non-interactive flags** with file operations to avoid hanging on confirmation prompts.

Shell commands like `cp`, `mv`, and `rm` may be aliased to include `-i` (interactive) mode on some systems, causing the agent to hang indefinitely waiting for y/n input.

**Use these forms instead:**
```bash
# Force overwrite without prompting
cp -f source dest           # NOT: cp source dest
mv -f source dest           # NOT: mv source dest
rm -f file                  # NOT: rm file

# For recursive operations
rm -rf directory            # NOT: rm -r directory
cp -rf source dest          # NOT: cp -r source dest
```

**Other commands that may prompt:**
- `scp` - use `-o BatchMode=yes` for non-interactive
- `ssh` - use `-o BatchMode=yes` to fail instead of prompting
- `apt-get` - use `-y` flag
- `brew` - use `HOMEBREW_NO_AUTO_UPDATE=1` env var

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
