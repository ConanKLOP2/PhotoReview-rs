# Rules for Codex (PhotoReviewRs port). Read all of them; the pack below this file is your whole context.

1. Work ONLY on the files listed under "Owned files" in the contract. Create or edit nothing else (no Cargo.toml, lib.rs, mod.rs unless listed, no docs, no goldens, no fixtures). If something else seems to need a change, say so in your summary instead.
2. Do not explore the repository. The pack contains the contract, the plan row, the C# sources and the signatures of the units you depend on. Open another file only if the pack names it.
3. Never change a public signature given in the contract ("Public API"). Keep stubs' names, argument types and return types.
4. C# is the spec. If the pack is missing a C# file, read it only with: `git -c safe.directory=C:/MyProjects/PhotoReview -C C:/MyProjects/PhotoReview show <baseline>:<path>` (`d92d6aae` for Wave 8; Wave 9 units use the baseline named in their contract and plan, for example `7eaee2a2`). Never run any other git command in the C# repo, never check out, edit or change config there.
5. Behaviour must be identical to the C# code (strings, ordering, edge cases, error messages). Mirror C# test names as `method_scenario_expected_result` with a `// C#: Class.Method` comment.
6. Never touch the real Recycle Bin, the user's profile, `%LOCALAPPDATA%`, the C# app's data, or any path outside your worktree. Tests use temp dirs only (tempfile) and fake/in-memory file systems.
7. `unsafe` is allowed only in the crate `pr-platform-win`, each block with a `// SAFETY:` comment. Anywhere else: no `unsafe`.
8. No `unwrap()`, `expect()`, `panic!`, indexing that can panic, or `as` truncation on code paths that handle user data (files, paths, settings, journal, sessions). Return errors or handle the case. `unwrap` is fine in tests.
9. No `todo!()` / `unimplemented!()` left in owned source files when you finish; remove any `#![allow(...)]` the stub added only for them.
10. Keep clippy clean with `-D warnings` and `cargo fmt` clean. Write doc comments for public items (`missing_docs` may be on).
11. Build and test only your crate: `cargo test -p <crate>`, `cargo clippy -p <crate> --all-targets -- -D warnings`. Always set these env vars first: `CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_INCREMENTAL=0` and keep the `CARGO_TARGET_DIR` that is already set (do not change it, do not use another target dir).
12. Do not run `git commit`, `git add`, `git checkout`, `git reset`, `git stash`, or `git push`. The orchestrator commits. Do not delete files outside the owned set. Do not create scratch files in the repo.
13. Do not weaken or delete tests to make them pass. A failing test means the implementation is wrong, unless the test contradicts the C# source (then say so in the summary).
14. When done, print a summary of at most 5 lines: files changed, tests passing, anything skipped or doubtful. Nothing else.
