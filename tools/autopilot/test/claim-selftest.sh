#!/usr/bin/env bash
# Selftest of claim.sh against a throw-away bare origin (no network, never touches the real origin). Covers review autopilot H1 (a claim
# released by another machine must vanish locally after fetch --prune) and M4/P-89 (gc frees a claim only for feat(<id>): / Merge <id> / Complete <id> subjects, and by age only with --stale).
# Usage: bash tools/autopilot/test/claim-selftest.sh
set -u
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"; SRC="$(cd "$HERE/.." && pwd)"
T="$(mktemp -d)"; FAILS=0
ok() { echo "  ok   $*"; }
bad() { echo "  FAIL $*"; FAILS=$((FAILS+1)); }
check() { if eval "$2"; then ok "$1"; else bad "$1  [$2]"; fi; }
git init -q --bare "$T/origin.git"
for m in A B; do
  git init -q -b master "$T/$m"; git -C "$T/$m" remote add origin "$T/origin.git"
  mkdir -p "$T/$m/tools/autopilot"; cp "$SRC/claim.sh" "$T/$m/tools/autopilot/claim.sh"
done
ca() { PR_MACHINE=A bash "$T/A/tools/autopilot/claim.sh" "$@"; }
cb() { PR_MACHINE=B bash "$T/B/tools/autopilot/claim.sh" "$@"; }
ca claim U1 >/dev/null; check "A claims U1" "[ \$? -eq 0 ]"
cb claim U1 >/dev/null; rc=$?; check "B is refused (exit 1)" "[ $rc -eq 1 ]"
ca release U1 >/dev/null
cb claim U1 >/dev/null; rc=$?; check "H1: B claims U1 after A released (no ghost claim)" "[ $rc -eq 0 ]"
check "H1: who U1 is B" "[ \"\$(ca who U1)\" = B ]"
cb release U1 >/dev/null
check "H1: A's local copy has no ghost ref after release" "[ -z \"\$(ca who U1)\" ] && [ -z \"\$(git -C $T/A for-each-ref refs/claims/)\" ]"
# release safety: another machine's claim is refused without --force, stays on origin, and --force removes it
ca claim U2 >/dev/null
cb release U2 >/dev/null; rc=$?
check "release: B is refused (exit 1) for A's claim" "[ $rc -eq 1 ]"
check "release: A's claim survives the refused release" "[ \"\$(cb who U2)\" = A ]"
cb release U2 --force >/dev/null
check "release --force: the claim is gone for both machines" "[ -z \"\$(cb who U2)\" ] && [ -z \"\$(ca who U2)\" ]"
# gc on the fake origin only (never the real one). P-89 option A: only a commit SUBJECT feat(<id>): / Merge <id> / Complete <id> frees a
# claim; a mention, a partial/wip commit or a regex look-alike does not; age alone frees nothing unless gc --stale is given.
commit_master() { git -C "$T/A" -c user.name=t -c user.email=t@e.com commit -q --allow-empty -m "$1"; }
commit_master "feat(UX1): unrelated unit"
commit_master "docs: mention LIVE-7 and U.1 and MENT-1 in a subject"
commit_master "feat(PART-1): first half (partial)"
commit_master "feat(WIP-1): wip red tests"
commit_master "test(RED-1): red tests for the unit"
git -C "$T/A" -c user.name=t -c user.email=t@e.com commit -q --allow-empty -m "refactor: touches DONE-1 only" -m "body says feat(BODY-1): done"
git -C "$T/A" push -q origin HEAD:refs/heads/master
for id in U.1 LIVE-7 MENT-1 PART-1 WIP-1 RED-1 DONE-1 BODY-1 DONE-2 DONE-3 DONE-4 DONE-5; do ca claim "$id" >/dev/null; done
ca gc >/dev/null 2>&1
check "gc: regex look-alike U.1 is not freed by a commit that only says UX1 (--fixed-strings)" "[ \"\$(ca who U.1)\" = A ]"
check "gc: a fresh claim that no master commit mentions is kept" "[ \"\$(ca who LIVE-7)\" = A ]"
check "gc: a subject that only mentions the unit does not free it" "[ \"\$(ca who MENT-1)\" = A ]"
check "gc: a partial commit feat(PART-1) does not free it" "[ \"\$(ca who PART-1)\" = A ]"
check "gc: a wip / red tests commit does not free it" "[ \"\$(ca who WIP-1)\" = A ] && [ \"\$(ca who RED-1)\" = A ]"
check "gc: a mention in a refactor subject or in a body does not free it" "[ \"\$(ca who DONE-1)\" = A ] && [ \"\$(ca who BODY-1)\" = A ]"
commit_master "feat(DONE-2): finish the unit"
commit_master "Merge DONE-3 from the integration branch"
commit_master "Complete DONE-4"
commit_master "feat(OTHER-9,DONE-5): two units in one scope"
git -C "$T/A" push -q origin HEAD:refs/heads/master
ca gc >/dev/null 2>&1
check "gc: feat(<id>): frees the claim" "[ -z \"\$(ca who DONE-2)\" ] && [ -z \"\$(cb who DONE-2)\" ]"
check "gc: Merge <id> frees the claim" "[ -z \"\$(ca who DONE-3)\" ]"
check "gc: Complete <id> frees the claim" "[ -z \"\$(ca who DONE-4)\" ]"
check "gc: feat(A,<id>): frees the claim of a unit named in a multi-unit scope" "[ -z \"\$(ca who DONE-5)\" ]"
check "gc: the claims that did not finish are still held after the gc run" "[ \"\$(ca who PART-1)\" = A ] && [ \"\$(ca who LIVE-7)\" = A ]"
# age: a claim whose commit is 20 days old (pushed by hand with a back-dated committer date) stays without --stale and goes with it
old=$(date -u -d '20 days ago' +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u -v-20d +%Y-%m-%dT%H:%M:%SZ)
oc=$(GIT_AUTHOR_NAME=A GIT_AUTHOR_EMAIL=claims@local GIT_COMMITTER_NAME=A GIT_COMMITTER_EMAIL=claims@local GIT_AUTHOR_DATE="$old" GIT_COMMITTER_DATE="$old" \
     git -C "$T/A" commit-tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904 -m "claim OLD-1 by A")
git -C "$T/A" push -q origin "$oc:refs/claims/OLD-1"
check "gc setup: the back-dated claim OLD-1 exists" "[ \"\$(ca who OLD-1)\" = A ]"
ca gc >/dev/null 2>&1
check "gc: a 20-day-old claim stays without --stale" "[ \"\$(ca who OLD-1)\" = A ]"
ca gc --stale >/dev/null 2>&1
check "gc --stale: the 20-day-old claim is dropped" "[ -z \"\$(ca who OLD-1)\" ]"
check "gc --stale: a fresh claim is not dropped" "[ \"\$(ca who LIVE-7)\" = A ] && [ \"\$(ca who PART-1)\" = A ]"
rm -rf "$T"
[ "$FAILS" -eq 0 ] && echo "claim-selftest: all passed" || { echo "claim-selftest: $FAILS failed"; exit 1; }
