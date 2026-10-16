export const meta = {
  name: 'wave2-port',
  description: 'Port all 21 Wave 2 units in 3 dependency tiers (port -> mutation verify -> repair -> integrate), then open one PR',
  phases: [
    { title: 'Setup' },
    { title: 'Tier1', detail: 'W2-00 W2-20 W2-14 W2-15 W2-16 W2-17 W2-18' },
    { title: 'Integrate1' },
    { title: 'Tier2', detail: 'W2-01 W2-04..09 W2-11 W2-12 W2-13 W2-19' },
    { title: 'Integrate2' },
    { title: 'Tier3', detail: 'W2-02 W2-03 W2-10' },
    { title: 'Integrate3' },
    { title: 'Finish' },
  ],
}

const REPO = 'C:/MyProjects/PhotoReview-rs'
const INTEG = REPO + '/.claude/worktrees/integ-w2'
const RUST = 'Rust is at $HOME/.cargo/bin (add to PATH; on Windows PowerShell use $env:USERPROFILE\\.cargo\\bin). Use powershell -ExecutionPolicy Bypass for tools/*.ps1.'

// model per unit (contracts/README.md Wave 2 roster)
const MODEL = {
  'W2-11': 'haiku', 'W2-13': 'opus', 'W2-14': 'opus',
}
const modelFor = u => MODEL[u] || 'sonnet'

const TIERS = [
  ['W2-00', 'W2-20', 'W2-14', 'W2-15', 'W2-16', 'W2-17', 'W2-18'],
  ['W2-01', 'W2-04', 'W2-05', 'W2-06', 'W2-07', 'W2-08', 'W2-09', 'W2-11', 'W2-12', 'W2-13', 'W2-19'],
  ['W2-02', 'W2-03', 'W2-10'],
]

const PORT_REPORT = {
  type: 'object',
  properties: {
    wu: { type: 'string' },
    status: { type: 'string', enum: ['done', 'blocked'] },
    branch: { type: 'string' },
    commit: { type: 'string' },
    files_changed: { type: 'array', items: { type: 'string' } },
    fmt: { type: 'string' }, clippy: { type: 'string' }, test: { type: 'string' },
    behaviour_doubts: { type: 'array', items: { type: 'string' } },
    notes: { type: 'string' },
  },
  required: ['wu', 'status', 'branch'],
}
const VERIFY = {
  type: 'object',
  properties: {
    wu: { type: 'string' },
    verdict: { type: 'string', enum: ['PASS', 'FAIL'] },
    failing_tests: { type: 'array', items: { type: 'string' } },
    surviving_mutations: { type: 'array', items: { type: 'string' } },
    behaviour_differences: { type: 'array', items: { type: 'string' } },
  },
  required: ['wu', 'verdict'],
}
const INTEG_REPORT = {
  type: 'object',
  properties: {
    merged: { type: 'array', items: { type: 'string' } },
    not_merged: { type: 'array', items: { type: 'string' } },
    fmt: { type: 'string' }, clippy: { type: 'string' }, test: { type: 'string' },
    problems: { type: 'array', items: { type: 'string' } },
  },
  required: ['merged', 'not_merged', 'problems'],
}

const portPrompt = (u, extra) => `You are porting ONE unit (${u}) of the C# PhotoReview app to Rust, in repo ${REPO}.
${RUST}
Setup (do first): create your own worktree: git -C ${REPO} worktree add ${REPO}/.claude/worktrees/${u} -b wu/${u} integration/wave-2 ; work ONLY there. Use your own cargo target dir: CARGO_TARGET_DIR=${REPO}/target-wave-2/${u} (never share it). Run cargo with -p <crate> (pr-core or pr-l10n), not --workspace.
Read in order: AGENTS.md, contracts/README.md (lead decisions D1-D20), contracts/${u}.md (your contract: owned files, API, semantics, traps, "Done when"), the stub files it names (already in the tree with todo!() bodies), tests/golden/<your goldens>.json, and the C# source via: git -C ${REPO}/../PhotoReview show d92d6aae:<path> (read only; never edit the C# repo).
Rules: modify ONLY the contract's "Owned files" plus the PARITY.md rows for this unit; never touch Cargo.toml (except where the contract says the lead-owned edit is already done), lib.rs/mod.rs, tests/golden/, assets/. Keep every public signature of the stub. No unsafe except what the contract allows (FFI with SAFETY comment). No unwrap/expect on file-system or user-data paths. C# behaviour wins over intuition, including edge and error cases. Port the C# tests listed in the contract (name mirrors C#, comment names the C# test) and the golden-driven tests. Remove the stub marker/#![allow(unused...)] lines the contract tells you to remove.
If you truly cannot proceed without breaking a rule, write contracts/${u}.BLOCKED.md, commit, and report status=blocked.
Steps: implement; then run until clean: cargo fmt --all ; cargo clippy -p <crate> --all-targets -- -D warnings ; cargo test -p <crate> . Commit on wu/${u} with "port(${u}): <name>" (end message with: Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>). Do NOT merge or push.
${extra || ''}
Final answer: the structured report.`

const verifyPrompt = u => `Adversarially verify ${u}. Rust port is on branch wu/${u} (worktree ${REPO}/.claude/worktrees/${u}); C# source is at baseline d92d6aae (git -C ${REPO}/../PhotoReview show d92d6aae:<path>); contract is contracts/${u}.md. ${RUST}
Use CARGO_TARGET_DIR=${REPO}/target-wave-2/${u}-verify. Find behaviour differences (edge cases, error paths, ordering, path/string comparison, number formatting, overflow, time, .NET quirks listed in the contract). For each suspected difference write a failing Rust test that proves it (keep tests uncommitted in a scratch copy: git stash is forbidden; instead copy the worktree to a temp dir or use a temporary branch you delete). Run 8-10 manual mutations of the owned file (flip a condition, drop a branch, change an ordering); every mutation must make some test fail. Never run code that deletes from the real Recycle Bin. Do not fix the code. Answer PASS only if there are no real differences and no surviving non-equivalent mutants.`

async function runTier(units, tierName) {
  return await pipeline(
    units,
    u => agent(portPrompt(u), { label: `port:${u}`, phase: tierName, model: modelFor(u), schema: PORT_REPORT }),
    (p, u) => (p && p.status === 'done')
      ? agent(verifyPrompt(u), { label: `verify:${u}`, phase: tierName, model: 'sonnet', schema: VERIFY }).then(v => ({ p, v }))
      : { p, v: null },
    async (r, u) => {
      if (!r || !r.p || r.p.status !== 'done') return r
      let { p, v } = r
      for (let round = 0; round < 2 && v && v.verdict === 'FAIL'; round++) {
        const rep = await agent(
          portPrompt(u, `REPAIR round ${round + 1}: the worktree/branch wu/${u} already exists (use it, do not recreate). Fix the following verifier findings with real code fixes and add/keep tests that prove them; do not weaken tests.\nFailing tests: ${JSON.stringify(v.failing_tests)}\nSurviving mutations: ${JSON.stringify(v.surviving_mutations)}\nBehaviour differences: ${JSON.stringify(v.behaviour_differences)}`),
          { label: `repair${round + 1}:${u}`, phase: tierName, model: modelFor(u), schema: PORT_REPORT })
        if (!rep) break
        p = rep
        v = await agent(verifyPrompt(u), { label: `reverify${round + 1}:${u}`, phase: tierName, model: 'sonnet', schema: VERIFY })
      }
      return { p, v }
    }
  )
}

const integratePrompt = (tierName, results) => `You are the integrator for Wave 2 in repo ${REPO}. ${RUST}
Work in worktree ${INTEG} (branch integration/wave-2); do all merging there. For each unit branch wu/<WU> below, merge it (git merge --no-ff, message "merge(<WU>)", co-author line "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"). Units are disjoint by design; resolve any conflict in mod.rs/PARITY.md by keeping both sides. Verdicts (a FAIL verdict still gets merged if the branch builds, but list it in problems):
${JSON.stringify(results.filter(Boolean).map(r => ({ wu: r.p && r.p.wu, branch: r.p && r.p.branch, status: r.p && r.p.status, verdict: r.v && r.v.verdict, open: r.v && { failing: r.v.failing_tests, mutants: r.v.surviving_mutations, diffs: r.v.behaviour_differences } })))}
Skip blocked units (list them in not_merged). Then run, with CARGO_TARGET_DIR=${REPO}/target-wave-2/integ: cargo fmt --all ; cargo clippy --workspace --all-targets --locked -- -D warnings ; cargo test --workspace --locked (use timeouts; never run unbounded). Fix only trivial integration breakage (imports, fmt) and commit it. Report results honestly; do not claim success for steps that failed. Do not push.`

phase('Setup')
await agent(`In repo ${REPO}: ensure master is current (git fetch origin; the main checkout is on master, fast-forward only), create branch integration/wave-2 from origin/master and its worktree at ${INTEG} (git worktree add ${INTEG} -b integration/wave-2 origin/master). Create dir ${REPO}/target-wave-2. Report the HEAD sha. Do not delete anything.`, { label: 'setup', phase: 'Setup', model: 'haiku' })

phase('Tier1')
const t1 = await runTier(TIERS[0], 'Tier1')
phase('Integrate1')
const i1 = await agent(integratePrompt('Tier1', t1), { label: 'integrate:tier1', phase: 'Integrate1', model: 'sonnet', schema: INTEG_REPORT })

phase('Tier2')
const t2 = await runTier(TIERS[1], 'Tier2')
phase('Integrate2')
const i2 = await agent(integratePrompt('Tier2', t2), { label: 'integrate:tier2', phase: 'Integrate2', model: 'sonnet', schema: INTEG_REPORT })

phase('Tier3')
const t3 = await runTier(TIERS[2], 'Tier3')
phase('Integrate3')
const i3 = await agent(integratePrompt('Tier3', t3), { label: 'integrate:tier3', phase: 'Integrate3', model: 'sonnet', schema: INTEG_REPORT })

phase('Finish')
const fin = await agent(`In worktree ${INTEG} (branch integration/wave-2, repo ${REPO}): update docs/PARITY.md rows and tests/golden-inputs/README.md rows for every W2 unit that merged (status + proving test; units with open FAIL verdict stay 'Partial'), add the new "Lessons from Wave 2" bullets to docs/MULTI-AGENT-PLAN.md section 10 based on these reports: ${JSON.stringify({ i1, i2, i3 })}, and update task_on_progress.md (state, test results as actually run, next steps; no unverified claims). Run cargo fmt/clippy/test as in the gate (CARGO_TARGET_DIR=${REPO}/target-wave-2/integ), commit, push the branch (git push -u origin integration/wave-2) and open ONE PR to master with gh (title "feat: Wave 2 port (W2-00..W2-20)"; body: per-unit verdicts, accepted divergences, open issues; end with the line "🤖 Generated with [Claude Code](https://claude.com/claude-code)"). Return the PR URL and gate results.`, { label: 'finish', phase: 'Finish', model: 'sonnet' })

return { tier1: t1.map(r => r && { wu: r.p && r.p.wu, status: r.p && r.p.status, verdict: r.v && r.v.verdict }), tier2: t2.map(r => r && { wu: r.p && r.p.wu, status: r.p && r.p.status, verdict: r.v && r.v.verdict }), tier3: t3.map(r => r && { wu: r.p && r.p.wu, status: r.p && r.p.status, verdict: r.v && r.v.verdict }), i1, i2, i3, fin }
