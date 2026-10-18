export const meta = {
  name: 'wave7-port',
  description: 'Port Wave 7 imaging sub-units in tiers A->B->C->D (red tests, implement, verify, repair), integrate per tier, one PR',
  phases: [
    { title: 'Setup' },
    { title: 'TierA' }, { title: 'IntegrateA' },
    { title: 'TierB' }, { title: 'IntegrateB' },
    { title: 'TierC' }, { title: 'IntegrateC' },
    { title: 'Finish' },
  ],
}
const REPO = 'C:/MyProjects/PhotoReview-rs'
const INTEG = REPO + '/.claude/worktrees/integ-w4'
const CS = 'C:/MyProjects/PhotoReview'
const RUST = 'Rust: add $HOME/.cargo/bin (PowerShell $env:USERPROFILE\\.cargo\\bin) to PATH. ALWAYS build with CARGO_PROFILE_DEV_DEBUG=line-tables-only CARGO_INCREMENTAL=0 and your own CARGO_TARGET_DIR; delete that target dir when your unit is done. Run tools/*.ps1 from the PowerShell tool with powershell -NoProfile -ExecutionPolicy Bypass -File.'
const NEVER = 'NEVER touch the real Recycle Bin; use fakes. Never edit the C# repo (git show only). Never leave edits in the main checkout: work ONLY in your own worktree. Benchmark tests must use fakes and temp dirs they clean up; max model is Sonnet (P-34).'
const CO = 'Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>'

const opus = new Set([])
const sonnet = new Set(['W7-01a','W7-02b','W7-03a','W7-03b','W7-03c','W7-03d'])
const implModel = u => opus.has(u) ? 'opus' : sonnet.has(u) ? 'sonnet' : 'haiku'
const verModel = u => 'sonnet'

const TIERS = {
  A: ['W7-01a', 'W7-01b', 'W7-02a', 'W7-02b'],
  B: ['W7-03a', 'W7-03b', 'W7-03c'],
  C: ['W7-03d'],
}
const PORT = { type: 'object', properties: { wu: { type: 'string' }, status: { type: 'string', enum: ['done', 'blocked'] }, branch: { type: 'string' }, commit: { type: 'string' }, red_tests: { type: 'number' }, green_tests: { type: 'number' }, skipped_csharp_tests: { type: 'array', items: { type: 'string' } }, behaviour_doubts: { type: 'array', items: { type: 'string' } }, notes: { type: 'string' } }, required: ['wu', 'status', 'branch'] }
const VER = { type: 'object', properties: { wu: { type: 'string' }, verdict: { type: 'string', enum: ['PASS', 'FAIL'] }, failing_tests: { type: 'array', items: { type: 'string' } }, surviving_mutations: { type: 'array', items: { type: 'string' } }, behaviour_differences: { type: 'array', items: { type: 'string' } }, mutants_tool: { type: 'string' } }, required: ['wu', 'verdict'] }
const INT = { type: 'object', properties: { merged: { type: 'array', items: { type: 'string' } }, not_merged: { type: 'array', items: { type: 'string' } }, fmt: { type: 'string' }, clippy: { type: 'string' }, test: { type: 'string' }, problems: { type: 'array', items: { type: 'string' } } }, required: ['merged', 'not_merged', 'problems'] }

const common = `Repo ${REPO}. ${RUST} ${NEVER} Read AGENTS.md (PR flow, build artifact cleanup, unsafe rules), contracts/README.md (D1-D70, Wave 7 roster), docs/PROPOSED-DECISIONS.md (act on P-nn as written), docs/WAVE-4-PLAN.md, docs/MULTI-AGENT-PLAN.md sections 7, 10-13. C# spec at baseline d92d6aae only via git -C ${CS} show d92d6aae:<path>.`
const redPrompt = u => `${common}
Red tests for ${u} (test porter). Create your worktree: git -C ${REPO} worktree add ${REPO}/.claude/worktrees/${u} -b wu/${u} integration/wave-7 ; CARGO_TARGET_DIR=${REPO}/target-wave-7/${u}. Read contracts/${u}.md. Port EVERY C# test it lists (names mirror C#, comment names the C# test) plus golden-driven tests over tests/golden/<its goldens>.json into the contract's test files. They must compile against the stub and FAIL via todo!(). Do not implement bodies. Documented skips for non-portable tests. Commit "test(${u}): red tests" + ${CO}.`
const impPrompt = (u, extra) => `${common}
Implement ${u}. Worktree ${REPO}/.claude/worktrees/${u} (branch wu/${u}; create from integration/wave-7 if missing), CARGO_TARGET_DIR=${REPO}/target-wave-7/${u}. Read contracts/${u}.md and the committed red tests. Fill only the contract's Owned files. Make every red test green without weakening them; keep stub signatures except where the contract's amendments (contract amendments) say otherwise. Cache and persisted formats are byte-identical to C#. No unwrap/expect on file-system or user-data paths; unsafe only where the contract allows, one SAFETY comment per block. Run cargo fmt --all; cargo clippy -p <crate> --all-targets -- -D warnings; cargo test -p <crate> (and --include-ignored for native tests that only touch their own temp dirs). Update the PARITY rows. Commit "port(${u}): <name>" + ${CO}. Do not merge or push. ${extra || ''}`
const verPrompt = u => `${common}
Adversarial verification of ${u}. Branch wu/${u}, worktree ${REPO}/.claude/worktrees/${u}; CARGO_TARGET_DIR=${REPO}/target-wave-7/${u}-verify (delete it when done). Find behaviour differences against C# d92d6aae (edge cases, error paths, ordering, formats, overflow/rounding, time, INV-1/INV-2/INV-8/INV-12 as relevant); for each suspected difference write a failing Rust test (kept out of the commit); use the .NET oracle for ambiguous semantics, never edit another unit's goldens. Mutation gate: cargo-mutants if installed (cargo install cargo-mutants --locked), else 15+ manual mutations; every non-equivalent mutant on cache write paths must be killed; delete %TEMP%\\cargo-mutants-*.tmp you created. Do not fix the code. PASS only with no real differences and no surviving non-equivalent mutants; name the tool used.`

async function runTier(units, tier) {
  return await pipeline(units,
    u => agent(redPrompt(u), { label: `red:${u}`, phase: tier, model: 'haiku', schema: PORT }),
    (r, u) => (r && r.status === 'done') ? agent(impPrompt(u), { label: `impl:${u}`, phase: tier, model: implModel(u), schema: PORT }) : r,
    (p, u) => (p && p.status === 'done') ? agent(verPrompt(u), { label: `verify:${u}`, phase: tier, model: verModel(u), schema: VER }).then(v => ({ p, v })) : { p, v: null },
    async (r, u) => {
      if (!r || !r.p || r.p.status !== 'done') return r
      let { p, v } = r
      for (let round = 0; round < 2 && v && v.verdict === 'FAIL'; round++) {
        const rep = await agent(impPrompt(u, `REPAIR round ${round + 1}: fix these verifier findings with real fixes plus tests that kill them; never weaken tests. Failing: ${JSON.stringify(v.failing_tests)} Mutants: ${JSON.stringify(v.surviving_mutations)} Differences: ${JSON.stringify(v.behaviour_differences)}`),
          { label: `repair${round + 1}:${u}`, phase: tier, model: implModel(u) === 'haiku' ? 'sonnet' : implModel(u), schema: PORT })
        if (!rep) break
        p = rep
        v = await agent(verPrompt(u), { label: `reverify${round + 1}:${u}`, phase: tier, model: verModel(u), schema: VER })
      }
      return { p, v }
    })
}
const integ = (phaseName, results) => agent(`Integrator, worktree ${INTEG} (branch integration/wave-7), repo ${REPO}. ${RUST} ${NEVER} Merge each wu/<WU> below with git merge --no-ff ("merge(<WU>)" + ${CO}); resolve mod.rs/PARITY.md/Cargo.toml conflicts keeping both sides. Verdicts (FAIL still merges if it builds; list it): ${JSON.stringify(results.filter(Boolean).map(r => ({ wu: r.p && r.p.wu, branch: r.p && r.p.branch, status: r.p && r.p.status, verdict: r.v && r.v.verdict, open: r.v && { f: r.v.failing_tests, m: r.v.surviving_mutations, d: r.v.behaviour_differences } })))}. Then CARGO_TARGET_DIR=${REPO}/target-wave-7/integ: cargo fmt --all; cargo clippy --workspace --all-targets --locked -- -D warnings; cargo test --workspace --locked --no-fail-fast (bounded). Fix only trivial integration breakage. After the merge, delete the merged units' worktrees (git worktree remove) and target-wave-7/<WU>* dirs. Report honestly. Do not push.`, { label: `integrate:${phaseName}`, phase: phaseName, model: 'sonnet', schema: INT })

phase('Setup')
await agent(`Repo ${REPO}: fast-forward master; create branch integration/wave-7 from origin/master and worktree ${INTEG}; mkdir ${REPO}/target-wave-7. Report HEAD. Delete nothing.`, { label: 'setup', phase: 'Setup', model: 'haiku' })

phase('TierA'); const ta = await runTier(TIERS.A, 'TierA')
phase('IntegrateA'); const ia = await integ('IntegrateA', ta)
phase('TierB'); const tb = await runTier(TIERS.B, 'TierB')
phase('IntegrateB'); const ib = await integ('IntegrateB', tb)
phase('TierC'); const tc = await runTier(TIERS.C, 'TierC')
phase('IntegrateC'); const ic = await integ('IntegrateC', tc)

phase('Finish')
const fin = await agent(`In worktree ${INTEG} (branch integration/wave-7, repo ${REPO}). ${RUST} Update docs/PARITY.md and tests/golden-inputs/README.md for merged W7 units (FAIL verdict or surviving mutants -> Partial with the list), add "Lessons from Wave 7" to docs/MULTI-AGENT-PLAN.md, update task_on_progress.md with ONLY what was actually run (reports: ${JSON.stringify({ ia, ib, ic })}). Run the full gate (CARGO_TARGET_DIR=${REPO}/target-wave-7/integ). Merge origin/master into the branch if behind (conflicts: keep both sides). Commit, git push -u origin integration/wave-7, gh pr create --base master (title "feat: Wave 7 port (benchmarking)", body: per-unit verdicts, surviving mutants, accepted divergences, what was NOT run; end with "🤖 Generated with [Claude Code](https://claude.com/claude-code)"). The standing user instruction (run end to end without asking, auto-fix + auto-merge every PR) authorizes the push and PR. Then run gh pr merge --auto --squash. Return the PR URL and gate results.`, { label: 'finish', phase: 'Finish', model: 'sonnet' })
const sum = r => r.map(x => x && { wu: x.p && x.p.wu, status: x.p && x.p.status, verdict: x.v && x.v.verdict })
return { a: sum(ta), b: sum(tb), c: sum(tc), ia, ib, ic, fin }
