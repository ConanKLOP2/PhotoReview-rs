export const meta = {
  name: 'wave3-contracts',
  description: 'Write Wave 3 (data-safety) contracts, stubs, oracle commands and goldens for W3-01..W3-08, verify, wire into the crates and open one PR',
  phases: [{ title: 'Setup' }, { title: 'Write' }, { title: 'Verify' }, { title: 'Wire' }],
}
const REPO = 'C:/MyProjects/PhotoReview-rs'
const WT = REPO + '/.claude/worktrees/w3-contracts'
const CS = 'C:/MyProjects/PhotoReview'
const BASE = 'd92d6aae'
const RUST = 'Rust: add $HOME/.cargo/bin (PowerShell: $env:USERPROFILE\\.cargo\\bin) to PATH. Run tools/*.ps1 from the PowerShell tool with powershell -NoProfile -ExecutionPolicy Bypass -File (the Bash tar breaks oracle.ps1).'

const UNITS = [
  { id: 'W3-01', src: 'JournalTransaction.cs, OperationJournal.cs (+ JournalErrors.cs, JournalGroupMember.cs, JournalJsonContext.cs, JournalLineParser.cs if used)', rust: 'crates/pr-core/src/file_actions/journal.rs and transaction.rs', model: 'opus' },
  { id: 'W3-02', src: 'JournalCompactionPlan.cs and PhysicalJournalCompactionFiles (find it)', rust: 'crates/pr-core/src/file_actions/compaction.rs', model: 'opus' },
  { id: 'W3-03', src: 'JournalStartupRecovery.cs (ADR 0003 in the C# docs/adr, including the "older builds" rule)', rust: 'crates/pr-core/src/file_actions/startup_recovery.rs', model: 'opus' },
  { id: 'W3-04', src: 'FileActionService.cs, FileActionRequest.cs, FileActionResult.cs, CaptureGroupAction*.cs, FileFingerprint.cs, RecycleBinCapacity.cs (INV-3, INV-4)', rust: 'crates/pr-core/src/file_actions/service.rs', model: 'opus' },
  { id: 'W3-05', src: 'UndoService.cs, UndoResult.cs (INV-5, INV-6)', rust: 'crates/pr-core/src/file_actions/undo.rs', model: 'opus' },
  { id: 'W3-06', src: 'RecoveryRetryService.cs', rust: 'crates/pr-core/src/file_actions/recovery_retry.rs', model: 'opus' },
  { id: 'W3-07', src: 'RecoveryFileCheck.cs (the verdict table in the C# docs/architecture.md)', rust: 'crates/pr-core/src/file_actions/recovery_check.rs', model: 'sonnet' },
  { id: 'W3-08', src: 'PartialDestinationCleanup.cs', rust: 'crates/pr-core/src/file_actions/partial_cleanup.rs', model: 'sonnet' },
]

const WRITE = { type: 'object', properties: { unit: { type: 'string' }, contract: { type: 'boolean' }, stub_files: { type: 'array', items: { type: 'string' } }, oracle_and_golden: { type: 'boolean' }, depends_on: { type: 'array', items: { type: 'string' } }, open_questions: { type: 'array', items: { type: 'string' } }, problems: { type: 'array', items: { type: 'string' } } }, required: ['unit', 'contract', 'problems'] }
const VER = { type: 'object', properties: { unit: { type: 'string' }, changes: { type: 'array', items: { type: 'string' } }, lead_decisions: { type: 'array', items: { type: 'string' } } }, required: ['unit', 'changes', 'lead_decisions'] }

phase('Setup')
await agent(`In repo ${REPO} (main checkout is on master; fetch and fast-forward): create worktree ${WT} on new branch wave3/contracts from origin/master (git worktree add). Do not delete anything. Report HEAD.`, { label: 'setup', phase: 'Setup', model: 'haiku' })

const common = `Repo worktree: ${WT} (Rust port of the C# PhotoReview; read AGENTS.md, contracts/README.md (lead decisions D1-D20, Wave 1+2 rosters), docs/MULTI-AGENT-PLAN.md sections 2, 7, 10, 11, docs/PARITY.md, and ONE finished exemplar: contracts/W2-05.md plus its stub, oracle (tools/oracle-cs/Commands/W2-05.cs) and golden input/output). C# is the spec at baseline ${BASE}; read it only via git -C ${CS} show ${BASE}:<path> (never edit or check out in the C# repo). Existing Rust you must build on: crates/pr-core/src/{file_actions,io,abstractions,catalog,text,time.rs,sync.rs}, crates/pr-test-support.
Write ONLY inside ${WT}/contracts/drafts-w3/<unit>/ . Output per unit: contract.md (same sections as W2 contracts: header with colour/porter/test porter/verifier, C# source with line counts, C# tests to port (test-first: list every C# test method with file and name), PARITY rows, Owned files, Public API, semantics notes, traps incl. INV ids, Depends on, Done when, golden package, open questions), the Rust stub file(s) with todo!() bodies that compile clippy-clean with the full real signatures and types (data types complete), and where behaviour can be differentially tested: oracle/<unit>.cs in the style of W2 oracles plus golden-inputs/*.json (do NOT run the oracle; the lead wiring step does). Journal line format and every on-disk format must stay byte-identical to C# (AGENTS.md). Use the real FileSystem/abstractions traits already in pr-core; never invent I/O. Never run code that deletes from the real Recycle Bin. Do not commit.`

phase('Write')
const out = await pipeline(
  UNITS,
  u => agent(`${common}\nUnit ${u.id}. C# source: ${u.src}. Intended Rust location: ${u.rust} (adjust if a better split is natural; state the final paths). Write the full package.`,
    { label: `write:${u.id}`, phase: 'Write', model: u.model === 'opus' ? 'opus' : 'sonnet', schema: WRITE }),
  (w, u) => agent(`${common}\nIndependent review of contracts/drafts-w3/${u.id}/ (contract.md, stubs, oracle). Re-read the C# source and its tests: check every claim, signature, missing behaviour, invariant (INV-3..INV-6, INV-8, INV-11 as relevant), the list of C# tests (is every test method present?), dependencies on other W3/W2 units, and that stubs compile logically against existing Rust types. Fix errors directly in the files; keep stub signatures stable unless wrong. Return what you changed and what the lead must decide.`,
    { label: `verify:${u.id}`, phase: 'Verify', model: u.model === 'opus' ? 'opus' : 'sonnet', schema: VER }).then(v => ({ unit: u.id, write: w, verify: v }))
)

phase('Wire')
const wire = await agent(`Lead wiring for Wave 3 contracts in ${WT} (branch wave3/contracts). ${RUST}
Do exactly what the Wave 2 wiring did (git show 07d2eec 50af884 and the PR #7 diff for how): (1) move contracts/drafts-w3/<unit>/contract.md to contracts/<unit>.md; (2) copy stubs to their crate paths and add the pub mod lines in alphabetical order (file_actions/mod.rs etc.); (3) copy oracle/*.cs to tools/oracle-cs/Commands/ and add dispatch lines to tools/oracle-cs/Program.cs; copy golden-inputs/*.json to tests/golden-inputs/; (4) run tools/oracle.ps1 -Commit ${BASE} once per new command (one -Command value per call), keep every golden deterministic (no cwd, clock or file-time dependent output; run twice and compare), fix oracle/input defects you find; do NOT commit changes to pre-existing goldens (git checkout them if only EOL churn); (5) cargo fmt --all, cargo clippy --workspace --all-targets --locked -- -D warnings, cargo test --workspace --locked must be clean (stubs are todo!(), so no new tests yet); (6) add Wave 3 roster + lead decisions D21+ to contracts/README.md (use the verify reports below; porter model per unit, dependency order W3-01 -> 02/03 -> 04 -> 05/06 -> 07/08, test-first rule, Opus verifier + cargo-mutants gate from docs/MULTI-AGENT-PLAN.md Wave 3), PARITY rows for the units, update task_on_progress.md with what was actually run; (7) rm -rf contracts/drafts-w3; commit ("feat: Wave 3 contracts, stubs and goldens (W3-01..W3-08)" + the co-author line "Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>"), push -u origin wave3/contracts, gh pr create to master (body: what was checked, what was NOT run; end with the line "🤖 Generated with [Claude Code](https://claude.com/claude-code)"). Report honestly any step that failed.
Writer/verify reports: ${JSON.stringify(out.filter(Boolean))}`,
  { label: 'wire', phase: 'Wire', model: 'sonnet' })
return { out: out.filter(Boolean).map(o => ({ unit: o.unit, problems: o.write && o.write.problems, decisions: o.verify && o.verify.lead_decisions })), wire }
