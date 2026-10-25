#!/usr/bin/env bash
# Selftest for tools/dev/unsafe-audit.sh on fake crate trees (read-only on the real repo).
set -u
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp=$(mktemp -d); fails=0
ok(){ echo "ok $1"; }; bad(){ echo "FAIL $1"; fails=$((fails+1)); }
export UNSAFE_AUDIT_ROOT="$tmp/repo" UNSAFE_AUDIT_BASELINE="$tmp/base.txt"
W="$tmp/repo/crates/pr-platform-win/src"; C="$tmp/repo/crates/pr-core/src"
mkdir -p "$W" "$C" "$tmp/repo/crates/pr-platform-win/tests" "$tmp/repo/crates/pr-platform-win/target/debug"
audit(){ out=$(bash "$HERE/unsafe-audit.sh" "$@" 2>&1); rc=$?; }
expect(){ # name want_rc pattern
  if [ "$rc" -eq "$2" ] && { [ -z "${3:-}" ] || printf '%s' "$out" | grep -q -- "$3"; }; then ok "$1"; else bad "$1 (rc=$rc)"; printf '%s\n' "$out" | tail -n 5; fi
}
reset(){ rm -f "$UNSAFE_AUDIT_BASELINE" "$W"/*.rs "$C"/*.rs "$tmp/repo/crates/pr-platform-win/tests"/*.rs; }

reset
cat > "$W/good.rs" <<'EOF'
pub fn a() {
    // SAFETY: f has no preconditions.
    unsafe { f() }
}
pub fn b() {
    let x = unsafe {
        // SAFETY: g is sound here.
        g()
    };
}
/// Does a thing.
///
/// # Safety
/// The pointer must be valid.
pub unsafe fn c(p: *const u8) {}
// SAFETY: the foreign functions are declared with their real signatures.
unsafe extern "system" {
    fn f();
}
type Cb = unsafe extern "system" fn(u32) -> u32;
EOF
audit; expect "clean file with SAFETY comments passes" 0 "no new violations"

cat > "$W/strings.rs" <<'EOF'
#![forbid(unsafe_code)]
// unsafe { not code }
/* unsafe { nested /* comment */ } */
pub fn s() -> &'static str { let _c = 'u'; "unsafe { in a string }" }
pub const R: &str = r#"unsafe { raw " string }"#;
#[allow(unsafe_op_in_unsafe_fn_name_only)]
pub fn t() {}
EOF
audit; expect "unsafe in comments, strings and unsafe_code idents is ignored" 0 "no new violations"

reset
printf 'pub fn a() {\n    unsafe { f() }\n}\n' > "$W/bad.rs"
audit; expect "unsafe block without SAFETY fails (A)" 1 '\[A\] crates/pr-platform-win/src/bad.rs:2'

reset
printf 'pub unsafe fn d(p: *const u8) {}\n' > "$W/badfn.rs"
audit; expect "unsafe fn without # Safety fails (A)" 1 '\[A\].*(fn)'

reset
printf 'pub fn a() {\n    // SAFETY: ok.\n    unsafe { f() }\n}\n' > "$C/out.rs"
audit; expect "unsafe outside allowed crates fails (B)" 1 '\[B\] crates/pr-core/src/out.rs:3'

reset
printf 'pub fn a() {\n    // SAFETY: ok.\n    unsafe { f() }\n}\n' > "$tmp/repo/crates/pr-platform-win/tests/t.rs"
audit; expect "tests directory is scanned too (ok when documented)" 0 "no new violations"
printf 'pub fn a() {\n    unsafe { f() }\n}\n' > "$tmp/repo/crates/pr-platform-win/tests/t.rs"
audit; expect "undocumented unsafe in tests fails" 1 '\[A\] crates/pr-platform-win/tests/t.rs:2'

reset
printf 'unsafe { bad() }\n' > "$tmp/repo/crates/pr-platform-win/target/debug/gen.rs"
audit; expect "target directories are excluded" 0 "no new violations"

reset
printf 'pub fn a() {\n    unsafe { f() }\n}\n' > "$W/bad.rs"
audit --update-baseline; expect "update-baseline writes entries" 0 "1 entries"
audit; expect "baselined violation passes and is counted" 0 "baselined: 1 finding"
printf '\n\n\n// shifted\npub fn a() {\n    unsafe { f() }\n}\n' > "$W/bad.rs"
audit; expect "baseline survives line shifts" 0 "no new violations"
printf '\n// shifted\npub fn a() {\n    unsafe { f() }\n}\npub fn b() {\n    unsafe { f() }\n}\n' > "$W/bad.rs"
audit; expect "a second identical violation is NEW (counted once per entry)" 1 "NEW VIOLATIONS (1)"
printf 'pub fn a() {\n    // SAFETY: fixed.\n    unsafe { f() }\n}\n' > "$W/bad.rs"
audit; expect "fixed violation leaves an unused baseline entry, still passes" 0 "unused baseline entries: 1"

reset
cat > "$W/count.rs" <<'EOF'
pub fn a() { x.unwrap(); y.expect("m"); panic!("p"); }
pub fn b() { let _s = ".unwrap() in string"; } // x.unwrap()
#[cfg(test)]
mod tests {
    fn t() { x.unwrap(); x.unwrap(); panic!("t"); }
}
EOF
audit; expect "counts only non-test code (unwrap=1 expect=1 panic!=1)" 0 'unwrap()=1 expect(=1 panic!=1'

reset
audit --bogus; expect "unknown option is a usage error" 2 ""

PY_BIN=/nonexistent/python audit; if [ "$rc" -ne 0 ]; then ok "missing python fails instead of passing"; else bad "missing python fails instead of passing"; fi

rm -rf "$tmp"
if [ "$fails" -eq 0 ]; then echo "selftest PASSED"; else echo "selftest $fails FAILED"; fi
[ "$fails" -eq 0 ]
