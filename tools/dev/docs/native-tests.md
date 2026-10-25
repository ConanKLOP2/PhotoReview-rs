# native-tests.sh

Purpose: list and run the native (`#[ignore = "native...`) integration tests of a crate, one test binary at a time.

Usage:
- `bash tools/dev/native-tests.sh --list` : default crate `pr-platform-win`; per-file counts, desktop check, no cargo call.
- `bash tools/dev/native-tests.sh [crate]` : runs `cargo test -p <crate> --locked --test <name> -- --include-ignored --test-threads=1` per file, prints pass/fail per binary and a final table. Exit 1 if any binary fails.

The desktop check calls `SetCursorPos` to the current cursor position through PowerShell. Without an interactive input desktop the tests that move the real cursor (the two NativeDialog tooltip tests) cannot pass; a warning is printed.

Env overrides: `CARGO_BIN`, `PS_BIN`, `TIMEOUT_BIN`, `NATIVE_TIMEOUT` (seconds per binary, default 900), `NATIVE_TESTS_ROOT`.

Safety: the tests create hidden windows only. The script deletes nothing outside its own mktemp log directory, kills no process by name, and never touches the Recycle Bin. Run it on a machine with no other cargo build in the same target dir. Only src unit tests (`--lib`) with native tests are not covered.
