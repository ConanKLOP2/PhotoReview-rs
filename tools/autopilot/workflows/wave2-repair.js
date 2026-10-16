export const meta = {
  name: 'wave2-repair',
  description: 'Close the open verifier findings of Wave 2 units on integration/wave-2 (real C# differences first, then mutation test gaps)',
  phases: [{ title: 'Repair' }, { title: 'Gate' }],
}
const REPO = 'C:/MyProjects/PhotoReview-rs'
const INTEG = REPO + '/.claude/worktrees/integ-w2'
const RUST = 'Rust: add $env:USERPROFILE\\.cargo\\bin (or $HOME/.cargo/bin) to PATH.'
const ITEMS = [
  { wu: 'W2-14', model: 'opus', file: 'crates/pr-core/src/io/physical.rs', todo: `REAL C# differences: (1) directory_exists("<root>\\\\sub<space>\\\\") and "<root>\\\\sub .\\\\" returns true, C# false; (2) get_file_stat("<root>\\\\sub\\\\b.txt<space>\\\\") returns Some, C# None. Trailing-space/dot path-segment semantics must follow C# (check with the .NET oracle or golden; W2-00 dotnet_path helpers may help). Also add tests killing surviving mutants: MAX_FOLLOWED_LINKS hop cap, skipping subdirectories in open_physical_listing. The 42 io_physical tests are #[ignore]d: keep them native-ignored but make sure they pass with --include-ignored.` },
  { wu: 'W2-17', model: 'sonnet', file: 'crates/pr-core/src/diagnostics/perf_csv.rs', todo: `REAL C# difference: PerfCsvListener::try_start/trace_dir_path differs from C# when the last trace-dir segment ends with a space or dot (106 of 231 cases differ). Fix to match C# (Path.GetFullPath semantics; use pr_core::text::dotnet_path). Add tests killing surviving mutants in perf_csv.rs: flush timer, DISPOSE_WAIT, CHANNEL_CAPACITY, FLUSH_INTERVAL.` },
  { wu: 'W2-00', model: 'sonnet', file: 'crates/pr-core/src/text/dotnet_path.rs', todo: `Add tests (with matching goldens inputs only if cheap, else hand-written C#-verified cases) killing 3 surviving mutants: directory_name_end second trim loop after a UNC root; root_length NT prefix check "\\\\??" missing separator; device UNC scan start 8 -> 7.` },
  { wu: 'W2-18', model: 'sonnet', file: 'crates/pr-core/src/updates/checker.rs', todo: `Add tests killing surviving mutants in is_rate_limited/is_leap: Unicode whitespace trim, 403-only gate, century leap-year rule.` },
  { wu: 'W2-01', model: 'sonnet', file: 'crates/pr-core/src/settings/app_settings.rs', todo: `Add tests killing 3 surviving mutants (surrogate guard widening; Infinity/-Infinity sign in json::to_f64; one more listed in the verifier notes in docs/PARITY.md/task_on_progress.md).` },
  { wu: 'W2-03', model: 'sonnet', file: 'crates/pr-core/src/settings/store.rs', todo: `Add tests killing 7 surviving mutants (M9, M17, M25, M26, M27, M35, M37 in the store tests; re-derive them by mutating store.rs). INV-11: never overwrite a corrupt or locked config.` },
  { wu: 'W2-12', model: 'sonnet', file: 'crates/pr-core/src/session/store.rs', todo: `Add a test killing mutant M20 (nests_too_deep escape handling): an escaped quote followed by >=64 '[' inside a string.` },
  { wu: 'W2-13', model: 'opus', file: 'crates/pr-core/src/session/writer.rs', todo: `Add a test killing mutant M22 (key_of fallback): two different uncanonicalisable folders stay two entries.` },
  { wu: 'W2-19', model: 'sonnet', file: 'crates/pr-core/src/instance/coalescer.rs', todo: `Add a test killing the mutant removing "if state.disposed { return; }" in flush_after_window: dispose-while-delay-ignores-cancel.` },
]
const REPORT = { type: 'object', properties: { wu: { type: 'string' }, status: { type: 'string', enum: ['fixed', 'partial', 'blocked'] }, tests_added: { type: 'number' }, mutants_still_alive: { type: 'array', items: { type: 'string' } }, notes: { type: 'string' } }, required: ['wu', 'status'] }

phase('Repair')
const res = await pipeline(ITEMS,
  it => agent(`Repair unit ${it.wu} in repo ${REPO}. ${RUST} Create your own worktree off the integration branch: git -C ${REPO} worktree add ${REPO}/.claude/worktrees/fix-${it.wu} -b fix/${it.wu} integration/wave-2. Own cargo target dir: CARGO_TARGET_DIR=${REPO}/target-wave-2/fix-${it.wu}. Read AGENTS.md, contracts/README.md, contracts/${it.wu}.md and ${it.file}; C# baseline d92d6aae via git -C ${REPO}/../PhotoReview show d92d6aae:<path> (read-only).
Task: ${it.todo}
Rules: change only the unit's owned files/tests; never edit tests/golden/; tests must fail when the guarded code is broken (verify by actually applying each mutation, then restoring); never touch the real Recycle Bin. Run cargo fmt --all; cargo clippy -p pr-core --all-targets -- -D warnings; cargo test -p pr-core (and -- --include-ignored for io_physical). Commit on fix/${it.wu} ("fix(${it.wu}): ..." + "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"). Do not push or merge. Report honestly, including mutants you could not kill.`,
    { label: `fix:${it.wu}`, phase: 'Repair', model: it.model, schema: REPORT }))
phase('Gate')
const gate = await agent(`Integrator in ${INTEG} (branch integration/wave-2, repo ${REPO}). ${RUST} Merge each branch fix/<WU> that exists for: ${ITEMS.map(i => i.wu).join(', ')} (git merge --no-ff; resolve conflicts keeping both sides). Reports: ${JSON.stringify(res.filter(Boolean))}. Then with CARGO_TARGET_DIR=${REPO}/target-wave-2/integ run: cargo fmt --all --check; cargo clippy --workspace --all-targets --locked -- -D warnings; cargo test --workspace --locked (bounded); cargo test -p pr-core --test io_physical -- --include-ignored. Update docs/PARITY.md rows of units now fixed from Partial to Done (only where the report says fixed and mutants are dead) and task_on_progress.md with the real results. Commit, git push origin integration/wave-2. Then comment on PR #8 via gh pr comment with a short summary of what is closed and what remains. Report honestly.`, { label: 'gate', phase: 'Gate', model: 'sonnet' })
return { res, gate }
