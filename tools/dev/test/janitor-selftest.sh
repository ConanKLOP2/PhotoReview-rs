#!/usr/bin/env bash
set -u
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"; tmp=$(mktemp -d); fails=0
ok(){ echo "ok $1"; }; bad(){ echo "FAIL $1"; fails=$((fails+1)); }
base="$ROOT/.claude/worktrees"
mkdir -p "$tmp/bin" "$tmp/temp/codex-evidence" "$tmp/temp/mut-old" "$base/selftest-merged/target" "$base/selftest-open/target" "$base/selftest-busy/target" "$base/selftest-logdirty/target" "$base/selftest-locked" "$base/selftest-fallback" "$base/selftest-stale" "$base/selftest-squash"
python -c 'import os,sys,time; os.utime(sys.argv[1],(time.time()-7200,time.time()-7200))' "$base/selftest-locked"
cat > "$tmp/bin/git.sh" <<EOF
#!/bin/sh
case "\$*" in
*'worktree list'*) printf 'worktree %s\nbranch refs/heads/merged-branch\n\n' '$base/selftest-merged'; printf 'worktree %s\nbranch refs/heads/open-branch\n\n' '$base/selftest-open'; printf 'worktree %s\nbranch refs/heads/busy-branch\n\n' '$base/selftest-busy'; printf 'worktree %s\nbranch refs/heads/dirty-branch\n\n' '$base/selftest-logdirty'; printf 'worktree %s\nbranch refs/heads/locked-branch\nlocked initializing\n\n' '$base/selftest-locked'; printf 'worktree %s\nbranch refs/heads/stale-branch\n\n' '$base/selftest-stale'; printf 'worktree %s\nbranch refs/heads/squash-branch\n\n' '$base/selftest-squash'; printf 'worktree %s\nbranch refs/heads/fallback-branch\n' '$base/selftest-fallback';;
*'rev-parse --verify -q refs/heads/stale-branch'*) echo newtip;;
*'rev-parse --verify -q refs/heads/squash-branch'*) echo sqtip;;
*selftest-logdirty*'status --porcelain'*) echo ' M tracked.log';;
*'merge-base --is-ancestor merged-branch'*|*'merge-base --is-ancestor fallback-branch'*) exit 0;;
*'merge-base --is-ancestor '*) exit 1;;
*'worktree unlock'*) echo unlock >> '$tmp/actions';;
*'worktree remove'*selftest-fallback*) echo remove-fallback >> '$tmp/actions'; exit 1;;
*'worktree remove'*) echo remove >> '$tmp/actions'; for p do :; done; rm -rf "\$p";;
*'worktree prune'*) echo prune >> '$tmp/actions';;
*'branch -D'*) echo "branch-delete \$*" >> '$tmp/actions';;
*'for-each-ref'*'refs/claims/'*) printf 'W8-18m\nW8-20a\n';;
*'log -1 --format=%an refs/claims/W8-18m'*) echo 'Machine A';;
*'log -1 --format=%an refs/claims/W8-20a'*) echo 'Machine B';;
*'log origin/master --format=%s'*) echo 'Wave work W8-18l..18n, W8-13g, 13h (18a, 18b, 19a)';;
*'push -q origin :refs/claims/'*) for x do :; done; echo "claim-release \$x" >> '$tmp/actions';;
esac
EOF
cat > "$tmp/bin/gh.sh" <<'EOF'
#!/bin/sh
case "$*" in *open-branch*'--state open'*) echo '[{"number":4}]';; *stale-branch*'--state merged'*) echo '[{"number":7,"headRefOid":"oldtip"}]';; *squash-branch*'--state merged'*) echo '[{"number":8,"headRefOid":"sqtip"}]';; *) echo '[]';; esac
EOF
cat > "$tmp/bin/ps.sh" <<'EOF'
#!/bin/sh
case "$*" in *Get-PSDrive*) echo 42;; *Get-CimInstance*) [ "${PS_FAIL:-0}" = 1 ] && exit 1; [ -n "${BUSY_PATH:-}" ] && echo "$BUSY_PATH" || echo 'other.exe';; *) exit 1;; esac
EOF
cat > "$tmp/bin/du.sh" <<'EOF'
#!/bin/sh
[ "${SLOW_DU:-0}" = 1 ] && while :; do :; done
echo "5 $2"
EOF
cat > "$tmp/bin/cmd.sh" <<'EOF'
#!/bin/sh
echo fallback >> "$ACTIONS"
rm -rf "$FALLBACK_PATH"
EOF
chmod +x "$tmp/bin"/*
cp "$tmp/bin/git.sh" "$tmp/bin/git"
chmod +x "$tmp/bin/git"
export GIT_BIN="$tmp/bin/git.sh" GH_BIN="$tmp/bin/gh.sh" PS_BIN="$tmp/bin/ps.sh" DU_BIN="$tmp/bin/du.sh" DU_TIMEOUT=0.1 CMD_BIN="$tmp/bin/cmd.sh" TEMP="$tmp/temp" PR_MACHINE='Machine A' PATH="$tmp/bin:$PATH" ACTIONS="$tmp/actions" FALLBACK_PATH="$base/selftest-fallback"
BUSY_PATH=$(python -c 'import sys; p=sys.argv[1]; p=p[1].upper()+":\\"+p[3:] if p.startswith("/") and len(p)>2 and p[2]=="/" else p; print(p.replace("/","\\").upper())' "$base/selftest-busy") export BUSY_PATH
dry=$(bash "$ROOT/tools/dev/janitor.sh")
case "$dry" in *'worktree-locked'*selftest-locked*) ok 'old initializing lock candidate';; *) bad 'old initializing lock candidate';; esac
case "$dry" in *selftest-busy*) bad 'in-use process path suppressed';; *) ok 'in-use process path suppressed';; esac
case "$dry" in *selftest-logdirty*) bad 'tracked log modification blocks cleanup';; *) ok 'tracked log modification blocks cleanup';; esac
if printf '%s\n' "$dry" | awk -F '\t' '$1=="worktree" && $2 ~ /selftest-open/ { found=1 } END { exit !found }'; then bad 'open PR worktree never candidate'; else ok 'open PR worktree never candidate'; fi
if printf '%s\n' "$dry" | awk -F '\t' '$1=="worktree" && $2 ~ /selftest-stale/ { found=1 } END { exit !found }'; then bad 'older merged PR with different tip is not a candidate'; else ok 'older merged PR with different tip is not a candidate'; fi
if printf '%s\n' "$dry" | awk -F '\t' '$1=="worktree" && $2 ~ /selftest-squash/ { found=1 } END { exit !found }'; then ok 'squash-merged branch (PR head equals tip) is a candidate'; else bad 'squash-merged branch (PR head equals tip) is a candidate'; fi
[ ! -e "$tmp/actions" ] && ok 'dry-run makes no changes' || bad 'dry-run makes no changes'
PS_FAIL=1 export PS_FAIL
blocked=$(bash "$ROOT/tools/dev/janitor.sh")
case "$blocked" in *'process list unavailable'* ) ok 'unreadable process list reported';; *) bad 'unreadable process list reported';; esac
case "$blocked" in *'selftest-merged'*|*'selftest-locked'*) bad 'unreadable process list suppresses removals';; *) ok 'unreadable process list suppresses removals';; esac
unset PS_FAIL BUSY_PATH
SLOW_DU=1 export SLOW_DU
timed=$(bash "$ROOT/tools/dev/janitor.sh")
case "$timed" in *$'\t~'*) ok 'du timeout marked approximate';; *) bad 'du timeout marked approximate';; esac
unset SLOW_DU
ap=$(bash "$ROOT/tools/dev/janitor.sh" --apply)
case "$ap" in *'DISK BEFORE:'*'DISK AFTER:'*'FREED SIZE:'*) ok 'apply disk and freed-size report';; *) bad 'apply disk and freed-size report';; esac
case "$ap" in *'FAILED:'*) bad 'long-path fallback succeeded';; *) ok 'long-path fallback succeeded';; esac
grep -q '^unlock$' "$tmp/actions" && ok 'locked worktree unlocked before removal' || bad 'locked worktree unlocked before removal'
grep -q '^prune$' "$tmp/actions" && grep -q '^fallback$' "$tmp/actions" && ok 'remove failure prunes and uses long-path fallback' || bad 'remove failure prunes and uses long-path fallback'
[ -d "$base/selftest-stale" ] && ok 'apply keeps worktree whose merged PR has an older tip' || bad 'apply keeps worktree whose merged PR has an older tip'
grep -q 'branch-delete.*stale-branch' "$tmp/actions" && bad 'apply keeps branch whose merged PR has an older tip' || ok 'apply keeps branch whose merged PR has an older tip'
[ ! -d "$base/selftest-squash" ] && grep -q 'branch-delete.*squash-branch' "$tmp/actions" && ok 'apply removes squash-merged worktree and branch' || bad 'apply removes squash-merged worktree and branch'
[ -d "$base/selftest-open" ] && ok 'open PR worktree kept' || bad 'open PR worktree kept'
claims=$(bash "$ROOT/tools/dev/janitor.sh" --claims)
case "$claims" in *$'claim\tW8-18m'*) ok 'merged range claim listed for this machine';; *) bad 'merged range claim listed for this machine';; esac
case "$claims" in *'W8-20a'*) bad 'other machine claim protected';; *) ok 'other machine claim protected';; esac
bash "$ROOT/tools/dev/janitor.sh" --apply --claims >/dev/null
if grep -q 'claim-release.*W8-18m' "$tmp/actions"; then ok 'claim released through claim.sh'; else bad 'claim released through claim.sh'; fi
rm -rf "$base/selftest-merged" "$base/selftest-open" "$base/selftest-busy" "$base/selftest-logdirty" "$base/selftest-locked" "$base/selftest-fallback" "$base/selftest-stale" "$base/selftest-squash" "$tmp"
if [ "$fails" -eq 0 ]; then echo 'selftest PASSED'; else echo "selftest $fails FAILED"; fi; exit "$fails"
