#!/usr/bin/env bash
# Format check without ever passing the whole workspace to one rustfmt command line: a plain `cargo fmt --check` in a deep worktree
# path overflows the Windows command line limit ("The filename or extension is too long. (os error 206)").
# Usage (cwd = workspace root):  fmtcheck.sh --all          every workspace member, one `cargo fmt -p <member> -- --check` each
#                                fmtcheck.sh -p <crate>...  only these members
# If a single crate is still too long for one command, its .rs files are checked with rustfmt in batches of 25.
# Env: FMT_FORCE_BATCH=1 (always use the batch path; used by the selftest). Exit 0 = formatted; non-zero = diff on stdout.
set -u
SELF="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export PYTHONUTF8=1
mapfile -t ALLM < <(python "$SELF/codex-lib.py" members | tr -d '\r')
[ "${#ALLM[@]}" -gt 0 ] || { echo "fmtcheck: cannot read the workspace members (cargo metadata failed)"; exit 9; }
WANT=" "
if [ "${1:-}" = "-p" ]; then shift; for c in "$@"; do WANT="$WANT$c "; done; else WANT=""; fi
rc=0
for l in "${ALLM[@]}"; do
  IFS=$'\t' read -r name ed dir <<< "$l"
  [ -z "$WANT" ] || case "$WANT" in *" $name "*) ;; *) continue;; esac
  out=""; r=0
  if [ "${FMT_FORCE_BATCH:-0}" != 1 ]; then out="$(cargo fmt -p "$name" -- --check 2>&1 </dev/null)"; r=$?; fi
  if [ "${FMT_FORCE_BATCH:-0}" = 1 ] || { [ $r != 0 ] && printf '%s' "$out" | grep -qiE 'os error 206|too long'; }; then
    out=""; r=0
    mapfile -t FILES < <(find "$dir" -name '*.rs' -not -path '*/target/*' | tr -d '\r' | sort)
    i=0
    while [ $i -lt ${#FILES[@]} ]; do
      o="$(rustfmt --edition "$ed" --check "${FILES[@]:$i:25}" 2>&1 </dev/null)" || { r=1; out="$out$o"$'\n'; }
      i=$((i+25))
    done
  fi
  if [ $r != 0 ]; then echo "== fmt: $name"; printf '%s\n' "$out"; rc=1; fi
done
exit $rc
