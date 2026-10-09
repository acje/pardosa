#!/bin/sh
set -eu

# ci-caller-parity: targeted static tests proving the producer-native CI caller
# invokes the same native verification entrypoint (scripts/verify.sh) with a
# non-skipped live-NATS prerequisite, without reimplementing or removing any
# native gate. Structural checks parse the workflow as YAML with python3/yaml;
# the pin file (tools/.nats-server-version) owns the version selection. Any
# breach exits non-zero with a named failure.

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

# The unchanged native entrypoint must remain syntactically valid shell.
sh -n scripts/verify.sh

exec python3 -B - .github/workflows/ci.yml tools/.nats-server-version <<'PY'
import re
import shlex
import sys
import yaml

workflow_path, pin_path = sys.argv[1:3]
workflow = ".github/workflows/ci.yml"
pin_file = "tools/.nats-server-version"
cf_rev = "b4666626bbeee4e74ca41fd6ff1048b2f167dd27"


def fail(msg):
    print(f"::error::ci-caller-parity: {msg}", file=sys.stderr)
    sys.exit(1)


def must_not_continue_on_error(props, what):
    coe = props.get("continue-on-error")
    if coe is not None and coe is not False:
        fail(f"{what} sets continue-on-error (including expression form); its failure would not fail the job")


try:
    with open(workflow_path) as fh:
        wf = yaml.safe_load(fh)
except Exception as exc:
    fail(f"{workflow_path} is not valid YAML: {exc}")
if not isinstance(wf, dict):
    fail(f"{workflow_path} is not a YAML mapping")

verify_job = wf.get("jobs", {}).get("verify")
if not isinstance(verify_job, dict):
    fail("jobs.verify is not a mapping")
must_not_continue_on_error(verify_job, "verification job")
if verify_job.get("if") not in (None, True):
    fail("verification job is conditional (if:) and could be disabled; must run unconditionally")

steps = wf["jobs"]["verify"]["steps"]
if not isinstance(steps, list):
    fail("jobs.verify.steps is not a list")

# Every third-party action is pinned to a full 40-hex immutable commit SHA.
for step in steps:
    ref = step.get("uses")
    if not ref:
        continue
    if not re.fullmatch(r"[\w.-]+/[\w.-]+@[0-9a-f]{40}", ref):
        fail(f"action ref {ref!r} is not SHA-pinned to a 40-hex immutable commit")

# Native entrypoint: an active, unconditional run step whose run text contains
# the standalone invocation `sh scripts/verify.sh` (a commented or `true # ...`
# spelling is not an active invocation).
native = []
for i, step in enumerate(steps):
    run = step.get("run")
    if not isinstance(run, str):
        continue
    if any(line.strip() == "sh scripts/verify.sh" for line in run.splitlines()):
        native.append((i, step))
if len(native) != 1:
    fail(f"expected exactly one active step invoking `sh scripts/verify.sh`, found {len(native)}")
i_native, native_step = native[0]
if "if" in native_step:
    fail("native verification step is conditional (if:) and could be disabled; must run unconditionally")
must_not_continue_on_error(native_step, "native verification step")
if native_step["run"].strip() != "sh scripts/verify.sh":
    fail("native verification step run must be exactly the single command `sh scripts/verify.sh`; conditional or complex shell is refused")

# nats-server precondition: a step that hard-verifies the exact complete pin
# version on PATH, unconditional, and ordered BEFORE the native entrypoint.
pre = []
for i, step in enumerate(steps):
    run = step.get("run")
    if not isinstance(run, str) or "nats-server --version" not in run:
        continue
    pre.append((i, step))
if len(pre) != 1:
    fail(f"expected exactly one nats-server version-verify step, found {len(pre)}")
i_pre, pre_step = pre[0]
if "if" in pre_step:
    fail("nats-server version-verify step is conditional (if:) and could be skipped")
must_not_continue_on_error(pre_step, "nats-server version-verify prerequisite step")
if i_pre >= i_native:
    fail("nats-server version-verify must run before the native verification entrypoint")
pre_run = pre_step["run"]
if 'grep -F "v${version}"' in pre_run:
    fail("nats-server version check shows the substring-match form; must compare the complete version")
if '"$reported" != "$version"' not in pre_run:
    fail("nats-server version check does not compare the complete reported version to the pin exactly")
if pin_file not in pre_run:
    fail(f"nats-server version check does not read the authoritative pin from {pin_file}")

# nats-server install is checksum-verified against upstream SHA256SUMS and
# reads the authoritative pin file.
install = []
for step in steps:
    run = step.get("run")
    if not isinstance(run, str):
        continue
    if "sha256sum -c -" in run and "SHA256SUMS" in run:
        install.append(step)
if len(install) != 1:
    fail(f"expected exactly one checksum-verified nats-server install step, found {len(install)}")
if pin_file not in install[0]["run"]:
    fail(f"nats-server install does not read the pin from {pin_file}")

# The nats-server binary cache is keyed by the authoritative pin file.
cache = [s for s in steps if isinstance(s.get("uses"), str) and s["uses"].split("@")[0] == "actions/cache"]
if len(cache) != 1:
    fail("expected exactly one actions/cache step")
key = (cache[0].get("with") or {}).get("key") or ""
if "hashFiles('tools/.nats-server-version')" not in key:
    fail("nats-server cache key does not hash my pin file (hashFiles('tools/.nats-server-version'))")

# comment-free installation is a bounded single-command contract: exactly one
# step whose run is ONE physical command line (no internal newline or
# comment-newline command boundary) whose tokenized form equals the canonical
# installer as an exact full argv, run unconditionally without
# continue-on-error. Any other spelling (echo/true prefix, a multiline block,
# an alternate toolchain, a branch, a missing flag, a suffix) is refused,
# never parsed as an acceptable install.
canonical_argv = shlex.split(
    "cargo +1.98.0 install --git https://github.com/acje/comment-free --rev "
    + cf_rev
    + " --locked comment-free",
    comments=True,
    posix=True,
)
cf_install = [
    s
    for s in steps
    if isinstance(s.get("run"), str)
    and "comment-free" in s["run"]
    and "--git https://github.com/acje/comment-free" in s["run"]
]
if len(cf_install) != 1:
    fail("expected exactly one comment-free install step")
install_step = cf_install[0]
if "if" in install_step:
    fail("comment-free install step is conditional (if:) and could be skipped")
must_not_continue_on_error(install_step, "comment-free install step")
if "\n" in install_step["run"]:
    fail("comment-free install run must be one physical command line; an internal newline or comment-newline command boundary is refused")
try:
    argv = shlex.split(install_step["run"], comments=True, posix=True)
except ValueError:
    argv = None
if argv != canonical_argv:
    fail("comment-free install run must be exactly the canonical single-command argv; any prefix/suffix/control/branch/flag mutation is refused")

# tools/.nats-server-version owns the version selection: it must hold a plain
# X.Y.Z version (syntax only; the value itself is owned by the pin file, and a
# coherent update must not be rejected here).
pin = open(pin_file).read().strip()
if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", pin):
    fail(f"{pin_file} does not hold a plain X.Y.Z version: {pin!r}")

print(f"OK: ci-caller-parity all targeted caller checks passed (pin {pin}).")
PY
