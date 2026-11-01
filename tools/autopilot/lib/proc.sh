#!/usr/bin/env bash
# Small process/lock helpers shared by codex-pipeline.sh and unit-pr.sh and covered by test/proc-selftest.sh (review autopilot M5, M6).

# kill_tree <pid>: terminates the msys/POSIX descendants of <pid> first (a bash subshell defers its TERM trap until its foreground child
# exits, so the child must go first), then <pid> itself (TERM, then KILL when it is still alive after ~2 s). Only PIDs this script
# started are passed in; never kills by image name. Native Windows children (codex.exe, cargo.exe) are not visible to msys `ps` as
# children and are NOT reached; they end by their own timeout (known limitation, review autopilot M6).
_kids() { { ps -o pid= --ppid "$1" 2>/dev/null || ps -e 2>/dev/null | awk -v pp="$1" 'NR>1 && $2==pp{print $1}'; } | tr -d ' '; }
kill_tree() {
  local p="$1" c k i
  [ -n "$p" ] || return 0
  for c in $(_kids "$p"); do
    for k in $(_kids "$c"); do kill "$k" 2>/dev/null; done
    kill "$c" 2>/dev/null
  done
  kill "$p" 2>/dev/null
  for i in 1 2 3 4; do kill -0 "$p" 2>/dev/null || return 0; sleep 0.5; done
  kill -9 "$p" 2>/dev/null
  return 0
}

# lock_stale <dir>: 0 when the lock directory is stale: its recorded pid is dead, or it has no pid file for over a minute (the holder
# died between `mkdir` and the pid write). A fresh lock without a pid is NOT stale (the holder is probably about to write it).
lock_stale() {
  local l="$1" op
  [ -d "$l" ] || return 1
  op="$(cat "$l/pid" 2>/dev/null)"
  if [ -n "$op" ]; then kill -0 "$op" 2>/dev/null && return 1; return 0; fi
  [ -n "$(find "$l" -maxdepth 0 -mmin +"${LOCK_GRACE_MIN:-1}" 2>/dev/null)" ]
}

# lock_reclaim <dir>: removes a stale lock. The directory is first RENAMED (only one of several reclaimers can win the rename) and
# re-checked after the move; a lock that turned out to be live is put back. Returns 0 when the caller may retry `mkdir`.
lock_reclaim() {
  local l="$1" tmp="$1.stale.$$"
  lock_stale "$l" || return 1
  mv "$l" "$tmp" 2>/dev/null || return 1
  if lock_stale "$tmp"; then rm -rf "$tmp"; return 0; fi
  if [ ! -e "$l" ]; then mv "$tmp" "$l" 2>/dev/null; else :; fi
  return 1
}

# Paths the owned-files check ignores: Codex's own scratch files in the repo root and cargo target dirs (review autopilot L7: a root
# file merely named like `target.rs` or `.codex-x/` content is no longer exempt).
GATE_IGNORE_RE='^(\.codex-[^/]*$|target(-[^/]*)?/)'
