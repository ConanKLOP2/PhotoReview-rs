# ADR-R-12: blank rename sources return a rejection

Status: accepted under the standing instruction to apply contract recommendations and record the decision. Date: 2026-10-26. Contract: W9-R1, D-W9R-12.

C# at 7eaee2a2 throws ArgumentException for a null or whitespace current path in RenameNamePolicy.Check and ExceedsPathBudget. The Rust UI boundary accepts a non-null &str and must not panic on user data. Its contract recommends an Empty verdict.

Check and CheckUtf16 return RenameNameProblem::Empty, an empty new name, no invalid character and false unchanged/case-only flags for a blank source. ExceedsPathBudget returns true for that invalid source, even with long-path awareness enabled. This prevents a caller that uses the budget helper directly from treating an invalid source as approved. Nonblank paths keep the C# budget rule, including permitting an already-long path to keep its length or shorten.

Alternatives were panicking like an exception (violates user-data error policy), changing the binding to Result (unnecessary downstream API churn), and returning the existing rejection verdict (selected). This is an explicit difference from C#, with no disk access or format change.

The C# Build_NullGroup_Throws case is represented by &CaptureGroup: a null value cannot be passed. No runtime null-parity test is claimed. The Rust blank-source test checks both ordinary and long-path-aware modes; the 44 existing oracle cases cover nonblank source behavior. Full mutation, native and corpus evidence remain separate gates.
