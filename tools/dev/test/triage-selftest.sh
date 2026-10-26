#!/usr/bin/env bash
set -u
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"; tmp=$(mktemp -d); fails=0
ok(){ echo "ok $1"; }; bad(){ echo "FAIL $1"; fails=$((fails+1)); }
mkdir -p "$tmp/state/needs-claude" "$tmp/state/logs" "$tmp/contracts"
note(){ # unit reason tail
  printf '# %s needs Claude\n\nReason: %s\n\n## Error tail\n```\n%s\n```\n## git diff --stat\n```\n```\n## Failing tests\n(none parsed)\n' "$1" "$2" "$3" > "$tmp/state/needs-claude/$1.md"; }
note U-fmt 'gate failed' $'Diff in C:\\x\\a.rs:5:\n-use a;\n+use b;'
note U-red 'gate failed' 'red-test-guard: test file modified/weakened: crates/x/tests/t.rs'
note U-stub 'gate failed' $'thread main panicked at src/a.rs:3:\nnot yet implemented'
note U-comp 'gate failed' $'error[E0425]: cannot find value `x`\n --> src/a.rs:1'
note U-test 'gate failed' $'thread t panicked at tests/t.rs:7:\nassertion `left == right` failed\n  left: 1\n right: 2'
note U-env 'gate failed' "error: process did not exit successfully (exit code: 0xc0000142, STATUS_DLL_INIT_FAILED)"
note U-mut 'mutants' 'MISSED   crates/x/src/a.rs:1:1: replace a with b'
note U-unk 'cannot derive crate' ''
note U-flaky 'gate failed' $'thread t panicked at tests/misc_app_log.rs:7:\nassertion failed: x'
note U-gate 'cannot derive crate' ''
printf 'GATE FAIL: cargo test\nerror[E0308]: mismatched types\n' > "$tmp/state/logs/U-gate.gate-error.txt"
for i in $(seq 1 70); do echo "tail line $i"; done > "$tmp/t70"
note U-long 'gate failed' "$(cat "$tmp/t70")"$'\nerror[E0001]: x'
cat > "$tmp/contracts/U-test.md" <<'C'
## Owned files (the ONLY files the porter may create or modify)
```
crates/x/src/a.rs            (stub)
crates/x/tests/t.rs          (new)
tools/oracle-cs/Commands/U.cs, tests/golden-inputs/g.json    (lead)
```
Lead-owned: foo
C
export STATE_DIR="$tmp/state" CONTRACTS_DIR="$tmp/contracts"
T="bash $ROOT/tools/dev/triage.sh"
chk(){ out=$($T "$1" 2>&1); echo "$out" | grep -q "^class: $2\$" && ok "$1 -> $2" || { bad "$1 -> $2"; echo "$out"; }; }
chk U-fmt rustfmt-only; chk U-red red-test-guard; chk U-stub stubs-left; chk U-comp compile-error
chk U-test test-failure; chk U-env environment; chk U-mut mutants-budget; chk U-unk unknown
out=$($T U-flaky); echo "$out" | grep -q 'known flaky misc_app_log' && ok flaky-hint || bad flaky-hint
chk U-gate compile-error
out=$($T NOPE); echo "$out" | grep -q 'nothing to triage' && ok missing-unit || bad missing-unit
$T U-test --prompt "$tmp/p.md" >/dev/null
if grep -q 'crates/x/src/a.rs' "$tmp/p.md" && grep -q 'tools/oracle-cs/Commands/U.cs' "$tmp/p.md" && grep -q 'tests/golden-inputs/g.json' "$tmp/p.md" && ! grep -q 'foo' "$tmp/p.md"; then ok prompt-owned; else bad prompt-owned; cat "$tmp/p.md"; fi
if grep -q 'Never weaken' "$tmp/p.md" && grep -q 'UTF-8, LF' "$tmp/p.md" && grep -q 'cite the C# file' "$tmp/p.md"; then ok prompt-rules; else bad prompt-rules; fi
$T U-long --prompt "$tmp/p2.md" >/dev/null
n=$(awk '/^```$/{c++; next} c==1' "$tmp/p2.md" | wc -l); [ "$n" -le 60 ] && ok prompt-tail-60 || bad "prompt-tail-60 ($n)"
if [ "$(head -c1 "$tmp/p.md")" = '#' ] && ! grep -q $'\r' "$tmp/p.md"; then ok prompt-lf-nobom; else bad prompt-lf-nobom; fi
$T --all | grep -q 'U-fmt *rustfmt-only' && ok all-list || bad all-list
out=$(STATE_DIR="$tmp/empty" $T U-x); echo "$out" | grep -q 'nothing to triage' && ok clean-checkout || bad clean-checkout
$T 2>/dev/null; [ $? -eq 2 ] && ok usage-exit2 || bad usage-exit2
rm -rf "$tmp"
if [ $fails -eq 0 ]; then echo "selftest PASSED"; else echo "selftest $fails FAILED"; exit 1; fi
