export const meta = {
  name: 'wave3-port',
  description: 'Port Wave 3 (data safety) test-first: seams, then W3-02/07/08, W3-01/03, W3-04, W3-05/06, W3-09 integration; Opus verify with mutation gate; one PR',
  phases: [
    { title: 'Setup' }, { title: 'Seams' },
    { title: 'Tier1', detail: 'W3-02 W3-07 W3-08' }, { title: 'Integrate1' },
    { title: 'Tier2', detail: 'W3-01 W3-03' }, { title: 'Integrate2' },
    { title: 'Tier3', detail: 'W3-04' }, { title: 'Integrate3' },
    { title: 'Tier4', detail: 'W3-05 W3-06' }, { title: 'Integrate4' },
    { title: 'Matrix', detail: 'W3-09 fault/crash integration' }, { title: 'Finish' },
  ],
}
const REPO = 'C:/MyProjects/PhotoReview-rs'
const INTEG = REPO + '/.claude/worktrees/integ-w3'
const CS = 'C:/MyProjects/PhotoReview'
const RUST = 'Rust: add $HOME/.cargo/bin (PowerShell $env:USERPROFILE\\.cargo\\bin) to PATH. Run tools/*.ps1 from the PowerShell tool with powershell -NoProfile -ExecutionPolicy Bypass -File.'
const NEVER = 'NEVER run code that deletes from, sweeps or empties the real Recycle Bin (AGENTS.md); use fakes. Never touch the C# repo except git show.'
const CO = 'Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>'

const PORT = { type: 'object', properties: { wu: { type: 'string' }, status: { type: 'string', enum: ['done', 'blocked'] }, branch: { type: 'string' }, commit: { type: 'string' }, red_tests: { type: 'number' }, green_tests: { type: 'number' }, skipped_csharp_tests: { type: 'array', items: { type: 'string' } }, behaviour_doubts: { type: 'array', items: { type: 'string' } }, notes: { type: 'string' } }, required: ['wu', 'status', 'branch'] }
const VER = { type: 'object', properties: { wu: { type: 'string' }, verdict: { type: 'string', enum: ['PASS', 'FAIL'] }, failing_tests: { type: 'array', items: { type: 'string' } }, surviving_mutations: { type: 'array', items: { type: 'string' } }, behaviour_differences: { type: 'array', items: { type: 'string' } }, mutants_tool: { type: 'string' } }, required: ['wu', 'verdict'] }
const INT = { type: 'object', properties: { merged: { type: 'array', items: { type: 'string' } }, not_merged: { type: 'array', items: { type: 'string' } }, fmt: { type: 'string' }, clippy: { type: 'string' }, test: { type: 'string' }, problems: { type: 'array', items: { type: 'string' } } }, required: ['merged', 'not_merged', 'problems'] }

const TIERS = [['W3-02', 'W3-07', 'W3-08'], ['W3-01', 'W3-03'], ['W3-04'], ['W3-05', 'W3-06']]
const common = `Repo ${REPO}. ${RUST} ${NEVER} Read AGENTS.md, contracts/README.md (D1-D28 and docs/PROPOSED-DECISIONS.md P-01..P-16: act on them as written), docs/MULTI-AGENT-PLAN.md sections 7, 10, 11. C# spec at baseline d92d6aae only via git -C ${CS} show d92d6aae:<path>.`

const redPrompt = u => `${common}
Test-first step for ${u} (you are the test porter). Create your worktree: git -C ${REPO} worktree add ${REPO}/.claude/worktrees/${u} -b wu/${u} integration/wave-3 ; own CARGO_TARGET_DIR=${REPO}/target-wave-3/${u}. Read contracts/${u}.md. Port EVERY C# test the contract lists (names mirror C#, a comment names the C# test) into Rust tests in the contract's test files, plus the golden-driven tests over tests/golden/<unit goldens>.json. They must compile against the stubs and FAIL (todo!() panics): that is the point. Do not implement bodies. Tests the contract marks not portable get a documented skip (list them). Commit "test(${u}): red tests" + ${CO}. Report counts.`
const impPrompt = (u, extra) => `${common}
Implement ${u} (Opus porter). Worktree ${REPO}/.claude/worktrees/${u} (branch wu/${u}; create it from integration/wave-3 if missing), own CARGO_TARGET_DIR=${REPO}/target-wave-3/${u}. Read contracts/${u}.md and the red tests already committed. Fill only the contract's Owned files (stubs). Make every red test green without weakening them. Keep stub signatures. No unwrap/expect on file-system or user-data paths; unsafe only with SAFETY comments where the contract allows. Journal/session formats stay byte-identical to C#. Run cargo fmt --all; cargo clippy -p pr-core --all-targets -- -D warnings; cargo test -p pr-core (and --include-ignored for native tests that only touch their own temp dirs). Update the PARITY.md rows. Commit "port(${u}): <name>" + ${CO}. Do not merge or push. ${extra || ''}`
const verPrompt = u => `${common}
Adversarial verification of ${u} (Opus verifier). Branch wu/${u}, worktree ${REPO}/.claude/worktrees/${u}; CARGO_TARGET_DIR=${REPO}/target-wave-3/${u}-verify. (1) Compare against the C# at d92d6aae for behaviour differences: edge cases, error paths, ordering, path/string comparison, number/time formatting, INV-3..INV-6/8/11 as relevant; for each suspected difference write a failing Rust test (kept out of the commit). Use the .NET oracle (tools/oracle.ps1 or tools/oracle-cs) for ambiguous semantics, never edit tests/golden of other units. (2) Mutation gate: install and run cargo-mutants on the unit's files if possible (cargo install cargo-mutants --locked; cargo mutants -p pr-core -f <file> --no-shuffle with a timeout per mutant; list missed mutants), else apply 15+ manual mutations (flip a condition, drop a write, reorder journal steps). Every non-equivalent mutant on the journal write path must be killed. Do not fix the code. Answer PASS only with no real differences and no surviving non-equivalent mutants; name the tool used.`

async function runTier(units, tier) {
  return await pipeline(units,
    u => agent(redPrompt(u), { label: `red:${u}`, phase: tier, model: 'haiku', schema: PORT }),
    (r, u) => (r && r.status === 'done') ? agent(impPrompt(u), { label: `impl:${u}`, phase: tier, model: 'opus', schema: PORT }) : r,
    (p, u) => (p && p.status === 'done') ? agent(verPrompt(u), { label: `verify:${u}`, phase: tier, model: 'opus', schema: VER }).then(v => ({ p, v })) : { p, v: null },
    async (r, u) => {
      if (!r || !r.p || r.p.status !== 'done') return r
      let { p, v } = r
      for (let round = 0; round < 3 && v && v.verdict === 'FAIL'; round++) {
        const rep = await agent(impPrompt(u, `REPAIR round ${round + 1}: fix these verifier findings with real code fixes plus tests that kill them; never weaken tests. Failing: ${JSON.stringify(v.failing_tests)} Surviving mutants: ${JSON.stringify(v.surviving_mutations)} Differences: ${JSON.stringify(v.behaviour_differences)}`),
          { label: `repair${round + 1}:${u}`, phase: tier, model: 'opus', schema: PORT })
        if (!rep) break
        p = rep
        v = await agent(verPrompt(u), { label: `reverify${round + 1}:${u}`, phase: tier, model: 'opus', schema: VER })
      }
      return { p, v }
    })
}
const integ = (name, results) => agent(`Integrator, worktree ${INTEG} (branch integration/wave-3), repo ${REPO}. ${RUST} ${NEVER} Merge each wu/<WU> below with git merge --no-ff ("merge(<WU>)" + ${CO}); resolve mod.rs/PARITY.md conflicts keeping both sides; the journal stubs are split across W3-01/02/03 (D23/D28): if two units define the same type, keep the one the contract owner defines and fix imports. Verdicts (FAIL still merges if the branch builds; list it): ${JSON.stringify(results.filter(Boolean).map(r => ({ wu: r.p && r.p.wu, branch: r.p && r.p.branch, status: r.p && r.p.status, verdict: r.v && r.v.verdict, open: r.v && { f: r.v.failing_tests, m: r.v.surviving_mutations, d: r.v.behaviour_differences } })))}. Then CARGO_TARGET_DIR=${REPO}/target-wave-3/integ: cargo fmt --all; cargo clippy --workspace --all-targets --locked -- -D warnings; cargo test --workspace --locked --no-fail-fast (bounded). Fix only trivial integration breakage. Report honestly. Do not push.`, { label: `integrate:${name}`, phase: name, model: 'sonnet', schema: INT })

phase('Setup')
await agent(`Repo ${REPO}: fast-forward master, create branch integration/wave-3 from origin/master and worktree ${INTEG} (git worktree add). mkdir ${REPO}/target-wave-3. Report HEAD. Delete nothing.`, { label: 'setup', phase: 'Setup', model: 'haiku' })

phase('Seams')
const seams = await agent(`${common}
Lead foundation work (you are the lead, Opus) in worktree ${INTEG} (branch integration/wave-3), CARGO_TARGET_DIR=${REPO}/target-wave-3/integ. Implement proposals P-11..P-15 exactly: (P-11) trait AppendStream with as_file_mut(); FileSystem::open_append returns Box<dyn AppendStream>; update io::physical (W2-14), FakeFileSystem and every caller; (P-12) FileSystem::try_get_file_stat with a typed error (denied/unreadable vs missing), get_file_stat unchanged semantics; implement in PhysicalFileSystem and FakeFileSystem (fakes can inject stat faults); (P-13) RecycleBin::try_restore returns io::Result<bool> where the abstraction exists; (P-14) port the C# FaultInjectionFileSystem and hook fakes (see C# tests/ helpers via git show) into crates/pr-test-support; (P-15) sync fixtures operations-legacy.jsonl and journal-older-build/*.jsonl with tools/sync-from-csharp.ps1 (never hand-edit; log in docs/SYNC-LEDGER.md). Keep existing tests green; add focused tests for each seam. Run fmt, clippy -D warnings, cargo test --workspace --locked. Commit in pieces (+ ${CO}). Update contracts only where a signature changed. Report what changed and anything you could not do.`, { label: 'seams', phase: 'Seams', model: 'opus' })

const t1 = await (async () => { phase('Tier1'); return runTier(TIERS[0], 'Tier1') })()
phase('Integrate1'); const i1 = await integ('Integrate1', t1)
phase('Tier2'); const t2 = await runTier(TIERS[1], 'Tier2')
phase('Integrate2'); const i2 = await integ('Integrate2', t2)
phase('Tier3'); const t3 = await runTier(TIERS[2], 'Tier3')
phase('Integrate3'); const i3 = await integ('Integrate3', t3)
phase('Tier4'); const t4 = await runTier(TIERS[3], 'Tier4')
phase('Integrate4'); const i4 = await integ('Integrate4', t4)

phase('Matrix')
const w309 = await agent(`${common}
W3-09 (Opus): integration tests for the fault/crash matrices over the merged Wave 3 code in worktree ${INTEG} (branch integration/wave-3), CARGO_TARGET_DIR=${REPO}/target-wave-3/integ. Port the C# crash/fault matrix tests that cross units (action -> journal -> undo/recovery; FaultInjection at every step; INV-3 journal before effect, INV-4 gate, INV-5/6 undo) into crates/pr-core/tests/file_actions_matrix.rs using FakeFileSystem + the fault fakes. Add contracts/W3-09.md describing it and the PARITY rows. Run the gate. Mutate the write path by hand (drop Prepared write, swap Commit/effect order): the matrix must fail. Commit (+ ${CO}). Report.`, { label: 'w3-09', phase: 'Matrix', model: 'opus' })

phase('Finish')
const fin = await agent(`In worktree ${INTEG} (branch integration/wave-3, repo ${REPO}): update docs/PARITY.md and tests/golden-inputs/README.md for merged W3 units (verdict FAIL or surviving mutants -> Partial with the list), add "Lessons from Wave 3" to docs/MULTI-AGENT-PLAN.md, add SYNC-LEDGER row for C# d92d6aae..origin/master (external editors: ExternalEditor.cs new, AppSettings +18, SettingsNormalizer +4, SettingsStore +6, ShortcutMappings +8, IDialogService +12, languages +34; behaviour yes; Rust status: NOT ported, patch WU after Wave 3, see docs/PROPOSED-DECISIONS.md P-17), append P-17 to docs/PROPOSED-DECISIONS.md ("sync patch for external editors (W1-04, W2-01/02/03, W2-20) done as W2-S1 after Wave 3 merges; goldens regenerated at the new baseline"), update task_on_progress.md with ONLY what was actually run (reports: ${JSON.stringify({ seams: typeof seams === 'string' ? seams.slice(0, 1500) : seams, i1, i2, i3, i4 })}). Run the full gate (CARGO_TARGET_DIR=${REPO}/target-wave-3/integ), commit, git push -u origin integration/wave-3, gh pr create --base master (title "feat: Wave 3 port (W3-01..W3-09)", body: per-unit verdicts, surviving mutants, accepted divergences, what was NOT run; end with "🤖 Generated with [Claude Code](https://claude.com/claude-code)"). Then bind and enable auto-merge is done by the lead. Return the PR URL and gate results.`, { label: 'finish', phase: 'Finish', model: 'sonnet' })
const sum = r => r.map(x => x && { wu: x.p && x.p.wu, status: x.p && x.p.status, verdict: x.v && x.v.verdict })
return { t1: sum(t1), t2: sum(t2), t3: sum(t3), t4: sum(t4), i1, i2, i3, i4, fin }
