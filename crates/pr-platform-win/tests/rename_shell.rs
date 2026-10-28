//! W9-R8 (FEAT-RENAME shell side): the native rename box, its routing through the secondary-window port and the dialog service, and the menu and settings deltas.
//! C# baseline `7eaee2a2`. Contract: `contracts/W9-R8.md`. Pure tests run everywhere; `#[ignore = "native: ..."]` tests create test-owned windows on a desktop.
#[path = "common/dialogs_dialog_fixture.rs"]
mod fixture;

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::rc::Rc;
use std::sync::Arc;

use fixture::{command, create_owner, destroy_owner, item, on_sta, set_text, show_on_owner};
use pr_app::input::primitives::PointD;
use pr_app::input::review_command::{ReviewCommand, ReviewCommandType};
use pr_app::menus::model::MenuItemModel;
use pr_core::abstractions::{
    BatchReviewItem, DialogService, NullLog, RenamePromptRequest, SettingsTarget, SkippedEntry,
};
use pr_core::settings::app_settings::AppSettings;
use pr_core::settings::context_menu::{ContextMenuItemId, ITEMS};
use pr_core::settings::shortcuts::ReviewAction;
use pr_platform_win::dialogs::dialog_service::{
    LazySecondaryPort, SecondaryWindowKind, SecondaryWindowPort, Win32DialogService,
};
use pr_platform_win::dialogs::folder_dialog::{FolderPicker, ShellFolderDialog};
use pr_platform_win::dialogs::native::dialog::{ID_CANCEL, ID_OK, NativeDialog};
use pr_platform_win::dialogs::rename_prompt::{self as rename, RenamePromptDialog};
use pr_platform_win::dialogs::secondary_window_port::{
    ActionProfilesOpener, PortOptions, PreparedDialog, SecondaryWindowDialogs, SecondaryWindowRouter,
    Win32SecondaryWindowPort,
};
use pr_platform_win::dialogs::task_dialog::{TaskDialogKind, TaskDialogPort};
use pr_platform_win::dialogs::zoom_prompt::{DialogMeasure, Placement};
use pr_platform_win::shell_menus::controller::{
    ActionResult, ContextMenuActions, ContextMenuController, PopupMenuHost,
};
use pr_platform_win::win32::dialog_native;
use pr_platform_win::win32::structs::{Hresult, Hwnd};

// ---- pure: items and layout -------------------------------------------------------------------------------------------------------------------

struct Measure;
impl DialogMeasure for Measure {
    fn dx(&self, n: i32) -> i32 {
        n * 2
    }
    fn dy(&self, n: i32) -> i32 {
        n * 3
    }
    fn line_height(&self) -> i32 {
        20
    }
    fn measure_item_height(&self, id: i32, width: i32) -> i32 {
        assert!(width > 0);
        match id {
            rename::LABEL_ID => 17,
            rename::ERROR_ID => 40,
            other => panic!("unexpected control {other}"),
        }
    }
    fn measure_text_width(&self, text: &str) -> i32 {
        i32::try_from(text.len()).unwrap_or(0) * 10
    }
    fn measure_text_height(&self, _: &str, _: i32) -> i32 {
        panic!("not used by this layout")
    }
    fn item_text(&self, _: i32) -> String {
        panic!("not used by this layout")
    }
    fn button_width(&self, _: &[i32]) -> i32 {
        panic!("not used by this layout")
    }
    fn button_row_width(&self, ids: &[i32]) -> i32 {
        assert_eq!(ids, [rename::CANCEL_ID, rename::RENAME_ID]);
        100
    }
}

// C#: RenamePromptDialog constants and Items (ids, order, texts, tab stops, default button, hidden error line).
#[test]
fn rename_items_keep_ids_order_texts_default_button_and_hidden_error_line() {
    assert_eq!(
        (
            RenamePromptDialog::LABEL_ID,
            RenamePromptDialog::EDIT_ID,
            RenamePromptDialog::EXTENSION_ID
        ),
        (100, 101, 102)
    );
    assert_eq!(
        (
            RenamePromptDialog::ERROR_ID,
            RenamePromptDialog::CANCEL_ID,
            RenamePromptDialog::RENAME_ID
        ),
        (103, 2, 1)
    );
    let items = rename::build_items("New name:", ".jpg", "Cancel", "Rename");
    assert_eq!(
        items.iter().map(|item| item.id).collect::<Vec<_>>(),
        [100, 101, 102, 103, 2, 1]
    );
    assert_eq!(
        items.iter().map(|item| item.text.as_str()).collect::<Vec<_>>(),
        ["New name:", "", ".jpg", "", "Cancel", "Rename"]
    );
    assert_eq!(items[1].ex_style, 0x200, "WS_EX_CLIENTEDGE on the box");
    let visible = 0x1000_0000;
    assert_eq!(items[3].style & visible, 0, "the error line starts hidden");
    assert_ne!(items[2].style & visible, 0, "the extension is visible");
    assert_eq!(items[5].style & 0xf, 1, "BS_DEFPUSHBUTTON on Rename");
    assert_eq!(items[4].style & 0xf, 0, "BS_PUSHBUTTON on Cancel");
}

// C#: RenamePromptDialog.PreferredClientSize - the error line only grows the window while it is visible.
#[test]
fn preferred_size_grows_by_gap_plus_error_height_only_while_the_error_shows() {
    // width = max(dx(260) = 520, 100 + 48) = 520; height = 30 + 17 + 12 + 42 + 30 + 42 + 30.
    assert_eq!(rename::preferred_client_size(&Measure, false), (520, 203));
    assert_eq!(rename::preferred_client_size(&Measure, true), (520, 203 + 12 + 40));
}

// C#: RenamePromptDialog.LayoutControls - the extension sits right of the box, vertically centred on a one-row cell.
#[test]
fn layout_places_the_box_left_of_the_fixed_extension_and_centres_it_vertically() {
    let plan = rename::layout_plan(&Measure, ".jpg", 520, false);
    // content = 520 - 48 = 472; extension = 40 + dx(4) = 48; gap = dx(3) = 6; box = 472 - 48 - 6 = 418; text row = min(42, 20 + 6) = 26.
    assert_eq!(
        plan.placements,
        vec![
            Placement::new(rename::LABEL_ID, 24, 30, 472, 17),
            Placement::new(rename::EDIT_ID, 24, 59, 418, 42),
            Placement::new(rename::EXTENSION_ID, 24 + 472 - 48, 59 + 8, 48, 26),
        ]
    );
    assert_eq!(plan.button_row.top, 101 + 30);
    assert_eq!(plan.button_row.right_margin, 24);
    assert_eq!(plan.button_row.ids, [rename::CANCEL_ID, rename::RENAME_ID]);
}

// C#: RenamePromptDialog.LayoutControls - no extension: the box takes the full width and the extension label is parked at 0 x 0.
#[test]
fn layout_without_extension_gives_the_box_the_full_width_and_parks_the_label() {
    let plan = rename::layout_plan(&Measure, "", 520, false);
    assert_eq!(plan.placements[1], Placement::new(rename::EDIT_ID, 24, 59, 472, 42));
    assert_eq!(plan.placements[2], Placement::new(rename::EXTENSION_ID, 24, 59, 0, 0));
}

// C#: RenamePromptDialog.LayoutControls - the error line sits dy(4) under the box and pushes the buttons down by its height.
#[test]
fn layout_with_an_error_places_the_line_under_the_box_and_moves_the_buttons() {
    let plain = rename::layout_plan(&Measure, ".jpg", 520, false);
    let shown = rename::layout_plan(&Measure, ".jpg", 520, true);
    assert_eq!(
        shown.placements[3],
        Placement::new(rename::ERROR_ID, 24, 59 + 42 + 12, 472, 40)
    );
    assert_eq!(shown.button_row.top, plain.button_row.top + 12 + 40);
}

// ---- pure: routing through the port and the service --------------------------------------------------------------------------------------------

#[derive(Default)]
struct SpyPort {
    answer: RefCell<Option<String>>,
    supports_rename: Cell<bool>,
    calls: RefCell<Vec<(Hwnd, String, String)>>,
}
impl SecondaryWindowPort for SpyPort {
    fn supports(&self, kind: SecondaryWindowKind) -> bool {
        kind != SecondaryWindowKind::RenamePrompt || self.supports_rename.get()
    }
    fn show_settings(&self, _: Hwnd, _: SettingsTarget) -> bool {
        false
    }
    fn show_recovery(&self, _: Hwnd) {}
    fn show_diagnostics(&self, _: Hwnd) {}
    fn show_benchmark(&self, _: Hwnd, _: Option<&std::path::Path>) {}
    fn show_skipped_files(&self, _: Hwnd, _: &[SkippedEntry]) {}
    fn show_batch_review(&self, _: Hwnd, _: &[BatchReviewItem]) -> bool {
        false
    }
    fn prompt_custom_zoom(&self, _: Hwnd, _: i32) -> Option<i32> {
        None
    }
    fn prompt_rename(&self, owner: Hwnd, request: &RenamePromptRequest) -> Option<String> {
        self.calls
            .borrow_mut()
            .push((owner, request.initial_name.clone(), request.extension.clone()));
        self.answer.borrow().clone()
    }
}

fn request(initial: &str, extension: &str) -> RenamePromptRequest {
    #[allow(clippy::arc_with_non_send_sync)] // the request type is Arc-based; the tests use it on one thread
    let validate: pr_core::abstractions::RenameValidateFn = Arc::new(|_, _| None);
    RenamePromptRequest {
        initial_name: initial.to_owned(),
        extension: extension.to_owned(),
        validate,
    }
}

// C#: SecondaryWindowRoutingTests.WindowsTheWin32PortSupports_GoToIt_AndNeverTouchTheFallback (the RenamePrompt kind of FEAT-RENAME).
#[test]
fn windows_the_win32_port_supports_go_to_it_and_never_touch_the_fallback() {
    let native = Rc::new(SpyPort::default());
    native.supports_rename.set(true);
    *native.answer.borrow_mut() = Some("Holiday".to_owned());
    let router = SecondaryWindowRouter::new(native.clone());
    assert!(router.supports(SecondaryWindowKind::RenamePrompt));
    assert_eq!(
        router.prompt_rename(7, &request("IMG_0001", ".jpg")).as_deref(),
        Some("Holiday")
    );
    assert_eq!(
        *native.calls.borrow(),
        [(7, "IMG_0001".to_owned(), ".jpg".to_owned())],
        "the owner and the request reach the native port"
    );
}

// C#: SecondaryWindowRouter.PromptRename => Pick(RenamePrompt) is { } port ? port.PromptRename(...) : null.
#[test]
fn router_answers_none_without_asking_when_the_native_port_lacks_the_rename_box() {
    let native = Rc::new(SpyPort::default());
    *native.answer.borrow_mut() = Some("Holiday".to_owned());
    let router = SecondaryWindowRouter::new(native.clone());
    assert_eq!(router.prompt_rename(7, &request("a", ".jpg")), None);
    assert!(native.calls.borrow().is_empty());
}

struct NoDialogs;
impl SecondaryWindowDialogs for NoDialogs {
    fn settings(&self, _: SettingsTarget, _: ActionProfilesOpener) -> Option<PreparedDialog<bool>> {
        None
    }
    fn recovery(&self, _: Hwnd) -> Option<PreparedDialog<()>> {
        None
    }
    fn benchmark(&self, _: Option<&std::path::Path>) -> Option<NativeDialog> {
        None
    }
    fn action_profiles(&self, _: &[ReviewAction]) -> Option<PreparedDialog<Option<Vec<ReviewAction>>>> {
        None
    }
    fn diagnostics(&self) -> Option<PreparedDialog<()>> {
        None
    }
    fn skipped_files(&self, _: &[SkippedEntry]) -> Option<PreparedDialog<()>> {
        None
    }
    fn batch_review(&self, _: &[BatchReviewItem]) -> Option<PreparedDialog<bool>> {
        None
    }
    fn custom_zoom(&self, _: i32) -> Option<PreparedDialog<Option<i32>>> {
        None
    }
    fn rename_prompt(&self, _: &RenamePromptRequest) -> Option<PreparedDialog<Option<String>>> {
        None
    }
}

// C#: Win32SecondaryWindowPort.Supports lists RenamePrompt; a disposed port answers null (Run returns IdCancel).
#[test]
fn win32_port_supports_the_rename_box_and_a_disposed_port_or_missing_dialog_answers_none() {
    let port = Win32SecondaryWindowPort::new(Rc::new(NoDialogs), Arc::new(NullLog), PortOptions::default());
    assert!(port.supports(SecondaryWindowKind::RenamePrompt));
    assert_eq!(
        port.prompt_rename(0, &request("a", ".jpg")),
        None,
        "no dialog was prepared"
    );
    port.dispose();
    assert_eq!(port.prompt_rename(0, &request("a", ".jpg")), None);
    assert_eq!(port.open_count(), 0);
}

struct FakeTasks;
impl TaskDialogPort for FakeTasks {
    fn show(&self, _: Hwnd, _: &str, _: &str, _: TaskDialogKind) -> i32 {
        0
    }
}
struct FakeFolder;
impl ShellFolderDialog for FakeFolder {
    fn show(&self, _: Hwnd, _: &str, _: Option<&str>) -> Result<Option<String>, Hresult> {
        Ok(None)
    }
}

fn service(secondary: Rc<LazySecondaryPort>) -> Win32DialogService {
    Win32DialogService::with_task_dialogs(
        Box::new(|| 31),
        Box::new(FakeTasks),
        FolderPicker::new(Rc::new(FakeFolder), Box::new(|| 31)),
        secondary,
    )
}

// C#: Win32DialogService.PromptRename(request) => Port()?.PromptRename(_ownerProvider(), request).
#[test]
fn dialog_service_forwards_the_rename_prompt_with_the_owner_and_answers_none_without_a_port() {
    let native = Rc::new(SpyPort::default());
    *native.answer.borrow_mut() = Some("Trip".to_owned());
    let with_port = service(Rc::new(LazySecondaryPort::from_value(Some(native.clone()))));
    assert_eq!(
        with_port.prompt_rename(&request("DSC_1", ".nef")).as_deref(),
        Some("Trip")
    );
    assert_eq!(*native.calls.borrow(), [(31, "DSC_1".to_owned(), ".nef".to_owned())]);
    let without_port = service(Rc::new(LazySecondaryPort::from_value(None)));
    assert_eq!(without_port.prompt_rename(&request("DSC_1", ".nef")), None);
}

// ---- pure: menu, shortcuts, mouse deltas ------------------------------------------------------------------------------------------------------

struct Actions {
    log: RefCell<Vec<String>>,
}
impl ContextMenuActions for Actions {
    fn settings(&self) -> Arc<AppSettings> {
        Arc::new(AppSettings::default())
    }
    fn has_image(&self) -> bool {
        true
    }
    fn can_copy_path(&self) -> bool {
        true
    }
    fn can_undo(&self) -> bool {
        false
    }
    fn can_open_in_external_editor(&self) -> bool {
        false
    }
    fn is_compare_visible(&self) -> bool {
        false
    }
    fn effective_zoom(&self) -> Option<f64> {
        None
    }
    fn execute(&self, command: ReviewCommand) -> ActionResult {
        self.log.borrow_mut().push(format!("Execute:{:?}", command.kind));
        Ok(())
    }
    fn fit(&self) -> ActionResult {
        Ok(())
    }
    fn zoom_to_configured_level(&self) -> ActionResult {
        Ok(())
    }
    fn apply_zoom_preset(&self, _: i32) -> ActionResult {
        Ok(())
    }
    fn custom_zoom(&self) -> ActionResult {
        Ok(())
    }
    fn set_zoom_also_sets_click_level(&self, _: bool) {}
    fn set_click_zoom_percent(&self, _: i32) {}
    fn open_in_external_editor(&self) -> ActionResult {
        Ok(())
    }
    fn copy_file_name(&self) -> ActionResult {
        Ok(())
    }
    fn copy_full_pathname(&self) -> ActionResult {
        Ok(())
    }
    fn show_settings(&self) -> ActionResult {
        Ok(())
    }
    fn report_action_failed(&self, _: &str, _: &dyn Error) {}
    fn zoom_action_name(&self) -> String {
        "Zoom".to_owned()
    }
}
struct NoHost;
impl PopupMenuHost for NoHost {
    fn show(&self, _: &[MenuItemModel], _: PointD) -> Result<Option<String>, Box<dyn Error>> {
        Ok(None)
    }
}

// C#: ContextMenuControllerTests (InlineData "RenameFile" -> "Execute:RenameFile"): the menu id runs the same command as F2.
#[test]
fn execute_the_rename_file_menu_id_runs_the_rename_file_review_command() {
    let actions = Rc::new(Actions {
        log: RefCell::new(Vec::new()),
    });
    let controller = ContextMenuController::new(actions.clone(), Rc::new(NoHost), None);
    assert!(controller.execute("RenameFile").is_ok());
    assert_eq!(*actions.log.borrow(), ["Execute:RenameFile"]);
    assert_eq!(ReviewCommand::RENAME_FILE.kind, ReviewCommandType::RenameFile);
}

// C#: MouseSettingsPageTests.ContextMenu_HasOneTickBoxPerItem_InMenuOrder_WithTheItemsLabel (the count: "FEAT-RENAME added RenameFile").
// The page builds one tick box per entry of this table (SettingsPresenter::context_menu_rows walks it whole), so the table is what is pinned here.
#[test]
fn context_menu_table_has_nineteen_items_and_rename_file_is_the_win32_only_last_id() {
    assert_eq!(ITEMS.len(), 19);
    assert!(ITEMS.iter().any(|info| info.id == ContextMenuItemId::RenameFile));
    assert_eq!(ContextMenuItemId::RenameFile as i32, 18);
}

// C#: SettingsShortcutsPageTests (F2 is now the default of Rename file).
#[test]
fn f2_is_the_default_shortcut_of_rename_file() {
    assert_eq!(
        pr_core::settings::shortcuts::ShortcutMappings::default().rename_file,
        "F2"
    );
}

// ---- native (test-owned windows, hidden behind the test owner) ----------------------------------------------------------------------------------

fn validator() -> RenamePromptRequest {
    #[allow(clippy::arc_with_non_send_sync)] // the request type is Arc-based; the tests use it on one thread
    let validate: pr_core::abstractions::RenameValidateFn = Arc::new(|text, is_commit| {
        if text.contains('*') {
            Some("A file name cannot contain *".to_owned())
        } else if is_commit && text == "TAKEN" {
            Some("TAKEN exists".to_owned())
        } else {
            None
        }
    });
    RenamePromptRequest {
        initial_name: "IMG_0001".to_owned(),
        extension: ".jpg".to_owned(),
        validate,
    }
}

// C#: RenamePromptDialog (OnInitialized): the stem fills the box and is all selected, the extension shows right of it, Rename starts enabled.
#[test]
#[ignore = "native: needs desktop"]
fn rename_dialog_preselects_the_stem_and_shows_the_immutable_extension() {
    on_sta(|| {
        let prompt = RenamePromptDialog::new(validator(), None);
        let result = show_on_owner(prompt.native(), |dialog, _| {
            assert_eq!(prompt.current_text(), "IMG_0001");
            assert_eq!(prompt.extension_text(), ".jpg");
            assert_eq!(prompt.selection(), (0, 8));
            assert!(prompt.is_rename_enabled());
            assert_eq!(prompt.error_message(), None);
            assert!(!fixture::is_visible(dialog, rename::ERROR_ID));
            command(dialog, ID_CANCEL, 0);
        })
        .unwrap();
        assert_eq!(result, ID_CANCEL);
        assert_eq!(prompt.value(), None, "Escape/Cancel: no file operation");
    });
}

// C#: RenamePromptDialog (EN_CHANGE -> Revalidate): the error shows under the box, Rename is disabled, the window grows; fixing the text reverses all of it.
#[test]
#[ignore = "native: needs desktop"]
fn rename_dialog_validates_on_en_change_disables_rename_and_grows_and_shrinks_the_error_line() {
    on_sta(|| {
        let prompt = RenamePromptDialog::new(validator(), None);
        show_on_owner(prompt.native(), |dialog, _| {
            let before = dialog.current_client_size();
            set_text(dialog, rename::EDIT_ID, "bad*name");
            assert_eq!(prompt.error_message().as_deref(), Some("A file name cannot contain *"));
            assert!(!prompt.is_rename_enabled());
            assert!(fixture::is_visible(dialog, rename::ERROR_ID));
            let grown = dialog.current_client_size();
            assert!(
                grown.1 > before.1,
                "the window grows for the error line: {before:?} -> {grown:?}"
            );
            // The error line lies between the box and the buttons, never over either.
            let edit = fixture::child_rect(dialog, rename::EDIT_ID);
            let error = fixture::child_rect(dialog, rename::ERROR_ID);
            let cancel = fixture::child_rect(dialog, rename::CANCEL_ID);
            assert!(error.top >= edit.bottom && error.bottom <= cancel.top, "no overlap");
            set_text(dialog, rename::EDIT_ID, "good");
            assert_eq!(prompt.error_message(), None);
            assert!(prompt.is_rename_enabled());
            assert!(!fixture::is_visible(dialog, rename::ERROR_ID));
            assert_eq!(dialog.current_client_size(), before, "the window shrinks back");
            command(dialog, ID_CANCEL, 0);
        })
        .unwrap();
    });
}

// C#: RenamePromptDialog.OnOk: the confirming call (isCommit = true) may refuse; the dialog stays open, the box has the focus, the text is kept.
#[test]
#[ignore = "native: needs desktop"]
fn rename_dialog_ok_asks_again_with_commit_and_stays_open_on_a_refusal() {
    on_sta(|| {
        let prompt = RenamePromptDialog::new(validator(), None);
        let result = show_on_owner(prompt.native(), |dialog, _| {
            set_text(dialog, rename::EDIT_ID, " TAKEN ");
            assert_eq!(prompt.error_message(), None, "typing does not ask the disk");
            command(dialog, ID_OK, 0);
            assert!(!dialog.is_closed());
            assert_eq!(prompt.error_message().as_deref(), Some("TAKEN exists"));
            assert!(!prompt.is_rename_enabled());
            assert_eq!(dialog_native::user32::get_focus(), item(dialog, rename::EDIT_ID));
            assert_eq!(prompt.value(), None);
            set_text(dialog, rename::EDIT_ID, "  Holiday  ");
            command(dialog, ID_OK, 0);
        })
        .unwrap();
        assert_eq!(result, ID_OK);
        assert_eq!(
            prompt.value().as_deref(),
            Some("Holiday"),
            "only the trimmed stem; the extension stays fixed"
        );
    });
}

// C#: Enter accepts only a valid name, Escape cancels.
#[test]
#[ignore = "native: needs desktop"]
fn rename_dialog_enter_accepts_only_a_valid_name_and_escape_cancels() {
    on_sta(|| {
        let prompt = RenamePromptDialog::new(validator(), None);
        let result = show_on_owner(prompt.native(), |dialog, _| {
            set_text(dialog, rename::EDIT_ID, "a*b");
            fixture::key(dialog, item(dialog, rename::EDIT_ID), fixture::VK_RETURN);
            assert!(!dialog.is_closed(), "Enter on an invalid name does nothing");
            fixture::key(dialog, item(dialog, rename::EDIT_ID), fixture::VK_ESCAPE);
        })
        .unwrap();
        assert_eq!(result, ID_CANCEL);
        assert_eq!(prompt.value(), None);
    });
}

// C#: Win32SecondaryWindowPort.PromptRename through Run: ID_OK gives the typed name, any other id null; nothing stays registered open.
#[test]
#[ignore = "native: needs desktop"]
fn port_prompt_rename_returns_the_name_on_ok_and_none_on_cancel() {
    on_sta(|| {
        for (result, expected) in [(ID_OK, Some("Trip")), (ID_CANCEL, None)] {
            struct Real;
            impl SecondaryWindowDialogs for Real {
                fn settings(&self, _: SettingsTarget, _: ActionProfilesOpener) -> Option<PreparedDialog<bool>> {
                    None
                }
                fn recovery(&self, _: Hwnd) -> Option<PreparedDialog<()>> {
                    None
                }
                fn benchmark(&self, _: Option<&std::path::Path>) -> Option<NativeDialog> {
                    None
                }
                fn action_profiles(&self, _: &[ReviewAction]) -> Option<PreparedDialog<Option<Vec<ReviewAction>>>> {
                    None
                }
                fn diagnostics(&self) -> Option<PreparedDialog<()>> {
                    None
                }
                fn skipped_files(&self, _: &[SkippedEntry]) -> Option<PreparedDialog<()>> {
                    None
                }
                fn batch_review(&self, _: &[BatchReviewItem]) -> Option<PreparedDialog<bool>> {
                    None
                }
                fn custom_zoom(&self, _: i32) -> Option<PreparedDialog<Option<i32>>> {
                    None
                }
            }
            let port = Win32SecondaryWindowPort::new(
                Rc::new(Real),
                Arc::new(NullLog),
                PortOptions {
                    ready: Some(Rc::new(move |dialog| {
                        set_text(dialog, rename::EDIT_ID, " Trip ");
                        command(dialog, result, 0);
                    })),
                    ..Default::default()
                },
            );
            let owner = create_owner();
            let answer = port.prompt_rename(owner, &validator());
            destroy_owner(owner);
            assert_eq!(answer.as_deref(), expected);
            assert_eq!(port.open_count(), 0);
        }
    });
}

// C#: DialogEditCenteringTests.SingleLineEditText_IsVerticallyCentred_AndCaretIsInsideTheTextRows ("rename" case). The C# measures ink on a window-DC
// BitBlt for 96/144 DPI and en/vi; this port checks the same promise structurally (the first text row and the caret lie inside the client area of the
// box, and that box has a client area smaller than its window, i.e. the subclass moved the text) in the language of the process. The pixel measurement is NOT ported.
#[test]
#[ignore = "native: needs desktop"]
fn single_line_edit_text_is_vertically_centred_and_caret_is_inside_the_text_rows() {
    on_sta(|| {
        let prompt = RenamePromptDialog::new(validator(), None);
        show_on_owner(prompt.native(), |dialog, _| {
            let edit = item(dialog, rename::EDIT_ID);
            let window = fixture::window_rect(edit);
            let client = fixture::client_rect(edit);
            let (window_h, client_h) = (fixture::height(window), fixture::height(client));
            assert!(
                client_h > 0 && client_h < window_h,
                "the client area is the shrunken text area"
            );
            // SAFETY: EM_POSFROMCHAR takes a character index and no pointer; the handle is a live control of this dialog.
            let packed = unsafe { dialog_native::user32::send_message(edit, 0x00D6, 1, 0) };
            let char_y = i32::from(((packed >> 16) & 0xFFFF) as u16 as i16);
            let line = fixture::font_height(dialog.body_font());
            assert!(
                char_y >= 0,
                "the first text row starts inside the client area (y = {char_y})"
            );
            assert!(
                char_y + line <= client_h + 1,
                "the row fits: y {char_y} + line {line} vs client {client_h}"
            );
            command(dialog, ID_CANCEL, 0);
        })
        .unwrap();
    });
}
