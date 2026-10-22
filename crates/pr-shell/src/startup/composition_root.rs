//! Port of `PhotoReview.Shell.Win32/Startup/ShellCompositionRoot.cs` (109 lines, baseline `d92d6aae`): `ShellHostServices` (18), `ShellCompositionRoot` (25-90),
//! `ShellFolderPickerAdapter` (93-96); `ShellNoticeSink` (102-109) is NOT ported (D-W8B6-8). Unit W8-17c. Contract: `contracts/W8-17c.md`.
//!
//! C-16 shell part. The C# builds a DI container (`AddPhotoReviewShared` plus the host services); Rust has none (plan section 09), so the root is a plain function that
//! returns a struct of the host-provided services. The SHARED graph (settings store, journal, view model...) is W8-09b's `composition::registry`; the two meet through
//! [`GraphSlot`] (the C# lazy resolution: the dialog port needs the graph, the graph needs the dialog service).
use std::cell::OnceCell;
use std::rc::Rc;
use std::sync::Arc;

use pr_app::coordinators::traits::IFolderPicker;
use pr_app::services::contracts::ClipboardService;
use pr_core::abstractions::{Log, UiDispatcher, UiScheduler};
use pr_platform_win::dialogs::clipboard::Win32ClipboardService;
use pr_platform_win::dialogs::dialog_service::{
    LazySecondaryPort, SecondaryWindowPort, SecondaryWindowPortHost, Win32DialogService,
};
use pr_platform_win::dialogs::file_dialog::NativeShellFileDialog;
use pr_platform_win::dialogs::file_dialog::ShellFileDialog;
use pr_platform_win::dialogs::folder_dialog::NativeShellFolderDialog;
use pr_platform_win::dialogs::folder_dialog::{FolderPicker, ShellFolderDialog};
use pr_platform_win::dialogs::secondary_window_port::SecondaryWindowDialogs;
use pr_platform_win::dialogs::secondary_window_port::SecondaryWindowRouter;
use pr_platform_win::dialogs::secondary_window_port::{PortOptions, Win32SecondaryWindowPort};
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
        let owner = self.owner_window.clone();
        Box::new(move || owner())
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
        Self { picker, log }
    }
}

impl IFolderPicker for ShellFolderPickerAdapter {
    /// C# `picker.Pick(title, initialFolder)`; `Err(hresult)` -> `log.error("Folder dialog failed", ..)` and `None`.
    fn pick_folder(&mut self, title: &str, initial_folder: Option<&str>) -> Option<String> {
        match self.picker.pick(title, initial_folder) {
            Ok(value) => value,
            Err(error) => {
                self.log
                    .error(&format!("Folder dialog failed (0x{:08X})", error.0 as u32), None);
                None
            }
        }
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
        let ui_scheduler: Arc<dyn UiScheduler> = Arc::new(host.dispatcher.clone());
        let ui_dispatcher: Arc<dyn UiDispatcher> = Arc::new(host.dispatcher.clone());
        let folder_dialog = options
            .folder_dialog
            .unwrap_or_else(|| Rc::new(NativeShellFolderDialog::new()));
        let file_dialog = options
            .file_dialog
            .unwrap_or_else(|| Rc::new(NativeShellFileDialog::new()));
        let clipboard = Rc::new(Win32ClipboardService::new(host.owner_closure()));
        let folder_picker = Rc::new(FolderPicker::new(folder_dialog.clone(), host.owner_closure()));
        let graph_slot: GraphSlot = Rc::new(OnceCell::new());
        let dialogs_override = options.dialogs;
        let port_override = options.secondary_port;
        let seams = options.seams;
        let version = options.version;
        let factory_clipboard: Rc<dyn ClipboardService> = clipboard.clone();
        let factory_file = file_dialog.clone();
        let factory_folder = folder_dialog.clone();
        let factory_log = log.clone();
        let loop_copy = host.message_loop.clone();
        let slot = graph_slot.clone();
        let factory: Box<dyn FnOnce() -> Option<Rc<dyn SecondaryWindowPort>>> = Box::new(move || {
            if let Some(port) = port_override {
                return Some(port);
            }
            let dialogs = if let Some(dialogs) = dialogs_override {
                Some(dialogs)
            } else {
                slot.get().map(|services| {
                    Rc::new(super::secondary_dialogs::ProductionSecondaryWindowDialogs::new(
                        services.clone(),
                        make_host_parts(
                            factory_clipboard.clone(),
                            factory_file.clone(),
                            factory_folder.clone(),
                            factory_log.clone(),
                            version,
                        ),
                        seams,
                    )) as Rc<dyn SecondaryWindowDialogs>
                })
            };
            let Some(dialogs) = dialogs else {
                factory_log.error(
                    "Secondary windows were requested before the service graph was bound",
                    None,
                );
                return None;
            };
            let native = Win32SecondaryWindowPort::new(
                dialogs,
                factory_log,
                PortOptions {
                    ready: None,
                    message_loop: loop_copy,
                    settings_enabled: None,
                },
            );
            Some(Rc::new(SecondaryWindowRouter::new(native)))
        });
        let secondary_port = Rc::new(LazySecondaryPort::new(factory));
        let secondary_port_host = Rc::new(SecondaryWindowPortHost::new(secondary_port.clone()));
        let zoom_prompt = Rc::new(Win32ZoomPromptService::new(
            host.owner_closure(),
            secondary_port.clone(),
        ));
        let dialog_service = Rc::new(
            Win32DialogService::new(
                host.owner_closure(),
                FolderPicker::new(folder_dialog.clone(), host.owner_closure()),
                secondary_port.clone(),
            )
            .with_log(log.clone()),
        );
        let sink_factory = Rc::new(Win32PresentationSinkFactory::new(hub.clone()));
        ShellServices {
            hub,
            ui_scheduler,
            ui_dispatcher,
            clipboard,
            folder_dialog,
            file_dialog,
            folder_picker,
            secondary_port,
            secondary_port_host,
            zoom_prompt,
            dialog_service,
            sink_factory,
            graph_slot,
            log,
        }
    }

    /// A new `IFolderPicker` over the shared `FolderPicker` (`IFolderPicker::pick_folder` takes `&mut self`, so each consumer owns an adapter).
    pub fn new_folder_picker(&self) -> Box<dyn IFolderPicker> {
        Box::new(ShellFolderPickerAdapter::new(
            self.folder_picker.clone(),
            self.log.clone(),
        ))
    }

    /// Binds the graph services the dialogs need. `false` when a graph was bound already (the first wins). Must happen before the first secondary window opens.
    pub fn bind_dialog_services(&self, services: Rc<DialogServices>) -> bool {
        self.graph_slot.set(services).is_ok()
    }

    /// The `DialogHostParts` this root would give the production dialogs (the host half of the factory; exposed for 17e and the tests).
    pub fn dialog_host_parts(&self, version: Option<String>) -> DialogHostParts {
        make_host_parts(
            self.clipboard.clone(),
            self.file_dialog.clone(),
            self.folder_dialog.clone(),
            self.log.clone(),
            version,
        )
    }
}

fn make_host_parts(
    clipboard: Rc<dyn pr_app::services::contracts::ClipboardService>,
    file_dialog: Rc<dyn ShellFileDialog>,
    folder_dialog: Rc<dyn ShellFolderDialog>,
    log: Arc<dyn Log>,
    version: Option<String>,
) -> DialogHostParts {
    use pr_platform_win::shell_startup::locale::{
        WindowsNumberFormatting, WindowsRecoveryTimeFormatter, group_separator, local_long_time_text,
    };
    DialogHostParts {
        clipboard,
        file_dialog,
        folder_dialog,
        log,
        version,
        localizer: Box::new(pr_l10n::localizer::Localizer::current),
        numbers: Rc::new(WindowsNumberFormatting),
        time: Rc::new(WindowsRecoveryTimeFormatter),
        group_separator: group_separator(),
        current_time_text: Arc::new(local_long_time_text),
    }
}
