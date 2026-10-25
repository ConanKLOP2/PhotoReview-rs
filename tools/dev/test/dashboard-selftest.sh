#!/usr/bin/env bash
set -u
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"; tmp=$(mktemp -d); fails=0
trap 'rm -rf "$tmp"' EXIT
ok(){ echo "ok $1"; }; bad(){ echo "FAIL $1"; fails=$((fails+1)); }
REALPY=${REALPY:-python}
mkdir -p "$tmp/bin"
# fake git: "log <ref> --format=%s" prints $FAKE_SUBJECTS; refs named missing-ref fail
cat > "$tmp/bin/git.sh" <<'EOF'
#!/bin/sh
case "$*" in *missing-ref*) exit 1;; *"log "*) cat "$FAKE_SUBJECTS";; esac
EOF
# fake python: logs that the override was used, then runs the real interpreter
cat > "$tmp/bin/py.sh" <<EOF
#!/bin/sh
echo call >> "$tmp/py.calls"
exec $REALPY "\$@"
EOF
cat > "$tmp/bin/parity.py" <<'EOF'
import json; print(json.dumps({"Done": 7, "Todo": 3}))
EOF
chmod +x "$tmp/bin"/*.sh
cat > "$tmp/plan.md" <<'EOF'
| WU | C# | Rust | Col | Model | Test | Depends on | Batch |
|---|---|---|---|---|---|---|---|
| W8-01a | x | y | g | Haiku | G | none | B1 |
| W8-01b | x | y | g | Haiku | G | 01a | B1 |
| W8-01c | x | y | g | Haiku | G | 01a | B1 |
| W8-02a | x | y | g | Haiku | G | 01a | B2 |
| W8-02b | x | y | g | Haiku | G | 01a | B2 (conditional) |
EOF
cat > "$tmp/subjects.txt" <<'EOF'
feat(W8-01a..01b): port viewport (#1)
docs(review): W8-01c static review only (#2)
fix(W8-02a): <script>alert(1)</script> (#3)
chore: W8-02b not counted (#4)
EOF
cat > "$tmp/status.json" <<'EOF'
{"quota":{"ok":true,"five_hour_percent":10,"five_hour_reset":"soon","week_percent":20,"week_reset":"later","status":"OK"},"disk_free_gb":50.5,
"processes":{"cargo":2,"rustc":0},"pipelines":[{"tag":"<b>tag</b>","counts":{"done":1,"failed":1},"failed":[{"unit":"W8-02b","reason":"<img src=x onerror=alert(2)>"}]}],
"claims":{"machine":"A","counts_by_machine":{"A":1},"mine":["u1"]},
"pull_requests":{"open":[{"number":9,"mergeStateStatus":"CLEAN","title":"<script>alert(3)</script> title"}],"last_5_merged":[{"number":8,"title":"merged \"q\" & co"}]},
"worktrees":{"count":3,"dirty_tracked_under_claude_worktrees":1,"with_target":["w1"]}}
EOF
export GIT_BIN="$tmp/bin/git.sh" PY_BIN="$tmp/bin/py.sh" STATUS_JSON="$tmp/status.json" FAKE_SUBJECTS="$tmp/subjects.txt" PLAN="$tmp/plan.md" PARITY_PY="$tmp/none.py"
out="$tmp/o/dash.html"
res=$(bash "$ROOT/tools/dev/dashboard.sh" --out "$out" 2>&1); rc=$?
if [ "$rc" -eq 0 ] && [ -s "$out" ]; then ok 'generates the file (custom --out, parent dir created)'; else bad "generates the file: $res"; fi
[ -s "$tmp/py.calls" ] && ok 'PY_BIN override used' || bad 'PY_BIN override used'
html=$(cat "$out" 2>/dev/null)
case "$html" in *'3 / 5</b> units'*|*'<b>3 / 5</b>'*) ok 'wave count: only qualifying subjects (feat range + fix), docs/chore ignored';; *) bad 'wave count 2/5';; esac
case "$html" in *'<span>2 / 3</span>'*|*'<span>2 / 3</span>'*) ok 'batch B1 bar 2/3';; *) case "$html" in *'2 / 3'*) ok 'batch B1 bar 2/3';; *) bad 'batch B1 bar 2/3';; esac;; esac
case "$html" in *'1 / 2'*) ok 'batch B2 bar 1/2 (conditional suffix normalised)';; *) bad 'batch B2 bar 1/2';; esac
case "$html" in *'<code>W8-01c W8-02b</code>'*) ok 'unmerged list';; *) bad 'unmerged list';; esac
case "$html" in *'&lt;script&gt;alert(3)&lt;/script&gt; title'*) ok 'injection: script in PR title is escaped';; *) bad 'injection: script in PR title is escaped';; esac
case "$html" in *'&lt;img src=x onerror=alert(2)&gt;'*'&lt;b&gt;tag&lt;/b&gt;'*|*'&lt;b&gt;tag&lt;/b&gt;'*'&lt;img src=x onerror=alert(2)&gt;'*) ok 'injection: failed reason and pipeline tag escaped';; *) bad 'injection: failed reason and pipeline tag escaped';; esac
case "$html" in *'alert(1)'*) bad 'script from a commit subject must not appear';; *) ok 'commit subject markup never rendered raw';; esac
n=$(printf '%s' "$html" | grep -o '<script' | wc -l)
[ "$n" -eq 1 ] && ok 'only our own single <script> element' || bad "script element count is $n"
case "$html" in *'merged &quot;q&quot; &amp; co'*) ok 'quotes and ampersands escaped';; *) bad 'quotes and ampersands escaped';; esac
if printf '%s' "$html" | grep -qiE 'https?://|<link|@import|url\(|<[a-z]+[^>]* (src|href)=|localStorage'; then bad 'no external resource / no localStorage'; else ok 'no external resource / no localStorage'; fi
case "$html" in *prefers-color-scheme:dark*'name="viewport"'*|*'name="viewport"'*prefers-color-scheme:dark*) ok 'dark mode and viewport meta';; *) bad 'dark mode and viewport meta';; esac
case "$html" in *PARITY*) bad 'parity skipped silently when absent';; *) ok 'parity skipped silently when absent';; esac
PARITY_PY="$tmp/bin/parity.py" bash "$ROOT/tools/dev/dashboard.sh" --out "$tmp/p.html" >/dev/null 2>&1
grep -q 'PARITY statuses' "$tmp/p.html" && grep -q '<th>Done</th><td>7</td>' "$tmp/p.html" && ok 'parity card when parity.py exists' || bad 'parity card when parity.py exists'
STATUS_JSON="$tmp/missing.json" bash "$ROOT/tools/dev/dashboard.sh" --out "$tmp/n.html" >/dev/null 2>&1
if [ -s "$tmp/n.html" ] && grep -q 'Codex quota' "$tmp/n.html" && grep -q '>n/a<' "$tmp/n.html" && grep -q '<b>3 / 5</b>' "$tmp/n.html"; then ok 'missing status JSON gives n/a cards, wave still shown'; else bad 'missing status JSON gives n/a cards'; fi
res=$(REF=missing-ref bash "$ROOT/tools/dev/dashboard.sh" --out "$tmp/r.html" 2>&1); grep -q 'using HEAD' <<<"$res" && grep -q '<b>3 / 5</b>' "$tmp/r.html" && ok 'missing ref falls back to HEAD' || bad 'missing ref falls back to HEAD'
PLAN="$tmp/nope.md" bash "$ROOT/tools/dev/dashboard.sh" --out "$tmp/q.html" >/dev/null 2>&1; grep -q 'Wave 8 progress' "$tmp/q.html" && grep -q 'n/a' "$tmp/q.html" && ok 'missing plan gives n/a wave card' || bad 'missing plan gives n/a wave card'
bash "$ROOT/tools/dev/dashboard.sh" --bogus >/dev/null 2>&1; [ $? -eq 2 ] && ok 'bad argument exits 2' || bad 'bad argument exits 2'
bash -n "$ROOT/tools/dev/dashboard.sh" && ok 'bash -n clean' || bad 'bash -n clean'
if [ "$fails" -eq 0 ]; then echo 'selftest PASSED'; else echo "selftest $fails FAILED"; fi; exit $((fails>0))
