#!/usr/bin/env python3
"""Helpers for the Claude-free Codex pipeline (plan/contract parsing, JSON state). No third-party imports."""
import json, os, re, sys
sys.stdout.reconfigure(encoding="utf-8", newline=chr(10))

def read(p):
    with open(p, encoding="utf-8", errors="replace") as f:
        return f.read()

def plan_rows(plan):
    """Yield dicts for table rows `| W8-04a | files | rust | col | model | test | deps | batch |`."""
    for line in read(plan).splitlines():
        if not re.match(r"^\|\s*W\d+-\d\d[a-z]\s*\|", line):
            continue
        c = [x.strip() for x in line.strip().strip("|").split("|")]
        if len(c) < 8:
            continue
        yield {"id": c[0], "deps_raw": c[6], "batch": c[7], "line": line}

def expand_deps(raw, uid):
    prefix = uid.rsplit("-", 1)[0]
    raw = re.sub(r"`[^`]*`", " ", raw)
    out = []
    for a, b in re.findall(r"\b(\d\d[a-z])\.\.(\d\d[a-z])\b", raw):
        if a[:2] == b[:2]:
            out += [a[:2] + chr(x) for x in range(ord(a[2]), ord(b[2]) + 1)]
    raw = re.sub(r"\b\d\d[a-z]\.\.\d\d[a-z]\b", " ", raw)
    out += re.findall(r"\b(\d\d[a-z])\b", raw)
    out += re.findall(r"\bW\d+-(\d\d[a-z])\b", raw)
    seen, res = set(), []
    for t in out:
        d = prefix + "-" + t
        if d not in seen:
            seen.add(d); res.append(d)
    return res

def cmd_order(plan, batch, only=None):
    rows = [r for r in plan_rows(plan) if r["batch"].upper() == batch.upper()]
    ids = [r["id"] for r in rows]
    deps = {r["id"]: [d for d in expand_deps(r["deps_raw"], r["id"]) if d in ids and d != r["id"]] for r in rows}
    done, order = set(), []
    while len(order) < len(ids):
        prog = False
        for i in ids:
            if i not in done and all(d in done for d in deps[i]):
                done.add(i); order.append(i); prog = True
        if not prog:
            sys.exit("dependency cycle in batch: " + ",".join(i for i in ids if i not in done))
    sel = set(only.split(",")) if only else None
    for i in order:
        if sel is None or i in sel:
            print(i + "\t" + ",".join(deps[i]))

def cmd_row(plan, uid):
    for r in plan_rows(plan):
        if r["id"] == uid:
            print(r["line"]); return
    sys.exit("no plan row for " + uid)

def cmd_deps(plan, uid):
    for r in plan_rows(plan):
        if r["id"] == uid:
            print("\n".join(expand_deps(r["deps_raw"], uid))); return

def owned(contract):
    res, on = [], False
    for line in read(contract).splitlines():
        if not on:
            if re.search(r"Owned files", line) and re.search(r"only", line, re.I):
                on = True
            continue
        m = re.match(r"^\s{2,}`?([\w./-]+\.\w+)`?(\s|$)", line)
        if m and "/" in m.group(1):
            res.append(m.group(1)); continue
        if line.strip() == "":
            continue
        if re.match(r"^\s*(-\s*)?(Lead-owned|Read-only)|^#|^-\s", line) or re.match(r"^\S", line):
            break
        # continuation / comment line: keep scanning
    return res

def cmd_cs(contract):
    on, res = False, []
    for line in read(contract).splitlines():
        if line.startswith("## "):
            on = line.startswith("## C# ")
            continue
        if not on:
            continue
        for t in re.findall(r"`([^`\s]+\.cs)`", line):
            if "tools/" in t or "/Commands/" in t or re.match(r"W\d+-", t) or t in res:
                continue
            res.append(t)
    print("\n".join(res))

def cmd_baseline(contract):
    m = re.search(r"[Bb]aseline[^`\n]*`([0-9a-f]{7,40})`", read(contract))
    print(m.group(1) if m else "")

def state_load(p):
    try:
        return json.loads(read(p))
    except Exception:
        return {"units": {}, "meta": {}}

def state_save(p, s):
    os.makedirs(os.path.dirname(p) or ".", exist_ok=True)
    tmp = p + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(s, f, indent=1, sort_keys=True)
    os.replace(tmp, p)

def main(a):
    c = a[0]
    if c == "order": cmd_order(a[1], a[2], a[3] if len(a) > 3 else None)
    elif c == "row": cmd_row(a[1], a[2])
    elif c == "deps": cmd_deps(a[1], a[2])
    elif c == "owned": print("\n".join(owned(a[1])))
    elif c == "cs": cmd_cs(a[1])
    elif c == "baseline": cmd_baseline(a[1])
    elif c == "sget":      # sget file id -> status (or empty)
        print(state_load(a[1])["units"].get(a[2], {}).get("status", ""))
    elif c == "sset":      # sset file id status [note] [survivors]
        s = state_load(a[1]); u = s["units"].setdefault(a[2], {})
        u["status"] = a[3]; u["note"] = a[4] if len(a) > 4 else ""
        u["survivors"] = int(a[5]) if len(a) > 5 and a[5] else u.get("survivors", 0)
        state_save(a[1], s)
    elif c == "slist":     # slist file -> id<TAB>status<TAB>survivors<TAB>note
        for k, v in sorted(state_load(a[1])["units"].items()):
            print("\t".join([k, v.get("status", ""), str(v.get("survivors", 0)), v.get("note", "")]))
    else:
        sys.exit("unknown command " + c)

if __name__ == "__main__":
    main(sys.argv[1:])
