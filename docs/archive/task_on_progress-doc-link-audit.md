## Documentation link audit (2026-10-26)
- Default-scope doc-links originally had 13 inherited missing references at baseline `4006635`. R1 pre-pr found two in touched files: corrected ICC path to its existing crate asset and marked the historical rename ref as a Git branch. Then master PR #274 repaired all13 inherited references; integrated `eba7ec3e` and default-scope doc-links now PASS. New task/archive targets exist; [exact evidence](docs/archive/task_on_progress-native-input-fixture.md). Native Rust gates are recorded separately.

