---
name: native-tests
description: Use when the user asks which native Windows tests exist for a crate, whether the machine can run them, or to run them.
---

Run `bash tools/dev/native-tests.sh --list` first (default crate pr-platform-win; pass another crate name if asked).
Report in Vietnamese in at most 10 lines: number of test files and native tests, and whether an interactive input desktop exists.
If there is no desktop, warn that the two NativeDialog tooltip tests (they move the real cursor) cannot pass.
Run the tests (`bash tools/dev/native-tests.sh [crate]`, slow, hidden windows only) ONLY if the user explicitly asks to run them.
When running, never run other cargo builds at the same time, and report the final table: passed, failed, and which binaries failed.
Never delete anything and never touch the real Recycle Bin.

Run every command from the repository root.
