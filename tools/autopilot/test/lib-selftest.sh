#!/usr/bin/env bash
# Selftest of lib/codex-lib.py against throw-away git repos (no cargo, no network, no Codex). Covers review autopilot
# M1 (testguard), M2 (lint test region), M3 (extsat), L3 (redcheck with no failing test), L4 (state_load keeps a corrupt file).
# Usage: bash tools/autopilot/test/lib-selftest.sh
set -u
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"; SRC="$(cd "$HERE/.." && pwd)"
LIBPY="$SRC/lib/codex-lib.py"; export PYTHONUTF8=1
T="$(mktemp -d)"; FAILS=0
ok() { echo "  ok   $*"; }
bad() { echo "  FAIL $*"; FAILS=$((FAILS+1)); }
check() { if eval "$2"; then ok "$1"; else bad "$1  [$2]"; fi; }
G() { git -c user.name=t -c user.email=t@e.com "$@"; }

echo "== M1 testguard"
LW="$T/g"; mkdir -p "$LW/crates/demo/tests"; git init -q -b master "$LW"
printf 'crates/demo/tests/t.rs\n' > "$T/owned.txt"
printf '#[test]\nfn one() {\n    assert_eq!(1, 1);\n    assert_eq!(2, 2);\n}\n' > "$LW/crates/demo/tests/t.rs"
G -C "$LW" add -A; G -C "$LW" commit -q -m base; BASE="$(git -C "$LW" rev-parse HEAD)"
guard() { # desc expected-rc content [base]
  printf '%s' "$3" > "$LW/crates/demo/tests/t.rs"
  python -I "$LIBPY" testguard "$LW" "${4:-$BASE}" "$T/owned.txt" > "$T/guard.out" 2>&1; local rc=$?; git -C "$LW" checkout -q -- .
  if [ "$rc" = "$2" ]; then ok "testguard: $1"; else bad "testguard: $1 (rc $rc, wanted $2)"; cat "$T/guard.out"; fi; }
guard "adding an assertion is allowed" 0 $'#[test]\nfn one() {\n    assert_eq!(1, 1);\n    assert_eq!(3, 3);\n    assert_eq!(2, 2);\n}\n'
guard "early return added after the opening brace is rejected" 1 $'#[test]\nfn one() {\n    return;\n    assert_eq!(1, 1);\n    assert_eq!(2, 2);\n}\n'
guard "wrapping the body in if false is rejected" 1 $'#[test]\nfn one() {\n    if false {\n    assert_eq!(1, 1);\n    assert_eq!(2, 2);\n    }\n}\n'
guard "a new #[cfg(...)] attribute is rejected" 1 $'#[cfg(any())]\n#[test]\nfn one() {\n    assert_eq!(1, 1);\n    assert_eq!(2, 2);\n}\n'
guard "unreadable base revision fails instead of passing vacuously" 1 $'#[test]\nfn one() {\n    assert_eq!(1, 1);\n}\n' deadbeefdeadbeefdeadbeefdeadbeefdeadbeef
check "unreadable base is named in the message" 'grep -q "cannot read the test base" "$T/guard.out"'
# braces inside a char literal / block comment / raw string must not cut a body short and hide a weakened tail
printf '#[test]\nfn one() {\n    let c = '"'"'}'"'"';\n    /* } */\n    let s = r#"}"#;\n    assert_eq!(1, 1);\n    assert_eq!(2, 2);\n}\n' > "$LW/crates/demo/tests/t.rs"
G -C "$LW" add -A; G -C "$LW" commit -q -m base2; BASE2="$(git -C "$LW" rev-parse HEAD)"
guard "tricky braces: unchanged file passes" 0 $'#[test]\nfn one() {\n    let c = \'}\';\n    /* } */\n    let s = r#"}"#;\n    assert_eq!(1, 1);\n    assert_eq!(2, 2);\n}\n' "$BASE2"
guard "tricky braces: dropping the last assertion is still caught" 1 $'#[test]\nfn one() {\n    let c = \'}\';\n    /* } */\n    let s = r#"}"#;\n    assert_eq!(1, 1);\n}\n' "$BASE2"

echo "== M2 lint test region"
LL="$T/l"; mkdir -p "$LL/crates/demo/src"; git init -q -b master "$LL"
printf 'crates/demo/src/a.rs\n' > "$T/owned2.txt"
printf 'pub fn a() {}\n' > "$LL/crates/demo/src/a.rs"; G -C "$LL" add -A; G -C "$LL" commit -q -m base
lint() { # desc expected-rc content
  printf '%s' "$3" > "$LL/crates/demo/src/a.rs"; python -I "$LIBPY" lint "$LL" "$T/owned2.txt" > "$T/lint.out" 2>&1; local rc=$?
  git -C "$LL" checkout -q -- .
  if [ "$rc" = "$2" ]; then ok "lint: $1"; else bad "lint: $1 (rc $rc, wanted $2)"; cat "$T/lint.out"; fi; }
lint "unwrap inside a real #[cfg(test)] mod is allowed" 0 $'pub fn a() {}\n#[cfg(test)]\nmod t {\n    fn f() { std::fs::read_to_string("x").unwrap(); }\n}\n'
lint "early #[cfg(test)] on a use does not exempt a later todo!()" 1 $'#[cfg(test)]\nuse std::fmt;\npub fn a() {\n    todo!()\n}\n'
lint "early #[cfg(test)] on a fn does not exempt a later unwrap" 1 $'#[cfg(test)]\nfn helper() {}\npub fn a() {\n    let s = std::fs::read_to_string("x").unwrap();\n}\n'
lint "#[cfg(test)] in a comment does not exempt a later todo!()" 1 $'// #[cfg(test)]\npub fn a() {\n    todo!()\n}\n'
lint "todo!() inside the real test mod is allowed" 0 $'pub fn a() {}\n#[cfg(test)]\n#[allow(dead_code)] // reason: fixture\nmod t {\n    fn f() { todo!() }\n}\n'

echo "== M3 extsat"
RP="$T/r"; git init -q -b master "$RP"; G -C "$RP" commit -q --allow-empty -m init
git clone -q "$RP" "$T/c" 2>/dev/null; C="$T/c"
cm() { G -C "$T/c" commit -q --allow-empty -m "$1" -m "${2:-}"; }
cm "feat(W8-17c): half done (partial)" "test(W8-17a): red tests"
cm "test(W8-17b): red tests for W8-17b"
cm "feat(W9-E4): port widget" "needs W8-09b"
cm "feat(W8-20a): done properly" "fixes W8-20b along the way"
cm "fix(W8-21a): bug" 
cm "Complete W8-22a layout unit"
git -C "$C" update-ref refs/remotes/origin/master HEAD
mkdir -p "$T/st"
ex() { python -I "$LIBPY" extsat "$C" "$T/st" "$1"; }
check "partial commit does not satisfy its own unit" '[ "$(ex W8-17c)" = W8-17c ]'
check "id only in the body of another commit does not satisfy" '[ "$(ex W8-17a)" = W8-17a ] && [ "$(ex W8-20b)" = W8-20b ]'
check "test(...) red-tests commit does not satisfy" '[ "$(ex W8-17b)" = W8-17b ]'
check "id mentioned later in the subject/body of a feat commit does not satisfy" '[ "$(ex W8-09b)" = W8-09b ]'
check "feat(<id>) commit satisfies" '[ -z "$(ex W8-20a)" ] && [ -z "$(ex W8-21a)" ]'
check "Complete <id> subject satisfies" '[ -z "$(ex W8-22a)" ]'

echo "== L3 redcheck, L4 state_load"
printf 'running 2 tests\ntest a ... ok\ntest b ... ok\n\ntest result: ok. 2 passed; 0 failed\n' > "$T/rc.txt"
python -I "$LIBPY" redcheck "$T/rc.txt" > "$T/rc.out" 2>&1; rc=$?
check "redcheck rejects a run in which no test failed" '[ $rc = 1 ] && grep -q "no test failed at all" "$T/rc.out"'
printf '{ not json' > "$T/state.json"
python -I "$LIBPY" sget "$T/state.json" X > /dev/null
check "state_load keeps a copy of a corrupt state file" '[ "$(cat "$T/state.json.corrupt")" = "{ not json" ]'
printf '{"foo": 1}' > "$T/state2.json"
check "state_load tolerates a state without units/meta" '[ -z "$(python -I "$LIBPY" sget "$T/state2.json" X)" ]'
rm -rf "$T"
[ "$FAILS" -eq 0 ] && echo "lib-selftest: all passed" || { echo "lib-selftest: $FAILS failed"; exit 1; }
