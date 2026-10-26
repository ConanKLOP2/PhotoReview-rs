#!/usr/bin/env bash
# Selftest of tools/dev/doc-links.py on a fixture tree (no network, no git).
set -u
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"; tmp=$(mktemp -d); fails=0
ok(){ echo "ok $1"; }; bad(){ echo "FAIL $1"; fails=$((fails+1)); }
PY="${PY_BIN:-python}"
mkdir -p "$tmp/good/docs" "$tmp/good/crates/pr-x/src" "$tmp/good/contracts" "$tmp/bad/docs" "$tmp/bad/crates/pr-x/src" "$tmp/bad/contracts"

# --- clean tree: every kind of reference resolves ---
printf '%s\n' 'fn main() {}' > "$tmp/good/crates/pr-x/src/lib.rs"
cat > "$tmp/good/docs/A.md" <<'EOF'
# Alpha Title

See [B](B.md), [B section](B.md#second-section-1), [self](#alpha-title), [web](https://example.com/x), [dir](../crates/pr-x/src/).
Code path `crates/pr-x/src/lib.rs` and with line `crates/pr-x/src/lib.rs:12` and glob `crates/*/src/*.rs`.
A future file `crates/pr-x/src/later.rs` is proposed (new). C# path `tests/PhotoReview.Core.Tests/FooTests.cs` is not checked.
Runtime `tools/autopilot/state/x.json` and placeholder `contracts/WU-xxx.md` are skipped.

```
[ignored](nope.md) and `crates/pr-x/nope.rs` inside a fence
```
EOF
cat > "$tmp/good/docs/B.md" <<'EOF'
# Beta

## Second section

## Second section
EOF
cat > "$tmp/good/contracts/W1-01.md" <<'EOF'
# W1-01

Owned files
- `crates/pr-x/src/not_yet.rs`
EOF
out=$("$PY" -I "$ROOT/tools/dev/doc-links.py" --root "$tmp/good"); rc=$?
if [ "$rc" -eq 0 ] && printf '%s' "$out" | grep -q 'errors=0'; then ok 'clean tree passes'; else bad "clean tree passes: $out"; fi
if printf '%s' "$out" | grep -q 'PROPOSED docs/A.md:5' && printf '%s' "$out" | grep -q 'PROPOSED contracts/W1-01.md'; then ok 'proposed paths are reported separately'; else bad "proposed paths are reported separately: $out"; fi
if printf '%s' "$out" | grep -q 'csharp_refs=1'; then ok 'C# repo paths counted, not errors'; else bad "C# repo paths counted: $out"; fi

# --- broken tree: each defect is found exactly ---
cat > "$tmp/bad/docs/A.md" <<'EOF'
# Alpha

[missing file](missing.md)
[dead anchor](B.md#no-such-heading)
[dead self anchor](#nope)
Wrong path `crates/pr-x/src/renamed.rs` and `docs/Gone.md` and `crates/*/nothing/*.zzz`.
EOF
printf '# Beta\n' > "$tmp/bad/docs/B.md"
printf 'x\n' > "$tmp/bad/crates/pr-x/src/lib.rs"
out=$("$PY" -I "$ROOT/tools/dev/doc-links.py" --root "$tmp/bad"); rc=$?
[ "$rc" -eq 1 ] && ok 'exit code 1 on errors' || bad "exit code 1 on errors (rc=$rc)"
for needle in 'docs/A.md:3: link `missing.md`' 'docs/A.md:4: anchor `B.md#no-such-heading`' 'docs/A.md:5: anchor `#nope`' \
  'docs/A.md:6: path `crates/pr-x/src/renamed.rs`' 'docs/A.md:6: path `docs/Gone.md`' 'docs/A.md:6: path `crates/*/nothing/*.zzz`'; do
  if printf '%s' "$out" | grep -qF "$needle"; then ok "found $needle"; else bad "missing $needle"; fi
done
printf '%s' "$out" | grep -q 'errors=6 ' && ok 'exactly 6 errors' || bad "exactly 6 errors: $out"

rm -rf "$tmp"
if [ "$fails" -eq 0 ]; then echo 'selftest PASSED'; else echo "selftest $fails FAILED"; fi
exit "$fails"
