#!/usr/bin/env bash
# Reproduce the high-value evidence in evidence/.
# Requires: cargo, cue (official), python3, sqlite3 (optional, tamper step uses python).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
export CUE_BIN="${CUE_BIN:-cue}"
export TZ=UTC

if ! "$CUE_BIN" version >/dev/null 2>&1; then
  echo "official CUE not found. Run scripts/install-cue.sh v0.17.1" >&2
  exit 1
fi

BIN="$ROOT/target/debug/ostinato"
cargo build -q -p ostinato

rm -rf evidence
mkdir -p evidence
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

section() {
  # Append a titled command transcript to evidence/<name>.txt
  local name="$1"; shift
  {
    set +e
    echo "### $*"
    echo
    "$@"
    local code=$?
    echo
    echo "exit=$code"
  } >"evidence/$name.txt" 2>&1 || true
}

{
  echo "$( "$BIN" --version )"
  echo "cue: $( "$CUE_BIN" version | head -1 )"
  echo "rust: $(rustc --version)"
  echo "cargo: $(cargo --version)"
  echo "platform: $(uname -srm)"
} >evidence/00-environment.txt

{
  echo "### cargo fmt --all -- --check"
  cargo fmt --all -- --check
  echo "exit=$?"
  echo
  echo "### cargo clippy --all-targets --all-features -- -D warnings"
  cargo clippy --all-targets --all-features -- -D warnings
  echo "exit=$?"
  echo
  echo "### cargo test --all"
  cargo test --all
  echo "exit=$?"
} >evidence/01-quality.txt 2>&1 || true

section 02-validate-rust "$BIN" --db "$WORK/db.sqlite" validate examples/module/complex.cue -p prefix=prod
section 02b-validate-oracle "$BIN" --db "$WORK/db.sqlite" --evaluator cue --cue "$CUE_BIN" validate examples/module/complex.cue -p prefix=prod
section 02c-conformance "$ROOT/scripts/conformance.sh"
section 03-run-hello "$BIN" --db "$WORK/db.sqlite" --cue "$CUE_BIN" run examples/hello.cue -p name=Ostinato
section 04-run-retry "$BIN" --db "$WORK/db.sqlite" run examples/retry.cue
section 05-run-cycle "$BIN" --db "$WORK/db.sqlite" run examples/cycle.cue

RUN=$("$BIN" --db "$WORK/db.sqlite" --json runs | python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["id"])')
section 06-status "$BIN" --db "$WORK/db.sqlite" status "$RUN"
section 07-logs "$BIN" --db "$WORK/db.sqlite" logs "$RUN"
section 08-trials "$BIN" --db "$WORK/db.sqlite" trials "$RUN"
section 09-audit "$BIN" --db "$WORK/db.sqlite" audit verify "$RUN"
section 10-metrics "$BIN" --db "$WORK/db.sqlite" metrics --run "$RUN" --format prometheus

# Resume evidence: a failing workflow, then a flag file, then resume.
cat >"$WORK/resume.cue" <<'CUE'
package demo
workflow: {
	name: "resume-evidence"
	steps: {
		a: { kind: "exec", exec: { cmd: ["sh", "-c", "echo a >> trace.log"] } }
		b: {
			kind: "exec"
			needs: ["a"]
			exec: { cmd: ["sh", "-c", "test -f go.flag || exit 7; echo b >> trace.log"] }
		}
	}
}
CUE
section 11-resume-fail "$BIN" --db "$WORK/resume.sqlite" run "$WORK/resume.cue"
RESUME_RUN=$("$BIN" --db "$WORK/resume.sqlite" --json runs | python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["id"])')
touch "$WORK/go.flag"
section 12-resume-ok "$BIN" --db "$WORK/resume.sqlite" resume "$RESUME_RUN"
section 13-resume-trials "$BIN" --db "$WORK/resume.sqlite" trials "$RESUME_RUN"

# Agent + recursive improvement evidence.
chmod +x examples/agent.sh
section 14-agentic "$BIN" --db "$WORK/agent.sqlite" --cue "$CUE_BIN" run examples/agentic.cue --agent-cmd "$ROOT/examples/agent.sh"
AGENT_RUN=$("$BIN" --db "$WORK/agent.sqlite" --json runs | python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["id"])')
section 15-improve "$BIN" --db "$WORK/agent.sqlite" improve "$AGENT_RUN" --rounds 2 --agent-cmd "$ROOT/examples/agent.sh"
section 16-children "$BIN" --db "$WORK/agent.sqlite" runs

# Gateways: XOR routing, dead-path skips, OR join, metrics and audit.
section 18-gateways-run "$BIN" --db "$WORK/gateway.sqlite" --cue "$CUE_BIN" run examples/gateway.cue
GATEWAY_RUN=$("$BIN" --db "$WORK/gateway.sqlite" --json runs | python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["id"])')
section 18-gateways-status "$BIN" --db "$WORK/gateway.sqlite" status "$GATEWAY_RUN"
section 18-gateways-logs "$BIN" --db "$WORK/gateway.sqlite" logs "$GATEWAY_RUN"
section 18-gateways-trials "$BIN" --db "$WORK/gateway.sqlite" trials "$GATEWAY_RUN"
section 18-gateways-metrics "$BIN" --db "$WORK/gateway.sqlite" metrics --run "$GATEWAY_RUN" --format prometheus
section 18-gateways-audit "$BIN" --db "$WORK/gateway.sqlite" audit verify "$GATEWAY_RUN"

# Tamper detection: mutate an audit payload and prove verification fails.
python3 - "$BIN" "$WORK/agent.sqlite" "$AGENT_RUN" <<'PY' >evidence/17-tamper.txt 2>&1 || true
import sqlite3, subprocess, sys
bin_path, db, run = sys.argv[1:4]
conn = sqlite3.connect(db)
conn.execute("UPDATE events SET data_json='{\"tampered\":true}' WHERE run_id=? AND seq=1", (run,))
conn.commit()
proc = subprocess.run([bin_path, "--db", db, "audit", "verify", run], capture_output=True, text=True)
print(proc.stdout, end="")
print(proc.stderr, end="")
print("exit=", proc.returncode)
PY

echo "evidence written to $ROOT/evidence"
