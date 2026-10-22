//! Port of `PhotoReview.Shell.Win32/Startup/ShellCompositionRoot.cs` (109 lines, baseline `d92d6aae`): `ShellHostServices` (18), `ShellCompositionRoot` (25-90),
//! `ShellFolderPickerAdapter` (93-96); `ShellNoticeSink` (102-109) is NOT ported (D-W8B6-8). Unit W8-17c. Contract: `contracts/W8-17c.md`.
//!
//! C-16 shell part. The C# builds a DI container (`AddPhotoReviewShared` plus the host services); Rust has none (plan section 09), so the root is a plain function that
//! returns a struct of the host-provided services. The SHARED graph (settings store, journal, view model...) is W8-09b's `composition::registry`; the two meet through
//! [`GraphSlot`] (the C# lazy resolution: the dialog port needs the graph, the graph needs the dialog service).
#![allow(unused_variables, dead_code)] // stub: the porter removes this line with the last `todo!()`

use std::cell::OnceCell;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::coordinators::traits::IFolderPicker;
use pr_core::abstractions::{Log, UiDispatcher, UiScheduler};
use pr_platform_win::dialogs::clipboard::Win32ClipboardService;
use pr_platform_win::dialogs::dialog_service::{
    LazySecondaryPort, SecondaryWindowPort, SecondaryWindowPortHost, Win32DialogService,
};
use pr_platform_win::dialogs::file_dialog::ShellFileDialog;
use pr_platform_win::dialogs::folder_dialog::{FolderPicker, ShellFolderDialog};
use pr_platform_win::dialogs::secondary_window_port::SecondaryWindowDialogs;
use pr_platform_win::dialogs::zoom_prompt_service::Win32ZoomPromptService;
use pr_platform_win::host::message_loop::MessageLoop;
use pr_platform_win::host::ui_dispatcher::Win32UiDispatcher;
use pr_platform_win::shell_viewing::presentation_sink::{ShellPresentationHub, Win32PresentationSinkFactory};
use pr_platform_win::win32::structs::Hwnd;

use super::secondary_dialogs::{DialogFactorySeams, DialogHostParts, DialogServices};

/// C# `ShellHostServices(Dispatcher, OwnerWindow, Loop)`: what the Win32 host gives the service graph. No hidden defaults.
pub struct ShellHostServices {
    /// C# `Dispatcher` (also the `IUiScheduler` and `IUiDispatcher` of the graph).
    pub dispatcher: Win32UiDispatcher,
    /// C# `OwnerWindow`: the owner HWND of the dialogs, `0` while there is no main window or it is destroyed. Evaluated at USE time, never cached (D-W8B5-1).
    pub owner_window: Rc<dyn Fn() -> Hwnd>,
    /// C# `Loop`: the shell message loop (a modeless Benchmark window and the modal loops run nested in it). `None` = the plain loop of `NativeDialog` (tests).
    pub message_loop: Option<Rc<MessageLoop>>,
}

impl std::fmt::Debug for ShellHostServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellHostServices").finish_non_exhaustive()
    }
}

impl ShellHostServices {
    /// A fresh `Box<dyn Fn() -> Hwnd>` over the same owner provider (one per service, D-W8B5-1).
    pub fn owner_closure(&self) -> Box<dyn Fn() -> Hwnd> {
        todo!()
    }
}

/// The services of the shared graph that the dialogs need, bound by W8-17e AFTER the graph is built (the factory of the lazy port runs on the first dialog, later).
pub type GraphSlot = Rc<OnceCell<Rc<DialogServices>>>;

/// The replaceable parts (the C# `Action<IServiceCollection>? overrides` of the tests and the `ISecondaryWindowPort` registered ahead of time).
#[derive(Default)]
pub struct ShellServicesOptions {
    /// C# "an `ISecondaryWindowPort` registered ahead wins over everything": used as the port instead of the router over the production port.
    pub secondary_port: Option<Rc<dyn SecondaryWindowPort>>,
    /// Replaces [`super::secondary_dialogs::ProductionSecondaryWindowDialogs`] under the real port and router (tests).
    pub dialogs: Option<Rc<dyn SecondaryWindowDialogs>>,
    /// Replaces `NativeShellFolderDialog` (`AddShellOverlay`).
    pub folder_dialog: Option<Rc<dyn ShellFolderDialog>>,
    /// Replaces `NativeShellFileDialog`.
    pub file_dialog: Option<Rc<dyn ShellFileDialog>>,
    /// The seams of the production dialogs.
    pub seams: DialogFactorySeams,
    /// The version text of the Settings window.
    pub version: Option<String>,
}

impl std::fmt::Debug for ShellServicesOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellServicesOptions").finish_non_exhaustive()
    }
}

/// C# `IFolderPicker` over `FolderPicker` (C# `ShellFolderPickerAdapter`, lines 92-96). `Err` of the folder dialog is logged (`"Folder dialog failed"`) and answers
/// `None` (D-W8B5-2, D-W8B5-11 a).
pub struct ShellFolderPickerAdapter {
    picker: Rc<FolderPicker>,
    log: Arc<dyn Log>,
}

impl std::fmt::Debug for ShellFolderPickerAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellFolderPickerAdapter").finish_non_exhaustive()
    }
}

impl ShellFolderPickerAdapter {
    /// C# constructor.
    pub fn new(picker: Rc<FolderPicker>, log: Arc<dyn Log>) -> Self {
        todo!()
    }
}

impl IFolderPicker for ShellFolderPickerAdapter {
    /// C# `picker.Pick(title, initialFolder)`; `Err(hresult)` -> `log.error("Folder dialog failed", ..)` and `None`.
    fn pick_folder(&mut self, title: &str, initial_folder: Option<&str>) -> Option<String> {
        todo!()
    }
}

/// What C# `ShellCompositionRoot.ConfigureServices` registers for the host (its numbered section 1), as fields. The graph services of section 2 are W8-09b.
pub struct ShellServices {
    /// C# `ShellPresentationHub` singleton.
    pub hub: Rc<ShellPresentationHub>,
    /// C# `IUiScheduler` (the dispatcher).
    pub ui_scheduler: Arc<dyn UiScheduler>,
    /// C# `IUiDispatcher` (the same dispatcher).
    pub ui_dispatcher: Arc<dyn UiDispatcher>,
    /// C# `IClipboardService` = `Win32ClipboardService(host.OwnerWindow)`.
    pub clipboard: Rc<Win32ClipboardService>,
    /// C# `IShellFolderDialog` (`AddShellOverlay`: `NativeShellFolderDialog`).
    pub folder_dialog: Rc<dyn ShellFolderDialog>,
    /// C# `IShellFileDialog` = `NativeShellFileDialog`.
    pub file_dialog: Rc<dyn ShellFileDialog>,
    /// C# `FolderPicker` singleton.
    pub folder_picker: Rc<FolderPicker>,
    /// C# `SecondaryWindowPortHost`'s lazy port (one for `Win32DialogService`, `Win32ZoomPromptService` and the host).
    pub secondary_port: Rc<LazySecondaryPort>,
    /// C# `SecondaryWindowPortHost`: `dispose()` closes the open secondary windows (call it on the UI thread before the owner window is destroyed, D-W8B5-11 b).
    pub secondary_port_host: Rc<SecondaryWindowPortHost>,
    /// C# `IZoomPromptService` = `Win32ZoomPromptService(host.OwnerWindow, port)`.
    pub zoom_prompt: Rc<Win32ZoomPromptService>,
    /// C# `IDialogService` = `Win32DialogService(host.OwnerWindow, folderPicker, port, ..)`.
    pub dialog_service: Rc<Win32DialogService>,
    /// C# `AddShellViewing`: `IPresentationSinkFactory` = `Win32PresentationSinkFactory(hub)`.
    pub sink_factory: Rc<Win32PresentationSinkFactory>,
    graph_slot: GraphSlot,
    log: Arc<dyn Log>,
}

impl std::fmt::Debug for ShellServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellServices").finish_non_exhaustive()
    }
}

impl ShellServices {
    /// C# `ShellCompositionRoot.Build(host, hub, overrides)` section 1 and `RegisterFeatures`.
    pub fn build(
        host: ShellHostServices,
        hub: Rc<ShellPresentationHub>,
        log: Arc<dyn Log>,
        options: ShellServicesOptions,
    ) -> ShellServices {
        todo!()
    }

    /// A new `IFolderPicker` over the shared `FolderPicker` (`IFolderPicker::pick_folder` takes `&mut self`, so each consumer owns an adapter).
    pub fn new_folder_picker(&self) -> Box<dyn IFolderPicker> {
        todo!()
    }

    /// Binds the graph services the dialogs need. `false` when a graph was bound already (the first wins). Must happen before the first secondary window opens.
    pub fn bind_dialog_services(&self, services: Rc<DialogServices>) -> bool {
        todo!()
    }

    /// The `DialogHostParts` this root would give the production dialogs (the host half of the factory; exposed for 17e and the tests).
    pub fn dialog_host_parts(&self, version: Option<String>) -> DialogHostParts {
        todo!()
    }
}
