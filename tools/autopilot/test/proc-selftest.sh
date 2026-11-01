#!/usr/bin/env bash
# Selftest of lib/proc.sh and lib/unitpr.sh plus the codex-usage.sh argument guard (no cargo, no network, no real gh/Codex).
# Covers review autopilot M5 (lock reclaim), M6 (kill_tree), M7 (unit-pr cleanup / branch state), L7 (gate ignore regex), L9 (usage guard).
# Usage: bash tools/autopilot/test/proc-selftest.sh
set -u
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"; SRC="$(cd "$HERE/.." && pwd)"
T="$(mktemp -d)"; FAILS=0
ok() { echo "  ok   $*"; }
bad() { echo "  FAIL $*"; FAILS=$((FAILS+1)); }
check() { if eval "$2"; then ok "$1"; else bad "$1  [$2]"; fi; }
. "$SRC/lib/proc.sh"; . "$SRC/lib/unitpr.sh"

echo "== M6 kill_tree"
( sleep 300 ) & SUB=$!          # a subshell whose foreground child would defer a plain TERM
sleep 1
kill_tree "$SUB"; wait "$SUB" 2>/dev/null
check "kill_tree stops the subshell" '! kill -0 "$SUB" 2>/dev/null'
check "kill_tree stops the subshell's msys child" '[ -z "$(ps -e | awk "NR>1 && \$2==$SUB{print \$1}")" ]'
kill_tree ""; check "kill_tree with an empty pid is a no-op" '[ $? -eq 0 ]'

echo "== M5 lock_stale / lock_reclaim"
L="$T/lock"; mkdir "$L"
check "fresh lock without pid file is NOT stale (holder about to write it)" '! lock_stale "$L"'
check "lock_reclaim leaves a fresh pid-less lock alone" '! lock_reclaim "$L" && [ -d "$L" ]'
touch -d '5 minutes ago' "$L"
check "pid-less lock older than the grace period is stale" 'lock_stale "$L"'
check "lock_reclaim removes the stale pid-less lock atomically" 'lock_reclaim "$L" && [ ! -e "$L" ] && [ -z "$(ls "$T" | grep "lock.stale")" ]'
mkdir "$L"; echo 999999 > "$L/pid"
check "lock with a dead pid is stale" 'lock_stale "$L"'
lock_reclaim "$L"; check "dead-pid lock is reclaimed" '[ ! -e "$L" ]'
mkdir "$L"; echo "$$" > "$L/pid"
check "lock owned by a live pid is not stale and not reclaimed" '! lock_stale "$L" && ! lock_reclaim "$L" && [ -d "$L" ]'
rm -rf "$L"
# two reclaimers on one stale lock: exactly one wins the rename
mkdir "$L"; touch -d '5 minutes ago' "$L"
( lock_reclaim "$L"; echo $? > "$T/r1" ) & ( lock_reclaim "$L"; echo $? > "$T/r2" ) & wait
check "two concurrent reclaimers: the lock is gone and no stale leftovers remain" '[ ! -e "$L" ] && [ -z "$(ls "$T" | grep "lock.stale")" ]'

echo "== L7 gate ignore regex"
check "root .codex-* scratch file is ignored" 'printf ".codex-last-message.txt\n" | grep -v -E "$GATE_IGNORE_RE" | wc -l | grep -qx 0'
check "target/ and target-x/ files are ignored" 'printf "target/debug/a\ntarget-mut/b\n" | grep -v -E "$GATE_IGNORE_RE" | wc -l | grep -qx 0'
check "a root file merely starting with target is NOT ignored" '[ "$(printf "target.rs\ncrates/x/target_dir.rs\n" | grep -v -E "$GATE_IGNORE_RE" | wc -l)" = 2 ]'

echo "== L9 codex-usage.sh argument guard"
out="$(bash "$SRC/codex-usage.sh" "95; import os" 2>&1)"; rc=$?
check "non-numeric threshold is refused with exit 2 before anything runs" '[ $rc = 2 ] && echo "$out" | grep -q "bad threshold"'

echo "== M7 unitpr_cleanup / unitpr_branch_state"
git init -q -b master "$T/repo"; git -C "$T/repo" -c user.name=t -c user.email=t@e.com commit -q --allow-empty -m init
LOG="$T/log"; : > "$LOG"
git -C "$T/repo" worktree add -q -b pr/x "$T/wt1"; echo dirty > "$T/wt1/f"
unitpr_cleanup "$T/repo" "$T/wt1" pr/x 1 0 "$LOG"
check "failed run: worktree and local branch created by the run are removed" '[ ! -d "$T/wt1" ] && ! git -C "$T/repo" show-ref --verify --quiet refs/heads/pr/x'
git -C "$T/repo" worktree add -q -b pr/y "$T/wt2"; echo dirty > "$T/wt2/f"
unitpr_cleanup "$T/repo" "$T/wt2" pr/y 1 1 "$LOG"
check "after a successful push the worktree and branch are kept" '[ -d "$T/wt2" ] && git -C "$T/repo" show-ref --verify --quiet refs/heads/pr/y'
git -C "$T/repo" worktree add -q -b pr/z "$T/wt3"; echo dirty > "$T/wt3/f"
unitpr_cleanup "$T/repo" "$T/wt3" pr/z 0 0 "$LOG"
check "a worktree that existed before the run is never touched" '[ -d "$T/wt3" ]'
mkdir -p "$T/bin"
printf '#!/usr/bin/env bash\ncase "$*" in *--head\ open*) echo 2;; *--head\ none*) echo 0;; *) exit 1;; esac\n' > "$T/bin/gh"; chmod +x "$T/bin/gh"
PATH="$T/bin:$PATH"
check "branch with an open PR is reported open" '[ "$(unitpr_branch_state open "$LOG")" = open ]'
check "pushed branch without an open PR is reported none (no false 'already open')" '[ "$(unitpr_branch_state none "$LOG")" = none ]'
check "gh failure is reported unknown (old behaviour kept)" '[ "$(unitpr_branch_state other "$LOG")" = unknown ]'
cd "$T/.." || true; rm -rf "$T"
[ "$FAILS" -eq 0 ] && echo "proc-selftest: all passed" || { echo "proc-selftest: $FAILS failed"; exit 1; }
