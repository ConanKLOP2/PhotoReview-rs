#!/usr/bin/env bash
# Writes ONE self-contained HTML dashboard (no network, no external resource). See tools/dev/docs/dashboard.md.
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
GIT_BIN=${GIT_BIN:-git}; PY_BIN=${PY_BIN:-python}
STATUS_BIN=${STATUS_BIN:-$ROOT/tools/dev/status.sh}
PARITY_PY=${PARITY_PY:-$ROOT/tools/dev/parity.py}
PLAN=${PLAN:-$ROOT/docs/WAVE-8-PLAN.md}
CODEX_LIB=${CODEX_LIB:-$ROOT/tools/autopilot/lib/codex-lib.py}
REF=${REF:-origin/master}
STATE_DIR=${STATE_DIR:-$ROOT/tools/autopilot/state}
out=$ROOT/tools/dev/out/dashboard.html
while [ $# -gt 0 ]; do
  case "$1" in
    --out) [ $# -ge 2 ] || { echo 'usage: dashboard.sh [--out FILE]' >&2; exit 2; }; out=$2; shift 2;;
    *) echo 'usage: dashboard.sh [--out FILE]' >&2; exit 2;;
  esac
done
mkdir -p "$(dirname "$out")" || exit 1
work=$(mktemp -d) || exit 1
trap 'rm -rf "$work"' EXIT
# status JSON: STATUS_JSON file (tests) or the real tools/dev/status.sh --json
if [ -n "${STATUS_JSON:-}" ]; then
  cp "$STATUS_JSON" "$work/status.json" 2>/dev/null || echo 'null' > "$work/status.json"
else
  PY_BIN="$PY_BIN" GIT_BIN="$GIT_BIN" bash "$STATUS_BIN" --json > "$work/status.json" 2>/dev/null || echo 'null' > "$work/status.json"
fi
# commits of the reference branch, subject then body, NUL-terminated records (fall back to HEAD when the ref is missing)
if ! "$GIT_BIN" -C "$ROOT" log "$REF" --format='%s%n%b%x00' > "$work/subjects.txt" 2>/dev/null; then
  "$GIT_BIN" -C "$ROOT" log HEAD --format='%s%n%b%x00' > "$work/subjects.txt" 2>/dev/null || : > "$work/subjects.txt"
  echo "REF $REF unavailable, using HEAD" >&2
fi
# optional parity statuses (interface: parity.py --json prints {"Done": n, "Todo": n, ...}); skipped silently when absent
if [ -f "$PARITY_PY" ]; then
  "$PY_BIN" -I "$PARITY_PY" --json > "$work/parity.json" 2>/dev/null || rm -f "$work/parity.json"
fi
"$PY_BIN" -I - "$out" "$work" "$PLAN" "$CODEX_LIB" "$REF" "$STATE_DIR" <<'PY'
import sys, os, re, json, html, runpy, datetime, glob
out, work, plan, lib, ref, state_dir = sys.argv[1:7]
E = lambda x: html.escape(str(x), quote=True)
def load(p, d=None):
    try:
        with open(p, encoding='utf-8') as f: return json.load(f)
    except Exception: return d
status = load(os.path.join(work, 'status.json'))
if not isinstance(status, dict): status = {}
try:
    with open(os.path.join(work, 'subjects.txt'), encoding='utf-8', errors='replace') as f: commits = [c.strip(chr(10)).split(chr(10)) for c in f.read().split(chr(0)) if c.strip()]
except Exception: commits = []
# wave progress: plan rows (W8 only) vs unit ids found in qualifying commit subjects
rows, expand, perr = [], None, ''
try:
    m = runpy.run_path(lib); expand = m['expand_unit_ids']; rows = [r for r in m['plan_rows'](plan) if r['id'].startswith('W8-')]
except Exception as e: perr = 'plan/lib unreadable: %s' % e
units = {}
for r in rows:
    mb = re.match(r'B\d+', r['batch'].strip()); units[r['id']] = mb.group(0) if mb else '?'
# A line counts when it is a code line: feat/port/fix/test/refactor/perf (optional scope), merge(...), "Merge W8-...",
# also as a "* " bullet inside a squash body. docs/chore/ci/style commits are skipped entirely; "red tests" lines
# (test-first commits of the pipeline) do not count because the unit is not ported by them.
cnt = re.compile(r'^(?:[*-]\s+)?(?:(?:feat|port|fix|test|refactor|perf|merge)(?:\([^)]*\))?!?:?|Merge W8-)', re.I)
skip = re.compile(r'^(?:docs|chore|ci|style)(?:\([^)]*\))?!?:', re.I)
# ids of one line: explicit tokens, lists after a W8-<id> ("W8-11a, 11b, 11c", "W8-13g,13h", "W8-18l..18n", "15a/b/c" is left to expand)
# and bare ids in parentheses after a batch token ("feat(W8-B5): ... (18e, 18f, 19b)").
ITEM = r'(?:W8-)?\d\d[a-z](?:\.\.(?:\d\d)?[a-z])?'
LIST = re.compile(r'W8-\d\d[a-z](?:\.\.(?:\d\d)?[a-z])?(?:\s*(?:,|/|&|and)\s*' + ITEM + ')*')
BARE = re.compile(r'(?<![\w-])(\d\d)([a-z])(?:\.\.(?:(\d\d))?([a-z]))?(?![\w-])')
def line_ids(line):
    ids = []
    def norm(m):
        out, wave = [], 'W8'
        for it in re.split(r'\s*(?:,|/|&|and)\s*', m.group(0)):
            mm = re.fullmatch(r'(?:W8-)?(\d\d)([a-z])(?:\.\.(?:\d\d)?([a-z]))?', it)
            if mm:
                d = mm.group(1); out.append('W8-%s%s' % (d, mm.group(2)) + ('..' + mm.group(3) if mm.group(3) else ''))
        return ' '.join(out)
    text = LIST.sub(norm, line)
    ids += expand(text)
    if re.search(r'W8-B\d', line):
        for grp in re.findall(r'\(([^()]*)\)', line):
            for m in BARE.finditer(grp):
                ids += expand('W8-%s%s%s' % (m.group(1), m.group(2), ('..' + m.group(4)) if m.group(4) else ''))
    return ids
merged, seen = set(), set()
if expand:
    for c in commits:
        if skip.match(c[0]): continue
        for ln in c:
            for u in line_ids(ln):
                if u in units: seen.add(u)
            if cnt.match(ln.strip()) and 'red tests' not in ln:
                for u in line_ids(ln):
                    if u in units: merged.add(u)
    # pipeline state files (machine-local, optional): done/partial counts only when the unit is also named by a non-docs commit
    for sf in glob.glob(os.path.join(state_dir, '*.json')):
        st = load(sf, {})
        for u, v in ((st.get('units') or {}) if isinstance(st, dict) else {}).items():
            if u in units and isinstance(v, dict) and v.get('status') in ('done', 'partial') and u in seen: merged.add(u)
batches = sorted(set(units.values()))
CSS = ''':root{--bg:#f6f7f9;--fg:#1b1f24;--card:#fff;--line:#d8dde3;--mut:#5b6570;--ok:#1a7f37;--bad:#c62828;--warn:#b26a00;--bar:#2f6fdb}
@media (prefers-color-scheme:dark){:root{--bg:#14171b;--fg:#e6e9ed;--card:#1d2127;--line:#323943;--mut:#9aa5b1;--ok:#4cc16b;--bad:#ff6b6b;--warn:#f0b24a;--bar:#6aa0ff}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.45 system-ui,Segoe UI,sans-serif;padding:16px}
main{max-width:980px;margin:0 auto}h1{font-size:1.4rem;margin:0 0 4px}h2{font-size:1.05rem;margin:0 0 8px}
.card{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:12px 14px;margin:12px 0;overflow-wrap:anywhere}
.mut{color:var(--mut)}.ok{color:var(--ok);font-weight:600}.bad{color:var(--bad);font-weight:600}.warn{color:var(--warn);font-weight:600}
.bar{background:var(--line);border-radius:6px;height:12px;overflow:hidden}.bar>i{display:block;height:100%;background:var(--bar)}
.row{display:grid;grid-template-columns:3rem 1fr 6rem;gap:8px;align-items:center;margin:6px 0}
table{border-collapse:collapse;width:100%}td,th{text-align:left;padding:3px 6px;border-bottom:1px solid var(--line);vertical-align:top}
ul{margin:4px 0;padding-left:20px}code{font-family:ui-monospace,Consolas,monospace;font-size:.9em}
@media (max-width:520px){.row{grid-template-columns:2.5rem 1fr 5rem}}'''
def kv(items): return '<table>' + ''.join('<tr><th>%s</th><td>%s</td></tr>' % (E(k), v) for k, v in items) + '</table>'
def card(title, body): return '<section class="card"><h2>%s</h2>%s</section>' % (E(title), body)
parts = []
q = status.get('quota', 'n/a')
if isinstance(q, dict):
    cls = 'bad' if q.get('status') == 'STOP' else 'ok'
    parts.append(card('Codex quota', kv([('Status', '<span class="%s">%s</span>' % (cls, E(q.get('status')))), ('5 hour', '%s%% (reset %s)' % (E(q.get('five_hour_percent')), E(q.get('five_hour_reset')))), ('Week', '%s%% (reset %s)' % (E(q.get('week_percent')), E(q.get('week_reset'))))])))
else: parts.append(card('Codex quota', '<span class="mut">n/a</span>'))
d = status.get('disk_free_gb'); low = isinstance(d, (int, float)) and d < 20
parts.append(card('Disk', '<span class="%s">%s</span>' % ('bad' if low else 'ok', E('%s GB free%s' % (d, ' (LOW)' if low else '')) if d is not None else 'n/a')))
pr = status.get('processes')
parts.append(card('Processes', kv([(k, E(v)) for k, v in pr.items()]) if isinstance(pr, dict) else '<span class="mut">n/a</span>'))
pl = status.get('pipelines')
if isinstance(pl, list) and pl:
    b = ''
    for p in pl:
        b += '<p><b>%s</b>: %s</p>' % (E(p.get('tag')), E(', '.join('%s=%s' % kvp for kvp in (p.get('counts') or {}).items())))
        if p.get('failed'): b += '<ul>' + ''.join('<li class="bad">%s: <span class="mut">%s</span></li>' % (E(x.get('unit')), E(x.get('reason'))) for x in p['failed']) + '</ul>'
    parts.append(card('Pipelines', b))
else: parts.append(card('Pipelines', '<span class="mut">none</span>'))
cl = status.get('claims')
if isinstance(cl, dict):
    parts.append(card('Claims', kv([('By machine', E(', '.join('%s=%s' % x for x in (cl.get('counts_by_machine') or {}).items()) or 'none')), ('Held by %s' % cl.get('machine'), E(', '.join(cl.get('mine') or []) or 'none'))])))
else: parts.append(card('Claims', '<span class="mut">n/a</span>'))
prs = status.get('pull_requests')
if isinstance(prs, dict):
    o = ''.join('<li>#%s <span class="mut">[%s]</span> %s</li>' % (E(x.get('number')), E(x.get('mergeStateStatus')), E(x.get('title'))) for x in prs.get('open', [])) or '<li class="mut">none</li>'
    mg = ''.join('<li>#%s %s</li>' % (E(x.get('number')), E(x.get('title'))) for x in prs.get('last_5_merged', [])) or '<li class="mut">none</li>'
    parts.append(card('Pull requests', '<b>Open</b><ul>%s</ul><b>Last merged</b><ul>%s</ul>' % (o, mg)))
else: parts.append(card('Pull requests', '<span class="mut">n/a</span>'))
wt = status.get('worktrees')
if isinstance(wt, dict):
    parts.append(card('Worktrees', kv([('Count', E(wt.get('count'))), ('Dirty (tracked, under .claude/worktrees)', E(wt.get('dirty_tracked_under_claude_worktrees'))), ('With target dir', E(', '.join(wt.get('with_target') or []) or 'none'))])))
else: parts.append(card('Worktrees', '<span class="mut">n/a</span>'))
if units:
    tot = len(units); dn = len(merged); b = '<p><b>%d / %d</b> units merged (%d%%) on <code>%s</code></p>' % (dn, tot, round(100 * dn / tot), E(ref))
    for bt in batches:
        t = [u for u, x in units.items() if x == bt]; k = sum(u in merged for u in t)
        b += '<div class="row"><b>%s</b><div class="bar"><i style="width:%d%%"></i></div><span>%d / %d</span></div>' % (E(bt), round(100 * k / len(t)), k, len(t))
    rest = [u for u in units if u not in merged]
    b += '<details><summary>Not yet merged (%d)</summary><p><code>%s</code></p></details>' % (len(rest), E(' '.join(rest) or 'none'))
    parts.append(card('Wave 8 progress', b))
else: parts.append(card('Wave 8 progress', '<span class="warn">n/a %s</span>' % E(perr)))
par = load(os.path.join(work, 'parity.json'))
if isinstance(par, dict) and par:
    parts.append(card('PARITY statuses', kv([(k, E(v)) for k, v in par.items()])))
now = datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
doc = '''<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="color-scheme" content="light dark"><title>PhotoReview-rs dashboard</title><style>%s</style></head><body><main><h1>PhotoReview-rs dashboard</h1><p class="mut">Generated <time id="t" datetime="%s">%s</time></p>%s</main><script>(function(){var t=document.getElementById("t");try{t.textContent=new Date(t.getAttribute("datetime")).toLocaleString()}catch(e){}})();</script></body></html>
''' % (CSS, now, now, ''.join(parts))
with open(out, 'w', encoding='utf-8', newline='\n') as f: f.write(doc)
print('dashboard: %s (%d bytes); wave units merged %d/%d' % (out, len(doc.encode('utf-8')), len(merged), len(units)))
PY
