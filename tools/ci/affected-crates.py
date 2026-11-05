#!/usr/bin/env python3
"""Decide which Rust test groups a pull request needs (CI job `changes`).

Input: the list of changed files (`--files-from FILE|-`, one path per line, or `--diff A...B` which runs git diff --name-only).
Output: one JSON object on stdout and, with `--github-output FILE`, the same values as `key=value` lines:

  full      true when everything must run exactly as on master (unknown path, parse error, any doubt)
  rust      true when lint and tests are needed at all (false only for docs/tools/markdown-only changes)
  affected  `ALL` or the space separated package names that need tests (changed workspace crates plus all reverse dependencies)
  groups    JSON array with the test matrix groups to run (core, crates-rest, win, app), never empty when rust is true

Fail closed: any path that is not understood, any crate not in `cargo metadata`, any error at all gives full=true.
The reverse dependency closure counts every dependency kind (normal, dev, build) between workspace members, because a dev
dependency (pr-test-support, pr-libraw -> pr-imaging-raw) changes what the dependent's tests execute.
"""
import argparse
import json
import subprocess
import sys

# Files whose change can alter every crate: build everything.
FULL_EXACT = {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".github/workflows/ci.yml"}
FULL_PREFIX = (".cargo/", "tests/", "assets/", "tools/ci/")
# Inputs that tests of one crate read from outside crates/ (found by grep of include_str!/repo_root joins). They only widen
# the set when some crate changed too; on their own they stay docs-only exactly as before (lint and tests skipped).
HIDDEN_INPUTS = (
    ("docs/PARITY.md", "pr-shell"),  # crates/pr-shell/tests/ui_parity_map.rs
    ("docs/drafts/", "pr-bench"),  # crates/pr-bench/tests/bench_compare_cli.rs
    ("tools/dev/make-synthetic-raw.py", "pr-libraw"),  # include_str! in three pr-libraw tests
)
# Same split as the `test` matrix in ci.yml.
GROUP_CORE, GROUP_WIN, GROUP_APP = "core", "win", "app"
CORE_PKG, WIN_PKG, APP_PKG = "pr-core", "pr-platform-win", "pr-app"
ALL_GROUPS = ["core", "crates-rest", "win", "app"]


def is_ignored(path):
    """Not Rust relevant, same rule as before this script: docs/, tools/ and markdown anywhere."""
    return path.startswith(("docs/", "tools/")) or path.endswith(".md")


def load_graph(metadata):
    root = metadata["workspace_root"].replace("\\", "/").rstrip("/")
    members = {}
    for package in metadata["packages"]:
        manifest = package["manifest_path"].replace("\\", "/")
        if not manifest.startswith(root + "/"):
            raise ValueError("member outside workspace: " + manifest)
        directory = manifest[len(root) + 1 :].rsplit("/", 1)[0]
        members[package["name"]] = directory
    reverse = {name: set() for name in members}
    for package in metadata["packages"]:
        for dep in package["dependencies"]:
            if dep["name"] in members and dep.get("path"):
                reverse[dep["name"]].add(package["name"])
    return members, reverse


def closure(seeds, reverse):
    seen = set(seeds)
    todo = list(seeds)
    while todo:
        for dependent in reverse[todo.pop()]:
            if dependent not in seen:
                seen.add(dependent)
                todo.append(dependent)
    return seen


def groups_for(affected):
    groups = []
    if CORE_PKG in affected:
        groups.append("core")
    if affected - {CORE_PKG, WIN_PKG, APP_PKG}:
        groups.append("crates-rest")
    if WIN_PKG in affected or APP_PKG in affected:
        groups.append("win")
    if APP_PKG in affected:
        groups.append("app")
    return groups


def decide(files, metadata):
    members, reverse = load_graph(metadata)
    by_dir = {directory: name for name, directory in members.items()}
    seeds, extras, notes = set(), set(), []
    for path in files:
        path = path.strip().replace("\\", "/")
        if not path:
            continue
        if path in FULL_EXACT or path.startswith(FULL_PREFIX) or path.endswith("/Cargo.toml"):
            return full("full run because of " + path)
        if path.startswith("crates/"):
            parts = path.split("/")
            directory = "/".join(parts[:2])
            if len(parts) < 3 or directory not in by_dir:
                return full("unknown crate path " + path)
            seeds.add(by_dir[directory])
            continue
        hidden = [crate for prefix, crate in HIDDEN_INPUTS if path == prefix or (prefix.endswith("/") and path.startswith(prefix))]
        if hidden:
            extras.update(hidden)
            notes.append(path)
            continue
        if is_ignored(path):
            continue
        return full("unknown path " + path)
    if not seeds:
        return {"full": False, "rust": False, "affected": "", "groups": [], "reason": "only docs/tools/markdown changed"}
    affected = closure(seeds | extras, reverse)
    groups = groups_for(affected)
    if not groups:
        return full("no test group for " + ",".join(sorted(affected)))
    if len(groups) == len(ALL_GROUPS) and affected == set(members):
        return {"full": False, "rust": True, "affected": "ALL", "groups": ALL_GROUPS,
                "reason": "every workspace crate is affected by " + ",".join(sorted(seeds))}
    return {"full": False, "rust": True, "affected": " ".join(sorted(affected)), "groups": groups,
            "reason": "changed " + ",".join(sorted(seeds)) + (" + inputs of " + ",".join(sorted(extras)) if extras else "")}


def full(reason):
    return {"full": True, "rust": True, "affected": "ALL", "groups": ALL_GROUPS, "reason": reason}


def main():
    parser = argparse.ArgumentParser()
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--files-from", help="file with changed paths, or - for stdin")
    source.add_argument("--diff", help="git revision range, e.g. origin/master...HEAD")
    parser.add_argument("--metadata-file", help="saved `cargo metadata --no-deps --format-version 1` output (tests)")
    parser.add_argument("--github-output")
    args = parser.parse_args()
    try:
        if args.diff:
            files = subprocess.run(["git", "diff", "--name-only", args.diff], check=True, capture_output=True, text=True).stdout.splitlines()
        elif args.files_from == "-":
            files = sys.stdin.read().splitlines()
        else:
            with open(args.files_from, encoding="utf-8") as handle:
                files = handle.read().splitlines()
        if args.metadata_file:
            with open(args.metadata_file, encoding="utf-8") as handle:
                metadata = json.load(handle)
        else:
            raw = subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
                                 check=True, capture_output=True, text=True).stdout
            metadata = json.loads(raw)
        result = decide(files, metadata)
    except Exception as error:  # fail closed on any doubt
        result = full("fail closed: %s: %s" % (type(error).__name__, error))
    print(json.dumps(result))
    if args.github_output:
        with open(args.github_output, "a", encoding="utf-8") as handle:
            handle.write("full=%s\nrust=%s\naffected=%s\ngroups=%s\n" % (
                str(result["full"]).lower(), str(result["rust"]).lower(), result["affected"], json.dumps(result["groups"])))


if __name__ == "__main__":
    main()
