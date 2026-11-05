"""Selftest for affected-crates.py: python -I tools/ci/test_affected_crates.py (needs cargo on PATH for the live metadata)."""
import importlib.util
import json
import os
import subprocess
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("affected", os.path.join(HERE, "affected-crates.py"))
affected = importlib.util.module_from_spec(spec)
spec.loader.exec_module(affected)
ROOT = os.path.dirname(os.path.dirname(HERE))
METADATA = json.loads(subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
                                     check=True, capture_output=True, text=True, cwd=ROOT).stdout)
EVERY = {p["name"] for p in METADATA["packages"]}


def run(*files):
    return affected.decide(list(files), METADATA)


def pkgs(result):
    return EVERY if result["affected"] == "ALL" else set(result["affected"].split())


class AffectedCrates(unittest.TestCase):
    def test_pr_core_reaches_every_crate(self):
        self.assertEqual(pkgs(run("crates/pr-core/src/lib.rs")), EVERY)

    def test_pr_imaging_reverse_dependencies(self):
        r = run("crates/pr-imaging/src/a.rs")
        self.assertEqual(pkgs(r), {"pr-imaging", "pr-app", "pr-bench", "pr-bench-win", "pr-imaging-raw", "pr-libraw",
                                   "pr-platform-win", "pr-shell", "pr-turbojpeg"})
        self.assertEqual(r["groups"], ["crates-rest", "win", "app"])

    def test_pr_platform_win_reverse_dependencies(self):
        r = run("crates/pr-platform-win/tests/a.rs")
        self.assertEqual(pkgs(r), {"pr-platform-win", "pr-bench-win", "pr-imaging-raw", "pr-libraw", "pr-shell", "pr-turbojpeg"})
        self.assertEqual(r["groups"], ["crates-rest", "win"])

    def test_leaf_crate_runs_one_group(self):
        self.assertEqual(run("crates/pr-shell/src/a.rs")["groups"], ["crates-rest"])

    def test_docs_only_is_not_rust(self):
        r = run("docs/PARITY.md", "README.md", "tools/dev/pre-pr.sh")
        self.assertFalse(r["rust"])

    def test_hidden_input_widens_only_with_a_crate_change(self):
        self.assertIn("pr-shell", pkgs(run("crates/pr-perf/src/a.rs", "docs/PARITY.md")))
        self.assertFalse(run("docs/PARITY.md")["rust"])

    def test_fail_closed(self):
        for path in ["Cargo.lock", "crates/pr-core/Cargo.toml", "Cargo.toml", "rust-toolchain.toml", ".cargo/config.toml",
                     "tests/fixtures/a.json", "assets/languages/en.json", ".github/workflows/ci.yml", "tools/ci/affected-crates.py",
                     "crates/pr-unknown/src/a.rs", "crates/stray.txt", "native/x64/a.dll", "contracts/a.json", "rustfmt.toml"]:
            r = run("crates/pr-perf/src/a.rs", path)
            self.assertTrue(r["full"], path)
            self.assertEqual(r["groups"], ["core", "crates-rest", "win", "app"], path)

    def test_main_fails_closed_on_bad_metadata(self):
        bad = os.path.join(HERE, "_bad_metadata.json")
        try:
            with open(bad, "w") as handle:
                handle.write("{}")
            out = subprocess.run([sys.executable, "-I", os.path.join(HERE, "affected-crates.py"), "--files-from", "-", "--metadata-file", bad],
                                 input="crates/pr-core/a.rs\n", capture_output=True, text=True).stdout
        finally:
            os.remove(bad)
        self.assertTrue(json.loads(out)["full"])


if __name__ == "__main__":
    unittest.main()
