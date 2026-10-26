#!/usr/bin/env bash
# Validate .claude/agents/*.md: frontmatter (name == file name, description, tools, model)
# and that every repo path written in backticks in the prompts exists.
# Usage: bash tools/dev/agents-check.sh [agents-dir]   (default .claude/agents, relative to the repo root)
# Env: PY_BIN (python, default "python"; falls back to python3).
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
cd "$ROOT" || exit 2
DIR=${1:-.claude/agents}
PY=${PY_BIN:-}
if [ -z "$PY" ]; then
  if command -v python >/dev/null 2>&1; then PY=python; else PY=python3; fi
fi

# Paths are passed relative to the repo root (a native Windows python cannot read /c/... paths).
"$PY" -I - "$DIR" <<'PYEOF'
import os, re, sys

agents_dir = sys.argv[1]
ROOTS = {"contracts", "docs", "crates", "tools", "tests", "assets", ".claude", ".github"}
ROOT_FILES = {"AGENTS.md", "CLAUDE.md", "task_on_progress.md", "README.md", "Cargo.toml"}
KNOWN_TOOLS = {"Read", "Grep", "Glob", "Bash", "Write", "Edit", "WebFetch", "WebSearch"}
failures = 0

def ok(msg):
    print("ok " + msg)

def fail(msg):
    global failures
    failures += 1
    print("FAIL " + msg)

def parse_frontmatter(text):
    # Returns (dict, body) or (None, text). PyYAML when present, else a strict mini parser.
    if not text.startswith("---\n"):
        return None, text
    end = text.find("\n---\n", 4)
    if end < 0:
        return None, text
    block, body = text[4:end], text[end + 5:]
    try:
        import yaml  # type: ignore
        data = yaml.safe_load(block)
        if not isinstance(data, dict):
            return None, body
        return data, body
    except ImportError:
        pass
    data = {}
    for line in block.split("\n"):
        if not line.strip():
            continue
        m = re.match(r"^([A-Za-z_][A-Za-z0-9_-]*):\s*(.*)$", line)
        if not m:
            return None, body
        v = m.group(2).strip()
        if v[:1] in ("'", '"'):
            q = v[0]
            if len(v) < 2 or v[-1] != q:
                return None, body
            v = v[1:-1]
        elif ": " in v or v.startswith(("[", "{", "&", "*", "!", "|", ">", "%", "@", "`")):
            return None, body  # would need quoting in real YAML
        data[m.group(1)] = v
    return data, body

def check_paths(body):
    checked, missing = 0, []
    for span in sorted(set(re.findall(r"`([^`\n]+)`", body))):
        s = re.sub(r":\d+(-\d+)?$", "", span.strip())
        if re.search(r"[<>*{}$%\s]", s) or "::" in s or s.startswith(("C:", "/", "~", "-")):
            continue
        first = s.split("/")[0]
        if "/" in s:
            if first not in ROOTS:
                continue
        elif s not in ROOT_FILES:
            continue
        checked += 1
        if not os.path.exists(s):
            missing.append(s)
    return checked, missing

if not os.path.isdir(agents_dir):
    fail("agents dir missing: " + agents_dir)
    names = []
else:
    names = sorted(n for n in os.listdir(agents_dir) if n.endswith(".md"))
if not names and os.path.isdir(agents_dir):
    fail("no agent files in " + agents_dir)

for fname in names:
    stem = fname[:-3]
    with open(os.path.join(agents_dir, fname), "rb") as fh:
        raw = fh.read()
    if raw.startswith(b"\xef\xbb\xbf") or b"\r\n" in raw:
        fail(stem + ": BOM or CRLF")
        continue
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError:
        fail(stem + ": not UTF-8")
        continue
    data, body = parse_frontmatter(text)
    if data is None:
        fail(stem + ": frontmatter missing or not valid YAML")
        continue
    bad = False
    if data.get("name") != stem:
        fail("%s: name %r does not match the file name" % (stem, data.get("name"))); bad = True
    desc = data.get("description")
    if not isinstance(desc, str) or not desc.strip():
        fail(stem + ": description empty"); bad = True
    tools = data.get("tools")
    if isinstance(tools, str):
        tl = [t.strip() for t in tools.split(",") if t.strip()]
    elif isinstance(tools, list):
        tl = [str(t).strip() for t in tools]
    else:
        tl = []
    if not tl:
        fail(stem + ": tools list missing or empty"); bad = True
    else:
        unknown = [t for t in tl if t not in KNOWN_TOOLS]
        if unknown:
            fail("%s: unknown tools %s" % (stem, unknown)); bad = True
    if not data.get("model"):
        fail(stem + ": model not set"); bad = True
    if len(body.strip()) < 200:
        fail(stem + ": system prompt is empty or too short"); bad = True
    if not bad:
        ok("%s frontmatter (tools: %s; model: %s)" % (stem, ", ".join(tl), data["model"]))
    n, missing = check_paths(body)
    if missing:
        for p in missing:
            fail("%s: path in backticks does not exist: %s" % (stem, p))
    else:
        ok("%s paths (%d repo paths exist)" % (stem, n))

print("agents-check PASSED" if failures == 0 else "agents-check %d FAILED" % failures)
sys.exit(0 if failures == 0 else 1)
PYEOF
