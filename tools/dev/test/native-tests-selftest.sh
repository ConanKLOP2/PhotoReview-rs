#!/usr/bin/env bash
# Selftest for tools/dev/native-tests.sh with fake cargo and fake powershell (no real cargo, no real desktop probe).
set -u
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp=$(mktemp -d); fails=0
ok(){ echo "ok $1"; }; bad(){ echo "FAIL $1"; fails=$((fails+1)); }
R="$tmp/repo"; T="$R/crates/pr-platform-win/tests"; mkdir -p "$T" "$R/crates/pr-other/tests" "$tmp/bin"
printf '#[test]\n#[ignore = "native: needs desktop"]\nfn a() {}\n#[ignore = "native: x"]\nfn b() {}\n#[ignore = "slow: not native"]\nfn c() {}\n' > "$T/alpha_native.rs"
printf '#[ignore = "native: needs desktop"]\nfn a() {}\n' > "$T/beta_native.rs"
printf '#[test]\nfn plain() {}\n' > "$T/gamma_plain.rs"
printf '#[ignore = "native: o"]\nfn a() {}\n' > "$R/crates/pr-other/tests/other_native.rs"
cat > "$tmp/bin/cargo.sh" <<'EOF'
#!/bin/sh
echo "$@" >> "$FAKE_LOG"
case "$*" in
  *--test\ beta_native*) echo "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out"; exit 101 ;;
  *--test\ broken*) echo "error: could not compile"; exit 101 ;;
  *) echo "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out"; exit 0 ;;
esac
EOF
cat > "$tmp/bin/ps_yes.sh" <<'EOF'
#!/bin/sh
echo true
EOF
cat > "$tmp/bin/ps_no.sh" <<'EOF'
#!/bin/sh
echo false
EOF
chmod +x "$tmp/bin"/*
export NATIVE_TESTS_ROOT="$R" CARGO_BIN="$tmp/bin/cargo.sh" FAKE_LOG="$tmp/cargo.log" NATIVE_TIMEOUT=30
run(){ out=$(bash "$HERE/native-tests.sh" "$@" 2>&1); rc=$?; }
has(){ printf '%s' "$out" | grep -q -- "$1"; }

: > "$FAKE_LOG"; PS_BIN="$tmp/bin/ps_yes.sh" run --list
if [ $rc -eq 0 ] && has "2 integration test files with native tests (3 native tests)" && has "alpha_native *2" && ! has gamma_plain; then ok "list counts native tests per file"; else bad "list counts native tests per file"; fi
if has "interactive input desktop: yes" && ! has WARNING; then ok "desktop probe true: no warning"; else bad "desktop probe true: no warning"; fi
if [ ! -s "$FAKE_LOG" ]; then ok "--list never calls cargo"; else bad "--list never calls cargo"; fi

PS_BIN="$tmp/bin/ps_no.sh" run --list
if has "interactive input desktop: NO" && has "WARNING: tests that move the real cursor"; then ok "desktop probe false: warning printed"; else bad "desktop probe false: warning printed"; fi

PS_BIN="$tmp/nope" run --list
if [ $rc -eq 0 ] && has "interactive input desktop: NO"; then ok "missing powershell is treated as no desktop"; else bad "missing powershell is treated as no desktop"; fi

PS_BIN="$tmp/bin/ps_yes.sh" run pr-other --list
if has "crate: pr-other: 1 integration" && has other_native; then ok "crate argument selects another crate"; else bad "crate argument selects another crate"; fi

PS_BIN="$tmp/bin/ps_yes.sh" run no-such-crate --list
if [ $rc -eq 2 ]; then ok "unknown crate is an error"; else bad "unknown crate is an error"; fi
PS_BIN="$tmp/bin/ps_yes.sh" run --frobnicate
if [ $rc -eq 2 ]; then ok "unknown option is a usage error"; else bad "unknown option is a usage error"; fi

: > "$FAKE_LOG"; PS_BIN="$tmp/bin/ps_yes.sh" run
if [ $rc -eq 1 ]; then ok "a failing binary makes the run exit 1"; else bad "a failing binary makes the run exit 1 (rc=$rc)"; fi
if grep -q -- '^test -p pr-platform-win --locked --test alpha_native -- --include-ignored --test-threads=1$' "$FAKE_LOG" \
   && grep -q -- '--test beta_native -- --include-ignored --test-threads=1$' "$FAKE_LOG" && [ "$(wc -l < "$FAKE_LOG")" -eq 2 ]; then ok "cargo invoked once per binary with the exact arguments"; else bad "cargo invoked once per binary with the exact arguments"; cat "$FAKE_LOG"; fi
if has "alpha_native  *3  *0  *PASS" && has "beta_native  *0  *1  *FAIL" && has "TOTAL  *3  *1  *1 binaries failed"; then ok "final table has per-binary counts and totals"; else bad "final table has per-binary counts and totals"; printf '%s\n' "$out" | tail -n 8; fi

rm -f "$T/beta_native.rs"; printf '#[ignore = "native: b"]\nfn a() {}\n' > "$T/broken.rs"
PS_BIN="$tmp/bin/ps_yes.sh" run
if [ $rc -eq 1 ] && has "broken  *0  *0  *FAIL(no result: build error?)"; then ok "build error is reported as failure"; else bad "build error is reported as failure"; printf '%s\n' "$out" | tail -n 5; fi

rm -f "$T/broken.rs"
PS_BIN="$tmp/bin/ps_yes.sh" run
if [ $rc -eq 0 ] && has "TOTAL  *3  *0  *0 binaries failed"; then ok "all passing run exits 0"; else bad "all passing run exits 0"; fi

if [ -d "$T" ] && [ -f "$T/alpha_native.rs" ]; then ok "nothing in the tests tree was deleted"; else bad "nothing in the tests tree was deleted"; fi

rm -rf "$tmp"
if [ "$fails" -eq 0 ]; then echo "selftest PASSED"; else echo "selftest $fails FAILED"; fi
[ "$fails" -eq 0 ]
