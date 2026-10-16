export const meta = {
  name: 'wave2-missing-contracts',
  description: 'Write the missing Wave 2 contract.md files (and missing oracle/golden) for 13 units, then verify each against the C# source',
  phases: [{ title: 'Write' }, { title: 'Verify' }],
}

const WT = 'C:/MyProjects/PhotoReview-rs/.claude/worktrees/w2-contracts'
const DRAFTS = WT + '/contracts/drafts-w2'
const CS = 'C:/MyProjects/PhotoReview'
const BASE = 'd92d6aae'

const UNITS = ['W2-01','W2-02','W2-03','W2-04','W2-10','W2-11','W2-13','W2-14','W2-15','W2-16','W2-17','W2-18','W2-19','W2-20']
const NEED_ORACLE = new Set(['W2-13','W2-15'])

const SCHEMA = {
  type: 'object',
  properties: {
    unit: { type: 'string' },
    contract_written: { type: 'boolean' },
    oracle_golden_done: { type: 'boolean' },
    porter_model: { type: 'string' },
    depends_on: { type: 'array', items: { type: 'string' } },
    open_questions: { type: 'array', items: { type: 'string' } },
    problems: { type: 'array', items: { type: 'string' } },
  },
  required: ['unit', 'contract_written', 'problems'],
}

const common = `Repo: ${WT} (Rust port; read AGENTS.md, contracts/README.md (lead decisions D1-D8, Wave 1 roster), docs/MULTI-AGENT-PLAN.md §2 and §7 (traps) first).
Work ONLY inside ${DRAFTS}/<unit>/ (never touch other dirs, never touch ${CS}; read C# with: git -C ${CS} show ${BASE}:<path>).
Exemplar of the expected contract format: ${DRAFTS}/W2-05/contract.md and ${DRAFTS}/W2-00/contract.md (header with color/porter/test porter/verifier, C# source, C# tests to port, PARITY rows, Owned files, Public API, Semantics notes, Traps, Depends on, Done when, Golden package, Open questions).
Write in English, concise, facts verified against the C# source at ${BASE}. Do not commit.`

phase('Write')
const results = await pipeline(
  UNITS,
  u => agent(
    `${common}
Unit: ${u}. The folder ${DRAFTS}/${u}/ already holds a stub (.rs / stubs/), possibly oracle/${u}.cs and golden-inputs/golden-reference. ` +
    `Write ${DRAFTS}/${u}/contract.md describing exactly that stub and its golden package: find the C# source it ports from the stub's module doc, read it and its C# tests, and state semantics, traps, dependencies on other W2 units, owned files, porter model (haiku/sonnet/opus, justify), and what "done" means. ` +
    (NEED_ORACLE.has(u)
      ? `This unit also lacks oracle/ and golden-inputs/golden-reference: create oracle/${u}.cs (follow the style of ${DRAFTS}/W2-05/oracle/W2-05.cs and the shared W2Util in ${DRAFTS}/W2-00/oracle/W2-00.cs) and golden-inputs/*.json. Only try to run the oracle if the W2-00 contract's method works quickly (tools/oracle.ps1, dotnet available); if you cannot, say so in problems and leave golden-reference empty.`
      : `Do not rewrite existing oracle/golden files unless you find them wrong; report any defect in problems.`) +
    ` Return the structured summary.`,
    { label: `write:${u}`, phase: 'Write', model: 'sonnet', schema: SCHEMA }
  ),
  (w, u) => agent(
    `${common}
Independent review of ${DRAFTS}/${u}/contract.md. Re-read the C# source it names (git -C ${CS} show ${BASE}:...) and the stub. Check every factual claim, missing behaviors, wrong signatures, wrong dependencies (other W2 units, W1 modules already in ${WT}/crates), and that the stub matches the contract. Fix errors directly in contract.md (small edits). List what you changed and anything unresolved needing a lead decision.`,
    { label: `verify:${u}`, phase: 'Verify', model: 'sonnet', schema: {
      type: 'object',
      properties: { unit: { type: 'string' }, changes: { type: 'array', items: { type: 'string' } }, lead_decisions: { type: 'array', items: { type: 'string' } } },
      required: ['unit', 'changes', 'lead_decisions'] } }
  ).then(v => ({ unit: u, write: w, verify: v }))
)
return results.filter(Boolean)
