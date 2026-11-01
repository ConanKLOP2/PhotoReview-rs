//! BACKLOG G1: what the main window records while a modal command holds the view model.
//!
//! A command that runs a modal dialog (Settings, the folder picker) keeps `&mut MainViewModel` for the whole nested message loop. In C# the dispatcher
//! simply re-enters (`Window.Close()`, paint, size and timer messages run on the same thread against the same object); a `RefCell` cannot allow that, so
//! the re-entrant paths used to either drop the request silently (`try_borrow_mut`) or panic (`borrow`/`borrow_mut`). Now they RECORD the intent here and
//! [`super::core::MainWindowInner::replay_modal_intents`] runs it once, when the borrow has ended.
//!
//! These are small `Cell`s outside the big `RefCell` on purpose (BACKLOG G1 step 1): the window procedure can always read and write them.

use std::cell::Cell;

/// The intents recorded while the view model is busy.
#[derive(Debug, Default)]
pub(crate) struct ModalIntents {
    /// `Closing` arrived while the view model was borrowed: the close was cancelled and must be requested again after the modal command.
    close_requested: Cell<bool>,
    /// `Closed` arrived while the view model was borrowed: `vm.close_session()` (and stopping the preload) is still to be done.
    closed: Cell<bool>,
    /// A command (input, menu, feature) was refused because the view model was busy: how many, so the loss is visible (log + tests).
    dropped_commands: Cell<u32>,
}

impl ModalIntents {
    pub(crate) fn request_close(&self) {
        self.close_requested.set(true);
    }

    pub(crate) fn take_close_request(&self) -> bool {
        self.close_requested.take()
    }

    pub(crate) fn note_closed(&self) {
        self.closed.set(true);
    }

    pub(crate) fn take_closed(&self) -> bool {
        self.closed.take()
    }

    pub(crate) fn note_dropped_command(&self) {
        self.dropped_commands.set(self.dropped_commands.get().saturating_add(1));
    }

    pub(crate) fn dropped_commands(&self) -> u32 {
        self.dropped_commands.get()
    }
}

#[cfg(test)]
mod tests {
    use super::ModalIntents;

    #[test]
    fn intents_are_taken_once() {
        let intents = ModalIntents::default();
        intents.request_close();
        intents.note_closed();
        assert!(intents.take_close_request());
        assert!(!intents.take_close_request());
        assert!(intents.take_closed());
        assert!(!intents.take_closed());
    }

    #[test]
    fn dropped_commands_are_counted() {
        let intents = ModalIntents::default();
        intents.note_dropped_command();
        intents.note_dropped_command();
        assert_eq!(intents.dropped_commands(), 2);
    }
}
