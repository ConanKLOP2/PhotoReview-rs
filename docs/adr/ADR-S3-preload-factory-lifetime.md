# S3 preload factory lifetime

- **Status:** Accepted and implemented (the user chose the separate S3 repair PR; merged as #225, 25d88a1, 2026-10-24). Not a row of `docs/PROPOSED-DECISIONS.md`; the decision is recorded here.

Date: 2026-10-24

The W8-17e shutdown regression exposed an Arc cycle: SharedServices owns a cached factory, and that factory owns SharedServices. Unlike the C# garbage-collected provider/delegate cycle, this cycle prevents the Rust graph from being released.

The user chose a separate S3 repair PR before continuing W8-17e, rather than extending the W8-17e owned-file scope. Keep the singleton factory identity, capture Weak<Inner>, and upgrade only for the duration of a scheduler construction. An externally retained factory returns RegistryError naming preload_scheduler_factory after the last graph handle is dropped. It cannot extend or resurrect the graph lifetime. Existing settings-at-invocation and scheduler construction behavior stays the same while a graph handle exists.

Oracle: src/PhotoReview.App.Shared/Composition/SharedServiceRegistration.cs at 7eaee2a2, singleton Func registration. This ADR specifies the Rust ownership boundary; it does not claim a new C# test exists. Regression tests cover graph release with and without an external factory handle, singleton identity, and invocation after graph drop. Existing scheduler construction coverage remains required.

Continue W8-17e only after this repair merges into master. Do not weaken or ignore its shutdown regression.