export const meta = {
  name: 'wave4-finish',
  description: 'Finish Wave 4: merge the implemented-but-blocked units, complete the unfinished ones (dependencies now merged), repair open FAIL verdicts, gate, push and open the PR',
  phases: [{ title: 'Merge' }, { title: 'Finish' }, { title: 'Repair' }, { title: 'Gate' }],
}
const REPO = 'C:/MyProjects/PhotoReview-rs'
const INTEG = REPO + '/.claude/worktrees/integ-w4'
const CS = 'C:/MyProjects/PhotoReview-rs/../PhotoReview'
const RUST = 'Rust: add $HOME/.cargo/bin (PowerShell $env:USERPROFILE\\.cargo\\bin) to PATH. ALWAYS build with CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_INCREMENTAL=0 and your own CARGO_TARGET_DIR (delete it when done; delete %TEMP%\\cargo-mutants-*.tmp you create).'
const NEVER = 'NEVER touch the real Recycle Bin; never edit the C# repo (git show only); never leave edits in the main checkout (work only in your worktree); native WIC tests are #[ignore = "native: needs WIC"].'
const CO = 'Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>'
const common = `Repo ${REPO}. ${RUST} ${NEVER} Read AGENTS.md, contracts/README.md (D1-D44, A1-A7), docs/PROPOSED-DECISIONS.md, docs/WAVE-4-PLAN.md, the section "Wave 4 open verdicts" of docs/PARITY.md and the Lessons in docs/MULTI-AGENT-PLAN.md. C# spec at d92d6aae via git -C ${CS} show d92d6aae:<path> only.`
const R = { type: 'object', properties: { wu: { type: 'string' }, status: { type: 'string', enum: ['done', 'partial', 'blocked'] }, verdict: { type: 'string' }, tests: { type: 'string' }, open: { type: 'array', items: { type: 'string' } }, notes: { type: 'string' } }, required: ['wu', 'status'] }
const M = { type: 'object', properties: { merged: { type: 'array', items: { type: 'string' } }, not_merged: { type: 'array', items: { type: 'string' } }, test: { type: 'string' }, problems: { type: 'array', items: { type: 'string' } } }, required: ['merged', 'problems'] }

// Units that were "blocked" only because their tests need stubs of other units in the same tier.
const BLOCKED = ['W4-01b', 'W4-04a', 'W4-03b', 'W4-02b', 'W4-04b', 'W4-06b', 'W4-10b', 'W4-12a', 'W4-14b', 'W4-14c', 'W4-14d']
const opusU = new Set(['W4-10b', 'W4-14b', 'W4-14c'])
// Units with an open FAIL verdict from the first pass.
const FAILED = ['W4-01c', 'W4-16a', 'W4-16b', 'W4-08a', 'W4-09', 'W4-10a', 'W4-11a', 'W4-11b', 'W4-11c', 'W4-11e', 'W4-14a', 'W4-15a']
const opusF = new Set(['W4-09', 'W4-10a', 'W4-11a', 'W4-11c', 'W4-14a', 'W4-15a'])

phase('Merge')
const merge = await agent(`${common}
Integrator in ${INTEG} (branch integration/wave-4). Several units were reported "blocked" only because their tests need stubs of other units of the same tier; their branches wu/<WU> exist (worktrees .claude/worktrees/<WU>) and contain real implementations. Merge, in this dependency order, every branch among ${JSON.stringify(BLOCKED)} that exists and has commits beyond its red-test commit: W4-01b, W4-04a, W4-03b, W4-02b, W4-04b, W4-06b, W4-12a, W4-10b, then W4-14b, W4-14c, W4-14d (git merge --no-ff, "merge(<WU>)" + ${CO}; PARITY.md conflicts: keep both sides, then tidy duplicates). After each merge run cargo check -p pr-imaging -p pr-platform-win. Then run the workspace gate: CARGO_TARGET_DIR=${REPO}/target-wave-4/integ cargo fmt --all; cargo clippy --workspace --all-targets --locked -- -D warnings; cargo test --workspace --locked --no-fail-fast. Do not push. Report which binaries still fail and why (remaining todo!() stubs per unit).`, { label: 'merge-blocked', phase: 'Merge', model: 'sonnet', schema: M })

phase('Finish')
const fin = await pipeline(BLOCKED,
  u => agent(`${common}
Finish unit ${u}. Its dependencies are now merged in integration/wave-4 (worktree ${INTEG}). Work in a fresh worktree: git -C ${REPO} worktree add ${REPO}/.claude/worktrees/f-${u} -b fin/${u} integration/wave-4 (CARGO_TARGET_DIR=${REPO}/target-wave-4/f-${u}); remove old worktree .claude/worktrees/${u} if it is only a stale copy. Read contracts/${u}.md. Check the unit's stub(s): if bodies are still todo!(), implement them (opus-level care for INV-1/2/8/12); then make sure EVERY test of the contract (red tests, goldens, C# tests) exists and passes; fix whatever the previous implement/verify left open (see PARITY "Wave 4 open verdicts", PARITY rows for ${u}). Run cargo fmt, clippy -D warnings, cargo test -p <crate> for the unit's test files. Then adversarial self-verification against the C# (edge cases, formats, error paths) with at least 10 manual mutations of the owned files (restore after each) — every non-equivalent mutant must be killed by a test. Commit "port(${u}): finish" + ${CO}. Do not push. Report.`,
    { label: `finish:${u}`, phase: 'Finish', model: opusU.has(u) ? 'opus' : 'sonnet', schema: R }))

phase('Repair')
const rep = await pipeline(FAILED,
  u => agent(`${common}
Repair unit ${u} (verdict FAIL in the first pass; the open findings are in docs/PARITY.md "Wave 4 open verdicts" and in contracts/${u}.md). Work in a fresh worktree off integration/wave-4: git -C ${REPO} worktree add ${REPO}/.claude/worktrees/r-${u} -b rep/${u} integration/wave-4 (CARGO_TARGET_DIR=${REPO}/target-wave-4/r-${u}). Fix every real behaviour difference with C# d92d6aae (use the .NET oracle for doubtful semantics) and add tests that kill the surviving non-equivalent mutants; accept and document equivalent or privilege-bound ones. Never weaken tests; keep stub signatures except per amendments A1-A7. Run fmt, clippy -D warnings, cargo test -p <crate>. Commit "fix(${u}): ..." + ${CO}. Do not push. Report remaining open items honestly.`,
    { label: `repair:${u}`, phase: 'Repair', model: opusF.has(u) ? 'opus' : 'sonnet', schema: R }))

phase('Gate')
const gate = await agent(`${common}
Final integrator in ${INTEG} (branch integration/wave-4). Merge every fin/* and rep/* branch that exists (git merge --no-ff; conflicts in PARITY.md/mod.rs: keep both sides and tidy). Finish reports: ${JSON.stringify(fin.filter(Boolean))}. Repair reports: ${JSON.stringify(rep.filter(Boolean))}. Then CARGO_TARGET_DIR=${REPO}/target-wave-4/integ: cargo fmt --all; cargo clippy --workspace --all-targets --locked -- -D warnings; cargo test --workspace --locked --no-fail-fast; cargo test -p pr-imaging -p pr-platform-win -- --include-ignored for native tests that only touch their own temp dirs (WIC tests are allowed). Fix only trivial integration breakage. Merge origin/master into the branch (conflicts: keep both sides). Update docs/PARITY.md (Done where all tests pass and mutants dead, Partial with the list otherwise), tests/golden-inputs/README.md, task_on_progress.md with ONLY what you ran, and the Lessons section (blocked-by-tier-stubs is the main lesson: P-28 graph scheduling). Remove all fin/*, rep/*, wu/* worktrees and branches that are merged and all target-wave-4 dirs. Commit. Do NOT push (the lead pushes and opens the PR). Report gate results honestly.`, { label: 'gate', phase: 'Gate', model: 'sonnet', schema: M })
return { merge, fin: fin.filter(Boolean).map(x => ({ wu: x.wu, s: x.status, open: x.open })), rep: rep.filter(Boolean).map(x => ({ wu: x.wu, s: x.status, open: x.open })), gate }
