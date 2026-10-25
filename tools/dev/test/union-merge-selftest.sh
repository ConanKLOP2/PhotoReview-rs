#!/usr/bin/env bash
# Selftest for the merge=union entries of .gitattributes. Uses real git in a throwaway temp repo.
set -u
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
GIT="${GIT_BIN:-git}"
tmp=$(mktemp -d); fails=0
ok(){ echo "ok $1"; }; bad(){ echo "FAIL $1"; fails=$((fails+1)); }
: > "$tmp/gitconfig"
export GIT_CONFIG_GLOBAL="$tmp/gitconfig" GIT_CONFIG_NOSYSTEM=1
g(){ "$GIT" -C "$tmp/repo" -c user.name=t -c user.email=t@example.invalid -c core.autocrlf=false -c commit.gpgsign=false "$@"; }

mkrepo(){ # $1 = attributes line ("" for none)
  rm -rf "$tmp/repo"; mkdir -p "$tmp/repo"; "$GIT" -C "$tmp/repo" init -q -b main 2>/dev/null || "$GIT" -C "$tmp/repo" init -q
  g checkout -q -b main 2>/dev/null
  [ -n "$1" ] && printf '%s\n' "$1" > "$tmp/repo/.gitattributes"
  printf '# rows\n| id | state |\n|---|---|\n| X | open |\n' > "$tmp/repo/rows.md"
  g add -A; g commit -q -m base
}
row(){ printf '%s\n' "$2" >> "$tmp/repo/rows.md"; g commit -q -am "$1"; }

# 1. both sides append different rows: union keeps both, no markers, merge succeeds
mkrepo 'rows.md merge=union'
g checkout -q -b a; row a '| A | new |'
g checkout -q main; g checkout -q -b b; row b '| B | new |'
g checkout -q a
if g merge -q --no-edit b >/dev/null 2>&1; then ok 'append+append merges without conflict'; else bad 'append+append merges without conflict'; fi
f="$tmp/repo/rows.md"
grep -qxF '| A | new |' "$f" && grep -qxF '| B | new |' "$f" && ok 'both appended rows kept' || bad 'both appended rows kept'
[ "$(grep -c '| A | new |' "$f")" = 1 ] && [ "$(grep -c '| B | new |' "$f")" = 1 ] && ok 'no duplicated appended row' || bad 'no duplicated appended row'
grep -qE '^(<<<<<<<|=======|>>>>>>>)' "$f" && bad 'no conflict marker lines' || ok 'no conflict marker lines'
[ "$(grep -c '| X | open |' "$f")" = 1 ] && ok 'untouched base row stays single' || bad 'untouched base row stays single'

# 2. control without the attribute: same two appends conflict (proves union is what removes the conflict)
mkrepo ''
g checkout -q -b a; row a '| A | new |'
g checkout -q main; g checkout -q -b b; row b '| B | new |'
g checkout -q a
if g merge -q --no-edit b >/dev/null 2>&1; then bad 'control without union conflicts'; else ok 'control without union conflicts'; fi
g merge --abort >/dev/null 2>&1

# 3. both sides edit the SAME row differently: union never conflicts, keeps both variants (duplicated row: must be reviewed)
mkrepo 'rows.md merge=union'
g checkout -q -b a; sed -i 's/| X | open |/| X | done-a |/' "$tmp/repo/rows.md"; g commit -q -am a
g checkout -q main; g checkout -q -b b; sed -i 's/| X | open |/| X | done-b |/' "$tmp/repo/rows.md"; g commit -q -am b
g checkout -q a
if g merge -q --no-edit b >/dev/null 2>&1; then ok 'same-row edit merges silently (no conflict reported)'; else bad 'same-row edit merges silently (no conflict reported)'; fi
f="$tmp/repo/rows.md"
grep -qxF '| X | done-a |' "$f" && grep -qxF '| X | done-b |' "$f" && ok 'same-row edit keeps both variants (duplicate row to review)' || bad 'same-row edit keeps both variants (duplicate row to review)'
[ "$(grep -c '^| X |' "$f")" = 2 ] && ok 'same-row edit yields exactly two X rows' || bad 'same-row edit yields exactly two X rows'
grep -qE '^(<<<<<<<|=======|>>>>>>>)' "$f" && bad 'same-row edit has no marker lines' || ok 'same-row edit has no marker lines'

# 4. one side appends, other edits an unrelated earlier line: both changes survive
mkrepo 'rows.md merge=union'
g checkout -q -b a; row a '| A | new |'
g checkout -q main; g checkout -q -b b; sed -i 's/^# rows$/# rows v2/' "$tmp/repo/rows.md"; g commit -q -am b
g checkout -q a
g merge -q --no-edit b >/dev/null 2>&1 && grep -qxF '# rows v2' "$tmp/repo/rows.md" && grep -qxF '| A | new |' "$tmp/repo/rows.md" && ok 'append + header edit both kept' || bad 'append + header edit both kept'

# 5. in-place rewrite of the LAST row on one side + append on the other: union silently keeps the STALE old row
mkrepo 'rows.md merge=union'
g checkout -q -b a; sed -i 's/| X | open |/| X | closed |/' "$tmp/repo/rows.md"; g commit -q -am a
g checkout -q main; g checkout -q -b b; row b '| B | new |'
g checkout -q a
g merge -q --no-edit b >/dev/null 2>&1; f="$tmp/repo/rows.md"
grep -qxF '| X | closed |' "$f" && grep -qxF '| X | open |' "$f" && grep -qxF '| B | new |' "$f" && ok 'rewrite + adjacent append keeps stale old row (why in-place files are not union)' || bad 'rewrite + adjacent append keeps stale old row (why in-place files are not union)'

# 6. policy guard on the real .gitattributes: union only for paths in the replay-approved list (empty: replay approved none), never docs/PARITY.md
attr="$ROOT/.gitattributes"
union=$(grep -v '^[[:space:]]*#' "$attr" | grep -E '(^|[[:space:]])merge=union([[:space:]]|$)' | awk '{print $1}')
allowed=''
badn=0
for p in $union; do
  case " $allowed " in *" $p "*) ;; *) echo "  union path not approved by the replay evidence: $p (update tools/dev/docs/union-merge.md and this list together)"; badn=$((badn+1));; esac
  [ -f "$ROOT/$p" ] || { echo "  missing file for union path: $p"; badn=$((badn+1)); }
done
[ "$badn" = 0 ] && ok 'real .gitattributes union paths match the approved list' || bad 'real .gitattributes union paths match the approved list'
case " $union " in *' docs/PARITY.md '*) bad 'docs/PARITY.md is not union-merged';; *) ok 'docs/PARITY.md is not union-merged';; esac
grep -q 'union-merge.md' "$attr" && ok '.gitattributes records the union decision' || bad '.gitattributes records the union decision'
case "$(od -An -c "$attr")" in *'\r'*) bad '.gitattributes has LF endings';; *) ok '.gitattributes has LF endings';; esac

rm -rf "$tmp"
if [ "$fails" = 0 ]; then echo "selftest PASSED"; else echo "selftest $fails FAILED"; exit 1; fi
