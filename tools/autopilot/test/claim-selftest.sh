#!/usr/bin/env bash
# Selftest of claim.sh against a throw-away bare origin (no network, never touches the real origin). Covers review autopilot H1 (a claim
# released by another machine must vanish locally after fetch --prune) and M4 (gc matches the unit name as a fixed string).
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
# gc on the fake origin only (never the real one). Only behaviour that is not an open policy question (P-88, P-89) is checked:
# --fixed-strings (M4) and a fresh claim that no master commit mentions is kept.
git -C "$T/A" -c user.name=t -c user.email=t@e.com commit -q --allow-empty -m "feat(UX1): unrelated unit"
git -C "$T/A" push -q origin HEAD:refs/heads/master
ca claim U.1 >/dev/null; ca claim LIVE-7 >/dev/null
ca gc >/dev/null 2>&1
check "gc: regex look-alike U.1 is not freed by a commit that only says UX1 (--fixed-strings)" "[ \"\$(ca who U.1)\" = A ]"
check "gc: a fresh claim that no master commit mentions is kept" "[ \"\$(ca who LIVE-7)\" = A ]"
rm -rf "$T"
[ "$FAILS" -eq 0 ] && echo "claim-selftest: all passed" || { echo "claim-selftest: $FAILS failed"; exit 1; }
