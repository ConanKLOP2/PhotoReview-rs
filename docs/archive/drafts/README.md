# Archive: draft reports

Moved unchanged (git mv) from `docs/drafts/` by the drafts-cleanup lane. Both are one-off review reports written by machine C (cloud, 2026-10-23), not proposals for real files.

| File | Origin | What is pending and where it belongs |
|---|---|---|
| [`w10-review.md`](w10-review.md) | `docs/drafts/w10-review.md`, #193 (DOC-W10-REVIEW) | Factual fixes (its section 3) are already in `contracts/W10-*.md`; open items N-1 and N-2 are recorded as P-50 and P-51 in `docs/PROPOSED-DECISIONS.md`. B-1 (contract baseline header `d92d6aae`), R-1 and R-2 (information) have no other home; nothing is pending unless the owner wants the headers changed. |
| [`ci-draft-check.md`](ci-draft-check.md) | `docs/drafts/ci-draft-check.md`, #195 (DOC-CI-DRAFT-CHECK) | Its findings 1-4 (missing scripts and tests, `throw` placeholder, env-var names, unpinned checkout) are summarised per file in `docs/drafts/README.md`; re-run the checks when the drafts are applied. |
