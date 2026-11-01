#!/usr/bin/env bash
# Helpers of unit-pr.sh, covered by test/proc-selftest.sh (review autopilot M7).

# unitpr_cleanup <repo> <worktree> <branch> <created 0|1> <pushed 0|1> <log>: after a FAILED run, removes the worktree and the local
# branch this run created (nothing was pushed, so nothing is lost) so that the next run does not stop at "worktree has changes".
# A worktree/branch that existed before the run, or whose commit was already pushed, is left alone.
unitpr_cleanup() {
  local repo="$1" wt="$2" br="$3" created="$4" pushed="$5" log="$6"
  [ "$created" = 1 ] && [ "$pushed" != 1 ] || return 0
  command git -C "$repo" worktree remove --force "$wt" >>"$log" 2>&1
  command git -C "$repo" branch -D "$br" >>"$log" 2>&1
  return 0
}

# unitpr_branch_state <branch> <log>: prints `open` when an open PR has this head, `none` when gh says there is none, `unknown` when
# gh cannot tell. A pushed branch whose PR was merged or closed used to be reported as "already open" (exit 0, no PR at all).
unitpr_branch_state() {
  local br="$1" log="$2" n
  n="$(gh pr list --head "$br" --state open --json number --jq length 2>>"$log")" || { echo unknown; return 0; }
  case "$n" in ''|*[!0-9]*) echo unknown;; 0) echo none;; *) echo open;; esac
}
