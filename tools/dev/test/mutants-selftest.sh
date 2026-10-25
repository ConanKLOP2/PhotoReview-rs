#!/usr/bin/env bash
set -u
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"; tmp=$(mktemp -d); fails=0
ok(){ echo "ok $1"; }; bad(){ echo "FAIL $1"; fails=$((fails+1)); }
mkdir -p "$tmp/temp"
# Fake cargo-mutants: logs args and env, writes mutants.out under --output DIR according to $FAKE_MODE.
cat > "$tmp/fakecargo" <<'EOF'
#!/usr/bin/env bash
echo "ARGS: $*" >> "$FAKE_LOG"; echo "TARGET: $CARGO_TARGET_DIR" >> "$FAKE_LOG"
od=""; prev=""; for a in "$@"; do [ "$prev" = --output ] && od="$a"; prev="$a"; done
[ "${FAKE_MODE:-}" = slow ] && sleep 4
[ "${FAKE_MODE:-}" = none ] && exit 4
mkdir -p "$od/mutants.out"
case "$*" in
 *' -F '*) cat > "$od/mutants.out/outcomes.json" <<'J'
{"outcomes":[{"scenario":{"Mutant":{"name":"crates/x/src/a.rs:10:5: replace f -> bool with true","file":"crates/x/src/a.rs"}},"summary":"MissedMutant"}],"total_mutants":1}
J
 exit 2;;
esac
cat > "$od/mutants.out/outcomes.json" <<'J'
{"outcomes":[
 {"scenario":"Baseline","summary":"Success"},
 {"scenario":{"Mutant":{"name":"replace f -> bool with true","file":"crates/x/src/a.rs","span":{"start":{"line":10,"column":5}}}},"summary":"MissedMutant"},
 {"scenario":{"Mutant":{"name":"crates/x/src/a.rs:20:9: replace == with != in g","file":"crates/x/src/a.rs"}},"summary":"MissedMutant"},
 {"scenario":{"Mutant":{"name":"crates/x/src/a.rs:30:9: replace h with ()","file":"crates/x/src/a.rs"}},"summary":"CaughtMutant"},
 {"scenario":{"Mutant":{"name":"crates/x/src/a.rs:31:9: replace i with ()","file":"crates/x/src/a.rs"}},"summary":"CaughtMutant"},
 {"scenario":{"Mutant":{"name":"crates/x/src/a.rs:40:9: replace j","file":"crates/x/src/a.rs"}},"summary":"Unviable"},
 {"scenario":{"Mutant":{"name":"crates/x/src/a.rs:50:9: replace k","file":"crates/x/src/a.rs"}},"summary":"Timeout"}
],"total_mutants":8}
J
exit 2
EOF
chmod +x "$tmp/fakecargo"
export CARGO_BIN="$tmp/fakecargo" TEMP="$tmp/temp" FAKE_LOG="$tmp/log"
M="bash $ROOT/tools/dev/mutants.sh"
: > "$tmp/log"
out=$($M pr-x Cargo.toml --tests a,b --budget 60 2>&1); rc=$?
[ $rc -eq 0 ] && ok exit-ok || { bad exit-ok; echo "$out"; }
grep -q 'ARGS: mutants -p pr-x --baseline skip --timeout 90 -j 1 --output .* --file Cargo.toml -- --test a --test b' "$tmp/log" && ok cargo-args || { bad cargo-args; cat "$tmp/log"; }
grep -q "TARGET: $tmp/temp/mut-target-pr-x" "$tmp/log" && ok target-outside-worktree || { bad target-outside-worktree; cat "$tmp/log"; }
case "$tmp/temp" in "$ROOT"*) bad temp-not-in-root;; *) ok temp-not-in-root;; esac
echo "$out" | grep -E -q 'caught +2' && echo "$out" | grep -E -q 'missed +2' && echo "$out" | grep -E -q 'unviable +1' && echo "$out" | grep -E -q 'timeout +1' && ok table || { bad table; echo "$out"; }
echo "$out" | grep -q 'NOT RUN: 2 mutants' && ok partial-note || bad partial-note
od=$(ls -d "$tmp"/temp/mut-out-pr-x-* | head -1)
if [ "$(wc -l < "$od/survivors.txt" | tr -d ' ')" = 2 ] && grep -q '^crates/x/src/a.rs:10:5: replace f -> bool with true$' "$od/survivors.txt" && grep -q '^crates/x/src/a.rs:20:9: replace == with != in g$' "$od/survivors.txt"; then ok survivors-txt; else bad survivors-txt; cat "$od/survivors.txt"; fi
# rerun survivors
: > "$tmp/log"
out=$($M pr-x Cargo.toml --rerun-survivors --budget 60 2>&1)
n=$(grep -o -- ' -F ' "$tmp/log" | wc -l | tr -d ' ')
[ "$n" = 2 ] && ok rerun-two-filters || { bad "rerun-two-filters ($n)"; cat "$tmp/log"; }
grep -F -q -- '-F crates/x/src/a\.rs:20:9: replace == with != in g' "$tmp/log" && ok filter-escaped || { bad filter-escaped; cat "$tmp/log"; }
echo "$out" | grep -q 'rerun: mutation results' && echo "$out" | grep -q 'survivors-rerun.txt: 1 line' && ok rerun-table || { bad rerun-table; echo "$out"; }
# missing outcomes.json
: > "$tmp/log"
out=$(FAKE_MODE=none $M pr-x Cargo.toml --budget 60 2>&1); rc=$?
echo "$out" | grep -q 'no outcomes.json' && ok missing-outcomes || { bad missing-outcomes; echo "$out"; }
[ $rc -eq 4 ] && ok baseline-exit-propagated || bad "baseline-exit-propagated ($rc)"
# budget
out=$(FAKE_MODE=slow $M pr-x Cargo.toml --budget 1 2>&1)
echo "$out" | grep -q 'BUDGET EXHAUSTED after 1s' && ok budget-partial || { bad budget-partial; echo "$out"; }
# jobs flag, no tests
: > "$tmp/log"; $M pr-x Cargo.toml --jobs 2 >/dev/null 2>&1
grep -q -- '-j 2 ' "$tmp/log" && ok jobs-flag || bad jobs-flag
# usage / validation
$M >/dev/null 2>&1; [ $? -eq 2 ] && ok usage-exit2 || bad usage-exit2
$M pr-x nofile.rs >/dev/null 2>&1; [ $? -eq 2 ] && ok missing-source-file || bad missing-source-file
$M pr-x Cargo.toml --budget abc >/dev/null 2>&1; [ $? -eq 2 ] && ok bad-budget || bad bad-budget
$M 'pr x' Cargo.toml >/dev/null 2>&1; [ $? -eq 2 ] && ok bad-crate || bad bad-crate
rm -rf "$tmp"
if [ $fails -eq 0 ]; then echo "selftest PASSED"; else echo "selftest $fails FAILED"; exit 1; fi
