#!/usr/bin/env bash
# Unit claims shared by several machines through git. A claim is a ref refs/claims/<unit> on origin that points at a parentless commit
# (empty tree) whose message names the machine, the time and a note. Creating a ref that already exists with unrelated history is rejected
# by git, so two machines can never both claim the same unit (the push is atomic). See docs/MULTI-MACHINE.md.
# Usage: claim.sh claim <unit> [note]   exit 0 = claimed (or already mine), 1 = claimed by another machine, 2 = cannot reach origin
#        claim.sh release <unit>        delete my claim (refuses another machine's claim unless --force)
#        claim.sh list                  every claim: unit, machine, time, note
#        claim.sh who <unit>            machine name that holds the claim (empty = none)
#        claim.sh gc                    delete claims whose unit already appears on origin/master (merged PR) or that are older than 14 days
# The machine name is $PR_MACHINE (set it once per machine, e.g. A or B), default the host name.
set -u
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
M="${PR_MACHINE:-$(hostname)}"
EMPTY=4b825dc642cb6eb9a060e54bf8d69288fbee4904
g() { git -C "$REPO" "$@"; }
cmd="${1:-}"; u="${2:-}"; note="${3:-}"

fetch_claims() { g fetch -q origin '+refs/claims/*:refs/claims/*' 2>/dev/null; }
holder() { g log -1 --format=%an "refs/claims/$1" 2>/dev/null; }

case "$cmd" in
  claim)
    [ -n "$u" ] || { echo "usage: claim.sh claim <unit> [note]" >&2; exit 2; }
    fetch_claims || { echo "claim: cannot reach origin" >&2; exit 2; }
    if g rev-parse -q --verify "refs/claims/$u" >/dev/null; then
      h="$(holder "$u")"; [ "$h" = "$M" ] && { echo "claim: $u already claimed by $M"; exit 0; }
      echo "claim: $u is claimed by $h"; exit 1
    fi
    c="$(GIT_AUTHOR_NAME="$M" GIT_AUTHOR_EMAIL="claims@local" GIT_COMMITTER_NAME="$M" GIT_COMMITTER_EMAIL="claims@local" \
         git -C "$REPO" commit-tree "$EMPTY" -m "claim $u by $M

machine: $M
time: $(date -u +%Y-%m-%dT%H:%M:%SZ)
note: $note")" || exit 2
    if g push -q origin "$c:refs/claims/$u" 2>/dev/null; then g update-ref "refs/claims/$u" "$c"; echo "claim: $u claimed by $M"; exit 0; fi
    fetch_claims || exit 2
    h="$(holder "$u")"; [ "$h" = "$M" ] && exit 0
    echo "claim: $u was just claimed by ${h:-another machine}"; exit 1 ;;
  release)
    [ -n "$u" ] || { echo "usage: claim.sh release <unit> [--force]" >&2; exit 2; }
    fetch_claims || exit 2
    h="$(holder "$u")"; [ -z "$h" ] && { echo "release: no claim on $u"; exit 0; }
    if [ "$h" != "$M" ] && [ "${3:-}" != "--force" ]; then echo "release: $u belongs to $h (use --force only after asking that machine)"; exit 1; fi
    g push -q origin ":refs/claims/$u" && g update-ref -d "refs/claims/$u" && echo "release: $u released" ;;
  list)
    fetch_claims || echo "(cannot reach origin, showing the local copy)" >&2
    g for-each-ref --format='%(refname:strip=2)' refs/claims/ | while read -r x; do
      printf '%s\t%s\t%s\t%s\n' "$x" "$(g log -1 --format=%an "refs/claims/$x")" "$(g log -1 --format=%cd --date=format:%Y-%m-%dT%H:%M "refs/claims/$x")" "$(g log -1 --format=%b "refs/claims/$x" | sed -n 's/^note: //p')"
    done ;;
  who) fetch_claims; holder "$u" ;;
  gc)
    fetch_claims || exit 2; g fetch -q origin master 2>/dev/null
    now=$(date +%s)
    g for-each-ref --format='%(refname:strip=2)' refs/claims/ | while read -r x; do
      age=$(( now - $(g log -1 --format=%ct "refs/claims/$x") ))
      if [ -n "$(g log origin/master --oneline -1 --grep "$x")" ] || [ "$age" -gt 1209600 ]; then
        g push -q origin ":refs/claims/$x" && g update-ref -d "refs/claims/$x" && echo "gc: $x removed"
      fi
    done ;;
  *) sed -n 2,12p "$0"; exit 2 ;;
esac
