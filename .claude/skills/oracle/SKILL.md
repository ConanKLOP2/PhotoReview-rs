---
name: oracle
description: Use when the owner asks to regenerate golden files with the C# oracle or to check that oracle output is reproducible.
---

The oracle builds the C# source and is slow; run it only when the owner asked for it, and only for the named commands.

1. Run `bash tools/dev/oracle.sh <command>... [--runs 2] [--commit <sha>] [--keep]` from the repo root.
2. Report in Vietnamese, per command: identical or DIFFERENT between runs, and the comparison with the committed golden
   (same, differs only in csharp_commit, or differs).
3. Report every WARNING about missing turbojpeg.dll or libraw.dll and every FAILED run.
4. Without `--keep` the committed goldens are restored; use `--keep` only when the owner wants to commit the regenerated files.
Porters never edit tests/golden by hand.

Run every command from the repository root.
