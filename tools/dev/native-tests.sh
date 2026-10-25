#!/usr/bin/env bash
# native-tests.sh [crate] [--list]: list and run the native (#[ignore = "native...]) integration tests of a crate.
# Never deletes anything. Tests create hidden windows only. Env: CARGO_BIN PS_BIN TIMEOUT_BIN NATIVE_TIMEOUT NATIVE_TESTS_ROOT.
set -u
ROOT="${NATIVE_TESTS_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
CARGO_BIN=${CARGO_BIN:-cargo}; PS_BIN=${PS_BIN:-powershell.exe}; TIMEOUT_BIN=${TIMEOUT_BIN:-timeout}; NATIVE_TIMEOUT=${NATIVE_TIMEOUT:-900}
crate=""; list=0
for x in "$@"; do
  case "$x" in
    --list) list=1 ;;
    -*) echo 'usage: native-tests.sh [crate] [--list]' >&2; exit 2 ;;
    *) if [ -z "$crate" ]; then crate="$x"; else echo 'usage: native-tests.sh [crate] [--list]' >&2; exit 2; fi ;;
  esac
done
crate=${crate:-pr-platform-win}
tdir="$ROOT/crates/$crate/tests"
[ -d "$tdir" ] || { echo "no tests directory: crates/$crate/tests" >&2; exit 2; }

names=(); counts=(); total=0
for f in "$tdir"/*.rs; do
  [ -f "$f" ] || continue
  n=$(grep -cE '#\[ignore *= *"native' "$f" 2>/dev/null); n=${n:-0}
  if [ "$n" -gt 0 ]; then b=$(basename "$f" .rs); names+=("$b"); counts+=("$n"); total=$((total+n)); fi
done

echo "crate: $crate: ${#names[@]} integration test files with native tests ($total native tests)"
for i in "${!names[@]}"; do printf '  %-48s %4s\n' "${names[$i]}" "${counts[$i]}"; done

# interactive input desktop probe: SetCursorPos to the current cursor position
probe="Add-Type -TypeDefinition 'using System;using System.Runtime.InteropServices;public static class PrNative{[StructLayout(LayoutKind.Sequential)]public struct PT{public int X;public int Y;}[DllImport(\"user32.dll\")]public static extern bool GetCursorPos(out PT p);[DllImport(\"user32.dll\")]public static extern bool SetCursorPos(int x,int y);}'; \$p=New-Object PrNative+PT; if([PrNative]::GetCursorPos([ref]\$p) -and [PrNative]::SetCursorPos(\$p.X,\$p.Y)){'true'}else{'false'}"
desk=$("$PS_BIN" -NoProfile -NonInteractive -Command "$probe" 2>/dev/null | tr -d '\r' | tail -n 1)
case "$desk" in
  true) echo "interactive input desktop: yes" ;;
  *) echo "interactive input desktop: NO (probe said '${desk:-no answer}')"
     echo "WARNING: tests that move the real cursor (the two NativeDialog tooltip tests) cannot pass without an interactive input desktop." ;;
esac

[ "$list" -eq 1 ] && exit 0
[ "${#names[@]}" -gt 0 ] || { echo "nothing to run"; exit 0; }

work=$(mktemp -d); rows=(); failed_bins=0; tp=0; tf=0
for b in "${names[@]}"; do
  echo "== running $b"
  log="$work/$b.log"
  "$TIMEOUT_BIN" "$NATIVE_TIMEOUT" "$CARGO_BIN" test -p "$crate" --locked --test "$b" -- --include-ignored --test-threads=1 >"$log" 2>&1
  rc=$?
  p=$(grep -E '^test result:' "$log" | sed -E 's/.* ([0-9]+) passed.*/\1/' | awk '{s+=$1} END{print s+0}')
  f=$(grep -E '^test result:' "$log" | sed -E 's/.* ([0-9]+) failed.*/\1/' | awk '{s+=$1} END{print s+0}')
  if [ "$rc" -eq 0 ]; then st=PASS; elif [ "$rc" -eq 124 ]; then st=TIMEOUT; else st=FAIL; fi
  if ! grep -qE '^test result:' "$log" && [ "$rc" -ne 0 ]; then st="${st}(no result: build error?)"; fi
  [ "$rc" -ne 0 ] && { failed_bins=$((failed_bins+1)); tail -n 15 "$log"; }
  tp=$((tp+p)); tf=$((tf+f))
  rows+=("$(printf '%-48s %6s %6s  %s' "$b" "$p" "$f" "$st")")
  echo "   $b: passed=$p failed=$f $st"
done
echo
printf '%-48s %6s %6s  %s\n' binary passed failed status
for r in "${rows[@]}"; do echo "$r"; done
printf '%-48s %6s %6s  %s binaries failed\n' TOTAL "$tp" "$tf" "$failed_bins"
rm -rf "$work"
[ "$failed_bins" -eq 0 ] || exit 1
exit 0
