#!/usr/bin/env python3
"""Markdown link checker for the repo docs.

Checks, for every markdown file in scope:
  1. relative link targets exist,
  2. GitHub anchors (#slug) exist in the target markdown file,
  3. backticked repo paths (crates/, tools/, docs/, contracts/, tests/, assets/, .github/) exist.

Backticked paths that are clearly proposed (the line says "new", or the path sits under an
"Owned files" heading or line) and do not exist are reported separately as PROPOSED, not errors.

Usage: python -I tools/dev/doc-links.py [--root DIR] [--all] [--quiet]
Exit code 1 when there is at least one error.
"""
import argparse
import fnmatch
import os
import re
import sys
import unicodedata
from urllib.parse import unquote

SKIP_DIRS = {".git", "target", ".claude", "node_modules"}
SCOPE_PREFIXES = ("docs/", "contracts/", "tools/")
SCOPE_FILES = {"README.md", "AGENTS.md"}
PATH_PREFIXES = ("crates/", "tools/", "docs/", "contracts/", "tests/", "assets/", ".github/")

LINK_RE = re.compile(r"(?<!\\)\[(?:[^\]\[]|\[[^\]]*\])*\]\(\s*(<[^>]*>|[^)\s]*)(?:\s+\"[^\"]*\")?\s*\)")
REFDEF_RE = re.compile(r"^\s{0,3}\[[^\]]+\]:\s*(\S+)")
TICK_RE = re.compile(r"`([^`\n]+)`")
HEADING_RE = re.compile(r"^(#{1,6})\s+(.*?)\s*#*\s*$")
NEW_RE = re.compile(
    r"\b(new|future|planned|creates?|will|yet|gitignored|removed|branch|target|missing|not (?:written|exist\w*|created|ported)|"
    r"(?:does|do|did|is|are)(?:n\'t| not) exist|none exists|neither exists|no longer)\b|"
    r"mới|không tồn tại|không có|chưa|sẽ", re.I)
CSHARP_RE = re.compile(r"^(tests|tools)/PhotoReview\.|.*\.(cs|csproj|sln)$")
CSHARP_DOC_RE = re.compile(r"^(docs/(architecture\.md|APP-MECHANISMS|perf\.md|refactoring/|adr/00\d\d)|tests/Fixtures/)")
RUNTIME_RE = re.compile(r"^(tools/autopilot/state|tools/dev/out|docs/trial)(/|$)")
PLACEHOLDER_RE = re.compile(r"\.\.|WU-|YYYY|\*\*?$")


def walk_md(root, scope_all):
    out = []
    for d, dirs, files in os.walk(root):
        dirs[:] = [x for x in dirs if x not in SKIP_DIRS]
        for f in files:
            if not f.lower().endswith(".md"):
                continue
            rel = os.path.relpath(os.path.join(d, f), root).replace(os.sep, "/")
            if scope_all or rel in SCOPE_FILES or rel.startswith(SCOPE_PREFIXES):
                out.append(rel)
    return sorted(out)


def all_files(root):
    out = set()
    for d, dirs, files in os.walk(root):
        dirs[:] = [x for x in dirs if x not in SKIP_DIRS]
        for f in files:
            out.add(os.path.relpath(os.path.join(d, f), root).replace(os.sep, "/"))
    # .github is tracked and may be skipped by nothing above; .claude is intentionally not indexed.
    return out


def read(root, rel):
    with open(os.path.join(root, rel), encoding="utf-8", errors="replace") as fh:
        return fh.read().replace("\r\n", "\n").split("\n")


def with_fences(lines):
    """Yield (lineno, text, in_fence)."""
    fence = None
    for i, ln in enumerate(lines, 1):
        m = re.match(r"^\s{0,3}(`{3,}|~{3,})", ln)
        if m:
            ch = m.group(1)[0]
            if fence is None:
                fence = ch
                yield i, ln, True
                continue
            if ch == fence:
                fence = None
                yield i, ln, True
                continue
        yield i, ln, fence is not None


def slugify(text):
    text = re.sub(r"!?\[([^\]]*)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"<[^>]+>", "", text)
    text = text.replace("`", "").replace("*", "")
    text = unicodedata.normalize("NFC", text).lower()
    out = []
    for ch in text:
        if ch.isalnum() or ch in "-_":
            out.append(ch)
        elif ch == " ":
            out.append("-")
        elif unicodedata.category(ch).startswith("M"):
            out.append(ch)
    return "".join(out)


_anchor_cache = {}


def anchors_of(root, rel):
    key = (root, rel)
    if key in _anchor_cache:
        return _anchor_cache[key]
    seen, res = {}, set()
    for _, ln, fenced in with_fences(read(root, rel)):
        if fenced:
            continue
        m = HEADING_RE.match(ln)
        if m:
            s = slugify(m.group(2))
            n = seen.get(s, 0)
            seen[s] = n + 1
            res.add(s if n == 0 else f"{s}-{n}")
        for a in re.findall(r"<a\s+(?:name|id)=\"([^\"]+)\"", ln):
            res.add(a)
    _anchor_cache[key] = res
    return res


def check_link(root, src, lineno, target, files, dirs, errors):
    target = target.strip()
    if target.startswith("<") and target.endswith(">"):
        target = target[1:-1]
    if not target or re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", target) or target.startswith("//"):
        return
    path, _, frag = target.partition("#")
    path = unquote(path.split("?", 1)[0])
    if path:
        if path.startswith("/"):
            full = os.path.normpath(path.lstrip("/")).replace(os.sep, "/")
        else:
            full = os.path.normpath(os.path.join(os.path.dirname(src), path)).replace(os.sep, "/")
        if full.startswith("..") or not (full in files or full in dirs):
            errors.append((src, lineno, "link", target, "target does not exist"))
            return
        dest = full
    else:
        dest = src
    if frag and dest.lower().endswith(".md") and dest in files:
        frag = unquote(frag)
        if re.match(r"^L\d+(-L\d+)?$", frag):
            return
        if frag.lower() not in {a.lower() for a in anchors_of(root, dest)}:
            errors.append((src, lineno, "anchor", target, "anchor not found in " + dest))


def clean_tick(tok):
    tok = tok.strip()
    tok = re.sub(r"#.*$", "", tok)
    tok = re.sub(r"::.*$", "", tok)
    tok = re.sub(r":\d+(?:[,-]\d+)*(?::\d+)?$", "", tok)
    return tok.rstrip(".,;")


def classify_missing(src, q, files):
    """Return the category of a missing path: csharp, runtime, placeholder, proposed or None (real error)."""
    if q.startswith("tools/oracle-cs/") is False and CSHARP_RE.match(q):
        return "csharp"
    if CSHARP_DOC_RE.match(q):
        return "csharp"
    if RUNTIME_RE.match(q):
        return "runtime"
    if PLACEHOLDER_RE.search(q):
        return "placeholder"
    if re.match(r"^contracts/(drafts|W\d+-\w+/)", q):
        return "proposed"  # contract-time draft dirs, removed after the unit merged
    if q.startswith("docs/adr/") and any(f.startswith(q) for f in files):
        return "ok"  # id prefix such as docs/adr/R0002
    if q.startswith("tests/") and any(f.startswith("crates/") and (f.endswith("/" + q) or ("/" + q + "/") in f)
                                       for f in files):
        return "ok"  # crate-relative tests path
    if q.startswith("tests/") and q.endswith((".rs", "/")) and not q.startswith(("tests/fixtures", "tests/golden")):
        return "proposed"  # crate-relative test file path that is not in the tree yet
    if src.startswith("contracts/W"):
        return "proposed"  # unit contract naming a file its unit has not created (or has since renamed)
    return None


def check_ticks(src, lineno, line, files, dirs, proposed_ctx, res):
    for m in TICK_RE.finditer(line):
        raw = m.group(1)
        if not raw.startswith(PATH_PREFIXES) or re.search(r"\s", raw.strip()):
            continue
        p = clean_tick(raw)
        if not p.startswith(PATH_PREFIXES):
            continue
        if any(c in p for c in "<>$|{"):
            res["skipped"].append((src, lineno, "path", p, "placeholder"))
            continue
        if any(c in p for c in "*?[") and "..." not in p:
            pat = p.replace("**", "*").rstrip("/")
            if any(fnmatch.fnmatch(f, pat) for f in files) or any(fnmatch.fnmatch(d, pat) for d in dirs):
                continue
            cat = classify_missing(src, p, files) or ("proposed" if proposed_ctx else None)
            res[cat or "errors"].append((src, lineno, "path", p, "glob matches nothing"))
            continue
        q = p.rstrip("/")
        if q in files or q in dirs:
            continue
        cat = classify_missing(src, q, files)
        if cat == "ok":
            continue
        if cat is None and proposed_ctx:
            cat = "proposed"
        res[cat or "errors"].append((src, lineno, "path", p, "path does not exist" if cat is None else cat))


def run(root, scope_all=False):
    files = all_files(root)
    gh = os.path.join(root, ".github")
    if os.path.isdir(gh):
        for d, _, fs in os.walk(gh):
            for f in fs:
                files.add(os.path.relpath(os.path.join(d, f), root).replace(os.sep, "/"))
    dirs = set()
    for f in files:
        d = os.path.dirname(f)
        while d:
            dirs.add(d)
            d = os.path.dirname(d)
    res = {k: [] for k in ("errors", "proposed", "skipped", "csharp", "runtime", "placeholder", "archive")}
    errors = res["errors"]
    mds = walk_md(root, scope_all)
    for src in mds:
        in_owned = False
        for i, ln, fenced in with_fences(read(root, src)):
            if fenced:
                continue
            h = HEADING_RE.match(ln)
            if h:
                in_owned = "owned files" in h.group(2).lower()
            elif re.match(r"^\s*(\*\*)?owned files", ln, re.I):
                in_owned = True
            low = ln.lower()
            proposed_ctx = in_owned or bool(NEW_RE.search(ln)) or "owned files" in low
            for m in LINK_RE.finditer(ln):
                check_link(root, src, i, m.group(1), files, dirs, errors)
            d = REFDEF_RE.match(ln)
            if d:
                check_link(root, src, i, d.group(1), files, dirs, errors)
            check_ticks(src, i, ln, files, dirs, proposed_ctx, res)
    # docs/archive/ holds material moved verbatim from another location: its relative links and
    # paths describe the old place, so they are counted but never edited and never fail the run.
    keep = [e for e in res["errors"] if not e[0].startswith("docs/archive/")]
    res["archive"] = [e for e in res["errors"] if e[0].startswith("docs/archive/")]
    res["errors"][:] = keep
    return mds, res


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
    ap.add_argument("--all", action="store_true", help="check every markdown file, not only the default scope")
    ap.add_argument("--quiet", action="store_true", help="summary only")
    a = ap.parse_args(argv)
    root = os.path.abspath(a.root)
    mds, res = run(root, a.all)
    errors = res["errors"]
    if not a.quiet:
        for tag, key in (("ERROR", "errors"), ("PROPOSED", "proposed")):
            for src, ln, kind, tgt, why in sorted(res[key]):
                print(f"{tag} {src}:{ln}: {kind} `{tgt}`: {why}")
    other = res["csharp"] + res["runtime"] + res["placeholder"] + res["skipped"]
    print(f"doc-links: files={len(mds)} errors={len(errors)} proposed={len(res['proposed'])} "
          f"archive_unchecked={len(res['archive'])} csharp_refs={len(res['csharp'])} runtime_paths={len(res['runtime'])} "
          f"placeholders={len(res['placeholder']) + len(res['skipped'])}")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
