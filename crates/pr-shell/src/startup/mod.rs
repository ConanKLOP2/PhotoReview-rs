//! Wave 8 batch B6 composition and start-up of the shell binary (lead-owned index, contract writer of 17a..17c).
//!
//! `composition_root` and `secondary_dialogs` are W8-17c (`contracts/W8-17c.md`, with the production `SecondaryWindowDialogs` of P-43). They live in this crate
//! because they name both `pr-app` and `pr-platform-win` types (D-W8B1-1, D-W8B6-1). W8-17d/17e append their modules here (re-read this file before editing it).

pub mod composition_root;
pub mod secondary_dialogs;

// W8-17d (`contracts/W8-17d.md`): options, the service graph seam, the single-instance gate, the first half of `StartupCoreAsync`, the shared state.
pub mod error;
pub mod graph;
pub mod instance;
pub mod options;
pub mod phase_one;
pub mod state;

// W8-17e (`contracts/W8-17e.md`): the main window, the forwarded-open target, the overlay view model, journal recovery, smoke, shutdown, and the entry point `ShellStartup::run`.
pub mod forward_target;
pub mod overlay_vm;
pub mod recovery;
pub mod run;
pub mod shutdown;
pub mod smoke;
pub mod window;
