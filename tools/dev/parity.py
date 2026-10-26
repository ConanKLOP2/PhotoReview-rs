#!/usr/bin/env python
"""Read docs/PARITY.md as data: stats, unit evidence, reconcile proposals, single-row status edit.

Run with `python -I tools/dev/parity.py <cmd>` (wrapper: tools/dev/parity.sh). Only `set --apply` writes.
Env: PARITY_DOC (file), GIT_BIN, PARITY_REF (default origin/master), CODEX_LIB (path of codex-lib.py).
"""
import os
import pathlib
import re
import runpy
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
DOC = pathlib.Path(os.environ.get("PARITY_DOC") or ROOT / "docs" / "PARITY.md")
GIT = os.environ.get("GIT_BIN", "git")
REF = os.environ.get("PARITY_REF", "origin/master")
VOCAB = ["Done", "Ported", "Superseded", "Partial", "Todo", "Different", "Contract", "Doing", "Implementation", "Retired", "Context"]
OPEN = {"Todo", "Contract", "Partial", "Doing"}
LOWER = {v.lower(): v for v in VOCAB}


def split_cells(line):
    """Spans (start, end) of the cell contents of a markdown row; pipes inside backticks do not split."""
    pipes, tick, i = [], False, 0
    while i < len(line):
        ch = line[i]
        if ch == "\\" and i + 1 < len(line):
            i += 2
            continue
        if ch == "`":
            tick = not tick
        elif ch == "|" and not tick:
            pipes.append(i)
        i += 1
    if not pipes or line[:pipes[0]].strip():
        return []
    spans = [(a + 1, b) for a, b in zip(pipes, pipes[1:])]
    tail = line[pipes[-1] + 1:]
    if tail.strip():
        spans.append((pipes[-1] + 1, len(line)))
    return spans


def status_word(cell):
    m = re.match(r"[\s`*_]*([A-Za-z]+)", cell)
    word = LOWER.get(m.group(1).lower()) if m else None
    return word


def parse(text):
    """Rows of the status tables: dicts with line (1-based), id, cell, status (None = unrecognised), text, spans."""
    lines = text.split("\n")
    rows, other = [], 0
    kind, col = None, 1  # kind: None (outside), "status", "plain"
    prev_pipe = False
    for n, line in enumerate(lines, 1):
        spans = split_cells(line)
        if not spans:
            if prev_pipe:
                kind = None  # a table ends at the first non-table line; later headerless rows reuse the default layout
            prev_pipe = False
            continue
        cells = [line[a:b].strip() for a, b in spans]
        sep = all(re.fullmatch(r":?-{3,}:?", c) for c in cells if c) and any(cells)
        if sep:
            prev_pipe = True
            continue
        if not prev_pipe:  # first row of a block
            hdr = [i for i, c in enumerate(cells) if c.lower().startswith("status")]
            nxt = split_cells(lines[n]) if n < len(lines) else []
            nxt_cells = [lines[n][a:b].strip() for a, b in nxt]
            is_hdr = bool(nxt_cells) and all(re.fullmatch(r":?-{3,}:?", c) for c in nxt_cells if c) and any(nxt_cells)
            if is_hdr:
                kind, col = ("status", hdr[0]) if hdr else ("plain", 1)
                prev_pipe = True
                continue
            kind, col = "status", 1  # headerless continuation of the main table
        prev_pipe = True
        if kind == "plain":
            other += 1
            continue
        if len(cells) <= col:
            other += 1
            continue
        rows.append({"line": n, "id": cells[0], "cell": cells[col], "status": status_word(cells[col]),
                     "text": line, "spans": spans, "col": col})
    return rows, other


def load():
    return DOC.read_bytes().decode("utf-8")


def expander():
    path = os.environ.get("CODEX_LIB") or str(ROOT / "tools" / "autopilot" / "lib" / "codex-lib.py")
    try:
        fn = runpy.run_path(path)["expand_unit_ids"]
        fn("W8-18l..18n")
        return fn
    except Exception:
        def fallback(text):
            out = []
            for m in re.finditer(r"\bW\d+-\d\d[a-z](?:\.\.(?:W\d+-)?(?:\d\d)?[a-z])?(?:/\d\d[a-z])*", text):
                tok = m.group(0)
                if ".." in tok:
                    a, b = tok.split("..", 1)
                    wave, digits, start = re.fullmatch(r"(W\d+)-(\d\d)([a-z])", a).groups()
                    end = b[-1]
                    out.extend(f"{wave}-{digits}{chr(c)}" for c in range(ord(start), ord(end) + 1)) if start <= end else out.append(a)
                else:
                    parts = tok.split("/")
                    wave = parts[0].split("-")[0]
                    out.append(parts[0])
                    out.extend(f"{wave}-{p}" for p in parts[1:])
            return list(dict.fromkeys(out))
        return fallback


def git_subjects():
    argv = ["bash", GIT] if GIT.endswith(".sh") else [GIT]
    p = subprocess.run(argv + ["-C", str(ROOT), "log", REF, "--format=%h%x09%s"], text=True, encoding="utf-8",
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if p.returncode != 0:
        return None, p.stderr.strip()
    return [tuple(x.split("\t", 1)) for x in p.stdout.splitlines() if "\t" in x], ""


def short(s, n=110):
    s = " ".join(s.split())
    return s if len(s) <= n else s[: n - 3] + "..."


def cmd_stats():
    rows, other = parse(load())
    counts = {}
    unknown = []
    for r in rows:
        if r["status"]:
            counts[r["status"]] = counts.get(r["status"], 0) + 1
        else:
            unknown.append(r)
    total = len(rows)
    print(f"status rows: {total}   (rows in tables without a Status column, not counted: {other})")
    for v in sorted(counts, key=lambda k: (-counts[k], k)):
        print(f"{v:<16}{counts[v]:>5}  {100.0 * counts[v] / total:5.1f}%")
    print(f"{'unrecognized':<16}{len(unknown):>5}  {100.0 * len(unknown) / max(total, 1):5.1f}%")
    for r in unknown[:15]:
        print(f"  line {r['line']}: {short(r['id'], 50)} | {short(r['cell'], 50)}")
    if len(unknown) > 15:
        print(f"  ... {len(unknown) - 15} more")


def cmd_unit(unit):
    expand = expander()
    wanted = expand(unit) or [unit]
    rows, _ = parse(load())
    hit = [r for r in rows if set(wanted) & set(expand(r["text"])) or any(w in r["text"] for w in wanted if w == unit)]
    print(f"unit {' '.join(wanted)}: {len(hit)} PARITY row(s)")
    for r in hit:
        print(f"  line {r['line']}  [{r['status'] or '?'}]  {short(r['id'], 60)} :: {short(r['cell'], 80)}")
    subj, err = git_subjects()
    if subj is None:
        print(f"git log {REF}: n/a ({err})")
        return
    commits = [(h, s) for h, s in subj if set(wanted) & set(expand(s))]
    print(f"git log {REF}: {len(commits)} commit(s)")
    for h, s in commits:
        print(f"  {h} {short(s, 140)}")


def cmd_reconcile():
    expand = expander()
    subj, err = git_subjects()
    if subj is None:
        print(f"cannot read git log {REF}: {err}", file=sys.stderr)
        return 1
    merged = {}
    for h, s in subj:
        for u in expand(s):
            merged.setdefault(u, (h, s))
    rows, _ = parse(load())
    n = 0
    for r in rows:
        if r["status"] not in OPEN:
            continue
        units = expand(r["text"])
        if not units or not all(u in merged for u in units):
            continue
        n += 1
        ev = ", ".join(f"{u}={merged[u][0]}" for u in units)
        print(f"line {r['line']}  {short(r['id'], 60)}  [{r['status']}]  all cited units merged: {ev}")
        print("    propose: re-check against the proving test, then `parity.sh set <ROW-ID> <status> <evidence>` (docs commits count as merged, verify)")
    print(f"{n} row(s) to review (no file was changed)")
    return 0


def find_row(rows, key):
    def keys(r):
        cell = re.sub(r"\([^)]*\)", " ", r["id"])
        ks = {r["id"], r["id"].split()[0] if r["id"].split() else ""}
        ks.update(t.strip() for t in re.split(r"[,/\s]+", cell) if t.strip())
        return ks
    return [r for r in rows if key in keys(r)]


def cmd_set(args):
    apply = "--apply" in args
    args = [a for a in args if a != "--apply"]
    if len(args) < 3:
        print("usage: parity.py set <ROW-ID> <status> <evidence> [--apply]", file=sys.stderr)
        return 2
    key, status, evidence = args[0], args[1], " ".join(args[2:])
    status = LOWER.get(status.lower())
    if not status:
        print(f"unknown status {args[1]!r}; use one of: {', '.join(VOCAB)}", file=sys.stderr)
        return 2
    if "|" in evidence or "\n" in evidence:
        print("evidence must not contain '|' or a newline", file=sys.stderr)
        return 2
    text = load()
    rows, _ = parse(text)
    hit = find_row(rows, key)
    if len(hit) != 1:
        print(f"row {key!r}: {len(hit)} matches" + ("" if not hit else " (lines " + ", ".join(str(r['line']) for r in hit) + ")"), file=sys.stderr)
        return 1
    r = hit[0]
    a, b = r["spans"][r["col"]]
    old = r["text"][a:b]
    m = re.match(r"(\s*[`*_]*)([A-Za-z]+)(.*?)(\s*)$", old, re.S)
    rest = m.group(3) if m and m.group(2).lower() in LOWER else (" " + old.strip() if old.strip() else "")
    lead = m.group(1) if m and m.group(2).lower() in LOWER else " "
    new = f"{lead}{status}{rest}; {evidence} "
    line = r["text"][:a] + new + r["text"][b:]
    print(f"line {r['line']}  {r['id']}")
    print(f"  - status cell:{old}")
    print(f"  + status cell:{new}")
    if not apply:
        print("dry-run: nothing written (add --apply)")
        return 0
    lines = text.split("\n")
    lines[r["line"] - 1] = line
    DOC.write_bytes("\n".join(lines).encode("utf-8"))
    print("written")
    return 0


def main(argv):
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
        sys.stderr.reconfigure(encoding="utf-8")
    if not argv or argv[0] not in ("stats", "unit", "reconcile", "set"):
        print("usage: parity.sh stats | unit <W8-18o> | reconcile | set <ROW-ID> <status> <evidence> [--apply]", file=sys.stderr)
        return 2
    c = argv[0]
    if c == "stats":
        return cmd_stats() or 0
    if c == "unit":
        if len(argv) != 2:
            print("usage: parity.sh unit <W8-18o>", file=sys.stderr)
            return 2
        return cmd_unit(argv[1]) or 0
    if c == "reconcile":
        return cmd_reconcile()
    return cmd_set(argv[1:])


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
