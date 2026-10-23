#!/usr/bin/env bash
# Publish one implemented integration-branch unit as a small PR against master.
set -u
export PATH="$HOME/.cargo/bin:$PATH" PYTHONUTF8=1
SELF="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIB="python $SELF/lib/codex-lib.py"
COMMON="$(git -C "$SELF" rev-parse --path-format=absolute --git-common-dir)" || exit 2
REPO="$(dirname "$COMMON")"
# The main checkout may have a different owner from the agent's worktree.
git() { command git -c "safe.directory=$REPO" -c "safe.directory=${WT:-$REPO}" "$@"; }
PLAN_REL="${PLAN_REL:-}"
LOGDIR="$SELF/state/logs"
IBR=""
DRY=0
UNIT=""
usage() { echo "usage: $0 <unit e.g. W8-06f> [--ibr branch] [--dry-run]" >&2; exit 2; }
while [ "$#" -gt 0 ]; do
  case "$1" in
    --ibr) [ "$#" -ge 2 ] || usage; IBR="$2"; shift 2 ;;
    --dry-run) DRY=1; shift ;;
    -*) usage ;;
    *) [ -z "$UNIT" ] || usage; UNIT="$1"; shift ;;
  esac
done
[[ "$UNIT" =~ ^(W[0-9]+-[0-9][0-9][a-z]|W9-[A-Z][0-9]+)$ ]] || usage
if [ -z "$PLAN_REL" ]; then PLAN_REL="$(cd "$REPO" && $LIB planfor "$REPO" "$UNIT")" || exit 2; fi
if [ -z "$IBR" ]; then
  if [[ "$UNIT" == W9-* ]]; then IBR="integration/wave9-$(printf '%s' "${UNIT:3:1}" | tr '[:upper:]' '[:lower:]')"; else IBR="integration/wave8-b2"; fi
fi
git check-ref-format --branch "$IBR" >/dev/null || usage
LOWER="$(printf '%s' "$UNIT" | tr '[:upper:]' '[:lower:]')"
BR="pr/$LOWER"
WT="$REPO/.claude/worktrees/pr-$LOWER"
PLAN="$SELF/../../$PLAN_REL"
[ -f "$PLAN" ] || { echo "missing plan: $PLAN" >&2; exit 2; }
CONTRACT="$SELF/../../contracts/$UNIT.md"
[ -f "$CONTRACT" ] || { echo "missing contract: $CONTRACT" >&2; exit 2; }
mkdir -p "$LOGDIR"
LOG="$LOGDIR/$UNIT.pr.log"
: > "$LOG"
mapfile -t OWN < <($LIB owned "$CONTRACT" | tr -d '\r')
[ "${#OWN[@]}" -gt 0 ] || { echo "no owned files: $UNIT" >&2; exit 2; }
CRATE="$(printf '%s\n' "${OWN[@]}" | grep -m1 '^crates/' | cut -d/ -f2)"
[ -n "$CRATE" ] || { echo "cannot derive crate: $UNIT" >&2; exit 2; }
ROW="$($LIB row "$PLAN" "$UNIT" | tr -d '\r')" || exit 2
if [[ "$UNIT" == W9-* ]]; then BATCH="${UNIT:3:1}"; else BATCH="$(printf '%s' "$ROW" | awk -F '|' '{gsub(/^[ \t]+|[ \t]+$/, "", $9); print $9}')"; fi
TITLE="$(printf '%s' "$ROW" | awk -F '|' '{gsub(/^[ \t]+|[ \t]+$/, "", $3); print $3}')"
TITLE="$(printf '%s' "$TITLE" | sed -E 's/`//g; s/ = [0-9]+.*$//')"
[ -n "$TITLE" ] || TITLE="$UNIT"
echo "$UNIT: fetching origin and checking $BATCH dependencies"
if ! git -C "$REPO" fetch -q origin >>"$LOG" 2>&1; then
  if [ "$DRY" -eq 1 ] && git -C "$REPO" rev-parse --verify origin/master >/dev/null 2>&1; then
    echo "dry-run: fetch unavailable; checking cached origin/master"
  else
    tail -n 12 "$LOG"; exit 2
  fi
fi
for DEP in $($LIB deps "$PLAN" "$UNIT" | tr -d '\r'); do
  DEP_ROW="$($LIB row "$PLAN" "$DEP" 2>/dev/null || :)"
  [ -n "$DEP_ROW" ] || continue
  if [[ "$DEP" == W9-* ]]; then DEP_BATCH="${DEP:3:1}"; else DEP_BATCH="$(printf '%s' "$DEP_ROW" | awk -F '|' '{gsub(/^[ \t]+|[ \t]+$/, "", $9); print $9}')"; fi
  [ "$DEP_BATCH" = "$BATCH" ] || continue
  DEP_CONTRACT="$SELF/../../contracts/$DEP.md"
  [ -f "$DEP_CONTRACT" ] || { echo "waiting on $DEP (contract missing)"; exit 3; }
  mapfile -t DEP_OWN < <($LIB owned "$DEP_CONTRACT" | tr -d '\r')
  for F in "${DEP_OWN[@]}"; do
    case "$F" in crates/*/src/*.rs) ;; *) continue ;; esac
    if ! git -C "$REPO" cat-file -e "origin/master:$F" 2>/dev/null; then
      echo "waiting on $DEP ($F missing on origin/master)"; exit 3
    fi
    if git -C "$REPO" show "origin/master:$F" | grep -qF 'todo!('; then
      echo "waiting on $DEP ($F still has todo!)"; exit 3
    fi
  done
done
if [[ "$UNIT" == W9-* ]]; then
  while IFS= read -r DEP; do
    [ -n "$DEP" ] || continue
    MISSING="$($LIB extsat "$REPO" "$SELF/state" "$DEP")"
    [ -z "$MISSING" ] || { echo "waiting on $DEP (not on master)"; exit 3; }
  done < <($LIB extdeps "$PLAN" "$UNIT")
fi
REMOTE_HEAD="$(git -C "$REPO" ls-remote --heads origin "$BR" 2>>"$LOG")" || {
  echo "cannot check remote branch $BR:"; tail -n 12 "$LOG"; exit 2;
}
if [ -n "$REMOTE_HEAD" ]; then
  echo "already open: $BR"
  exit 0
fi
git -C "$REPO" rev-parse --verify "$IBR^{commit}" >/dev/null 2>&1 || { echo "missing integration branch: $IBR" >&2; exit 2; }
CREATED=0
if [ -d "$WT" ]; then
  git -C "$WT" rev-parse --is-inside-work-tree >/dev/null 2>&1 || { echo "occupied path: $WT" >&2; exit 2; }
  CURRENT_BRANCH="$(git -C "$WT" symbolic-ref --quiet --short HEAD 2>/dev/null || :)"
  [ "$CURRENT_BRANCH" = "$BR" ] || { echo "occupied worktree uses $CURRENT_BRANCH: $WT" >&2; exit 2; }
  [ -z "$(git -C "$WT" status --porcelain -uall)" ] || { echo "worktree has changes: $WT" >&2; exit 2; }
  git -C "$REPO" worktree remove "$WT" >>"$LOG" 2>&1 || { tail -n 12 "$LOG"; exit 2; }
fi
if git -C "$REPO" show-ref --verify --quiet "refs/heads/$BR"; then
  [ "$(git -C "$REPO" rev-parse "$BR")" = "$(git -C "$REPO" rev-parse origin/master)" ] || {
    echo "existing local branch differs from origin/master: $BR" >&2; exit 2;
  }
  git -C "$REPO" worktree add -q "$WT" "$BR" >>"$LOG" 2>&1 || { tail -n 12 "$LOG"; exit 2; }
else
  git -C "$REPO" worktree add -q -b "$BR" "$WT" origin/master >>"$LOG" 2>&1 || { tail -n 12 "$LOG"; exit 2; }
  CREATED=1
fi
copy_from_ibr() {
  local F="$1"
  case "$F" in
    crates/*) ;;
    *) echo "unsafe owned path: $F" >&2; exit 2 ;;
  esac
  case "$F" in *'..'*|*\\*) echo "unsafe owned path: $F" >&2; exit 2 ;; esac
  git -C "$REPO" cat-file -e "$IBR:$F" 2>/dev/null || return 1
  mkdir -p "$WT/$(dirname "$F")"
  git -C "$REPO" show "$IBR:$F" | sed 's/\r$//' > "$WT/$F"
}
COPIED=0
for F in "${OWN[@]}"; do
  case "$F" in
    # the unit's own tests/*.rs ARE copied; synced assets, goldens and fixtures never are
    assets/*|*/assets/*|*/golden/*|*/goldens/*|*/fixtures/*|tests/golden/*|tests/fixtures/*) continue ;;
  esac
  if copy_from_ibr "$F"; then COPIED=$((COPIED+1)); fi
done
[ "$COPIED" -gt 0 ] || { echo "nothing to copy: no owned file of $UNIT exists on $IBR" >&2; exit 2; }
TA="$($LIB testargs "$WT" "$CONTRACT" | tr -d '\r')"
read -r -a TEST_ARGS <<< "$TA"
EXTRA=()
run_tests() {
  echo "$UNIT: cargo test round $1 (output: $LOG)"
  ( cd "$WT" && unset CARGO_TARGET_DIR && export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 &&
    timeout 900 cargo test -p "$CRATE" --locked --no-fail-fast "${TEST_ARGS[@]}" ) >"$LOG" 2>&1
}
for ROUND in 1 2 3 4; do
  if run_tests "$ROUND"; then break; fi
  # a real compile error only: a failing test run also prints "error: test failed, to rerun pass ..."
  if grep -Eq '^error\[E[0-9]+\]|^error: could not compile' "$LOG"; then
    echo "compile error:"; tail -n 16 "$LOG"; exit 5
  fi
  if ! grep -q 'not yet implemented' "$LOG"; then
    echo "failing tests:"; grep -E '^    .* --- FAILED|^failures:|^test result:' "$LOG" | tail -n 16
    tail -n 8 "$LOG"; exit 6
  fi
  mapfile -t MODULES < <(grep -E -A 2 'panicked at|not yet implemented' "$LOG" |
    grep -oE 'crates[\\/][^ :]+\.rs' | tr '\\' '/' | sort -u)
  [ "${#MODULES[@]}" -gt 0 ] || { echo "unresolved modules:"; tail -n 16 "$LOG"; exit 4; }
  ADDED=0
  for F in "${MODULES[@]}"; do
    case "$F" in crates/*/src/*.rs) ;; *) echo "uncopyable module: $F"; exit 4 ;; esac
    if ! git -C "$REPO" cat-file -e "$IBR:$F" 2>/dev/null ||
       git -C "$REPO" show "$IBR:$F" | grep -qF 'todo!('; then
      echo "uncopyable module: $F"; tail -n 12 "$LOG"; exit 4
    fi
    if ! cmp -s "$WT/$F" <(git -C "$REPO" show "$IBR:$F" | sed 's/\r$//'); then
      copy_from_ibr "$F" || { echo "uncopyable module: $F"; exit 4; }
      EXTRA+=("$F")
      ADDED=$((ADDED+1))
    fi
  done
  if [ "$ADDED" -eq 0 ] || [ "$ROUND" -eq 4 ]; then
    echo "unresolved modules after $ROUND rounds: ${MODULES[*]}"; tail -n 12 "$LOG"; exit 4
  fi
done
mapfile -t CHANGED < <(cd "$WT" && { git diff --name-only -- crates/; git ls-files --others --exclude-standard -- crates/; } | sort -u)
RS=()
for F in "${CHANGED[@]}"; do case "$F" in *.rs) RS+=("$WT/$F") ;; esac; done
if [ "${#RS[@]}" -gt 0 ]; then
  printf '%s\0' "${RS[@]}" | xargs -0 -n 40 rustfmt --edition 2024 >>"$LOG" 2>&1 || {
    echo "rustfmt failed:"; tail -n 12 "$LOG"; exit 7;
  }
fi
echo "$UNIT: cargo clippy (output: $LOG)"
( cd "$WT" && unset CARGO_TARGET_DIR && export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 &&
  timeout 900 cargo clippy -p "$CRATE" --all-targets --locked -- -D warnings ) >"$LOG" 2>&1 || {
  echo "clippy failed:"; tail -n 16 "$LOG"; exit 7;
}
run_tests final || { echo "final test run failed:"; tail -n 16 "$LOG"; exit 6; }
mapfile -t CHANGED < <(cd "$WT" && { git diff --name-only -- crates/; git ls-files --others --exclude-standard -- crates/; } | sort -u)
COUNT="${#CHANGED[@]}"
EXTRA_LIST="${EXTRA[*]:-none}"
TEST_COUNTS="$(grep '^test result: ok' "$LOG" | sed -nE 's/^test result: ok\. ([0-9]+) passed; ([0-9]+) failed; ([0-9]+) ignored;.*/\1 passed, \2 failed, \3 ignored/p' | paste -sd ';' -)"
if [ "$DRY" -eq 1 ]; then
  echo "DRY_RUN_OK files=$COUNT extra_modules=$EXTRA_LIST"
  # a dry run leaves nothing behind: remove the worktree and the branch this run created (a real run needs a clean slate)
  if [ "${CREATED:-0}" = 1 ]; then
    git -C "$REPO" worktree remove --force "$WT" >>"$LOG" 2>&1
    git -C "$REPO" branch -D "$BR" >>"$LOG" 2>&1
  fi
  exit 0
fi
[ "$COUNT" -gt 0 ] || { echo "no crate changes to commit" >&2; exit 2; }
SUBJECT="feat($UNIT): $TITLE"
BODY="Extra modules copied: $EXTRA_LIST"
( cd "$WT" && git add -- "${CHANGED[@]}" &&
  git commit -m "$SUBJECT" -m "$BODY" -m 'Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>' &&
  git push -u origin "$BR" ) >>"$LOG" 2>&1 || { tail -n 16 "$LOG"; exit 2; }
PR_BODY="Owned files: $COPIED
Tests: cargo test -p $CRATE --locked --no-fail-fast $TA ($TEST_COUNTS)
Extra modules: $EXTRA_LIST

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
PR_URL="$(gh pr create --base master --head "$BR" --title "$SUBJECT" --body "$PR_BODY" 2>>"$LOG")" || { tail -n 16 "$LOG"; exit 2; }
PR_NUMBER="$(gh pr view "$PR_URL" --json number --jq .number 2>>"$LOG")" || { tail -n 16 "$LOG"; exit 2; }
if ! gh pr merge "$PR_NUMBER" --auto --squash >>"$LOG" 2>&1; then
  if grep -qi 'clean status' "$LOG"; then
    gh pr merge "$PR_NUMBER" --squash >>"$LOG" 2>&1 || { tail -n 16 "$LOG"; exit 2; }
  else
    tail -n 16 "$LOG"; exit 2
  fi
fi
echo "PR_URL=$PR_URL"
