//! Port of `ExplorerOrderService.QueryShell/TryReadNativeView/TryReadViewOrderBatched/ReadSortColumns/ReadGroupState`
//! in PhotoReview at C# baseline d92d6aae. The caller runs this on W6-01c's STA pump.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;
use std::thread;

use pr_core::abstractions::Log;
use pr_core::catalog::explorer_reason;
use pr_core::catalog::explorer_types::{
    ExplorerGroupState, ExplorerOrderStatus, ExplorerQueryProgress, ExplorerSortColumn, ExplorerSortDirection,
    ExplorerViewSnapshot, Guid,
};
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;
use windows_core::GUID;

use crate::explorer_com_interop::{
    self as com, ExplorerNativeVtable, IExplorerWindow, IExplorerWindowSource, ShellCom, ShellComError, ShellWebBrowser,
};
use crate::explorer_window_selector::{
    self as selector, ExplorerSelectionWindow, LocationReadError, SelectionReadError,
};

const MAX_BATCH: usize = 512;
const MAX_SORT_COLUMNS: usize = 32;
const E_FAIL: i32 = 0x8000_4005_u32 as i32;

/// The production query delegate for the W6-01c scheduling service.
pub(crate) fn query_shell(
    folder: &str,
    progress: Option<&(dyn Fn(ExplorerQueryProgress) + Sync)>,
    batch_size: usize,
    cancellation: &CancelToken,
    log: Arc<dyn Log>,
) -> Result<ExplorerViewSnapshot, super::explorer_order_service::QueryError> {
    if cancellation.is_canceled() {
        return Ok(unavailable(
            folder,
            ExplorerOrderStatus::Canceled,
            explorer_reason::CANCELED,
        ));
    }
    let source = match com::ShellWindowsSource::open() {
        Ok(source) => source,
        Err(error) => return map_shell_open_error(folder, error.hresult()),
    };
    let windows = source.windows().map(|window| {
        window
            .map(|window| {
                Box::new(SelectionWindow(window, progress, batch_size)) as Box<dyn ExplorerSelectionWindow + '_>
            })
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)
    });
    Ok(select_windows(folder, windows, log.as_ref(), cancellation))
}

pub(crate) fn map_shell_open_error(
    folder: &str,
    hresult: i32,
) -> Result<ExplorerViewSnapshot, super::explorer_order_service::QueryError> {
    if hresult == com::ids::CLASS_NOT_REGISTERED {
        Ok(unavailable(
            folder,
            ExplorerOrderStatus::NativeViewUnavailable,
            explorer_reason::SHELL_UNAVAILABLE,
        ))
    } else {
        Err(super::explorer_order_service::QueryError::shell_open(hresult))
    }
}

pub(crate) fn select_windows<I, W>(
    folder: &str,
    windows: I,
    log: &dyn Log,
    cancellation: &CancelToken,
) -> ExplorerViewSnapshot
where
    I: IntoIterator<Item = Result<W, Box<dyn std::error::Error + Send + Sync>>>,
    W: ExplorerSelectionWindow,
{
    selector::select(folder, windows, log, cancellation)
}

struct SelectionWindow<'a>(
    ShellWebBrowser,
    Option<&'a (dyn Fn(ExplorerQueryProgress) + Sync)>,
    usize,
);

impl ExplorerSelectionWindow for Box<dyn ExplorerSelectionWindow + '_> {
    fn location_url(&self) -> Result<Option<String>, LocationReadError> {
        (**self).location_url()
    }
    fn read(&self, folder: &str, cancellation: &CancelToken) -> Result<ExplorerViewSnapshot, SelectionReadError> {
        (**self).read(folder, cancellation)
    }
    fn release(&self) {
        (**self).release()
    }
}

impl ExplorerSelectionWindow for SelectionWindow<'_> {
    fn location_url(&self) -> Result<Option<String>, LocationReadError> {
        // The C# selector skips any COMException or InvalidCastException without logging; this call only produces
        // ShellComError values, so every HRESULT failure is an unavailable candidate window.
        self.0.location_url().map_err(|_error| LocationReadError::Unavailable)
    }

    fn read(&self, folder: &str, cancellation: &CancelToken) -> Result<ExplorerViewSnapshot, SelectionReadError> {
        let unknown = self.0.acquire_unknown().map_err(selection_error)?;
        let pointer = NonNull::new(unknown.as_raw()).ok_or_else(canceled_error)?;
        read_native_view(pointer, folder, self.1, self.2, cancellation)
    }

    fn release(&self) {
        self.0.dispose();
    }
}

/// Query the selected Explorer window's active view. Each acquired COM pointer and task allocation is scoped so all
/// early returns, HRESULT failures, and cancellation paths release exactly the references they own.
pub(crate) fn read_native_view(
    window: NonNull<c_void>,
    folder: &str,
    progress: Option<&(dyn Fn(ExplorerQueryProgress) + Sync)>,
    batch_size: usize,
    cancellation: &CancelToken,
) -> Result<ExplorerViewSnapshot, SelectionReadError> {
    read_native_view_with(window, folder, progress, batch_size, cancellation, &ShellPidlApi)
}

pub(crate) fn read_native_view_with(
    window: NonNull<c_void>,
    folder: &str,
    progress: Option<&(dyn Fn(ExplorerQueryProgress) + Sync)>,
    batch_size: usize,
    cancellation: &CancelToken,
    pidls: &dyn PidlApi,
) -> Result<ExplorerViewSnapshot, SelectionReadError> {
    let service = match query_interface(window.as_ptr(), &com::ids::IID_SERVICE_PROVIDER) {
        Ok(pointer) => pointer,
        Err(error) => return Ok(com_failure(folder, "QueryInterface(IServiceProvider)", error.hresult())),
    };
    let browser = match query_service(
        service.as_raw(),
        &com::ids::SID_TOP_LEVEL_BROWSER,
        &com::ids::IID_SHELL_BROWSER,
    ) {
        Ok(pointer) => pointer,
        Err(error) => return Ok(com_failure(folder, "QueryService(IShellBrowser)", error.hresult())),
    };
    let shell_view = match query_active_shell_view(browser.as_raw()) {
        Ok(pointer) => pointer,
        Err(error) => return Ok(com_failure(folder, "QueryActiveShellView", error.hresult())),
    };
    let folder_view = match query_interface(shell_view.as_raw(), &com::ids::IID_FOLDER_VIEW2) {
        Ok(pointer) => pointer,
        Err(error) => return Ok(com_failure(folder, "QueryInterface(IFolderView2)", error.hresult())),
    };
    let count = match ExplorerNativeVtable::item_count(folder_view.pointer(), com::ids::SVGIO_ALL_VIEW) {
        Ok(count) => count,
        Err(error) => {
            return Ok(unavailable(
                folder,
                ExplorerOrderStatus::NativeViewUnavailable,
                &explorer_reason::format(
                    explorer_reason::EMPTY_VIEW,
                    &[&format!("0x{:08X}", error.hresult()), "0"],
                ),
            ));
        }
    };
    if count <= 0 {
        return Ok(unavailable(
            folder,
            ExplorerOrderStatus::NativeViewUnavailable,
            &explorer_reason::format(explorer_reason::EMPTY_VIEW, &["0x00000000", &count.to_string()]),
        ));
    }
    if cancellation.is_canceled() {
        return Err(canceled_error());
    }
    let count = count as usize;
    let paths = match read_batched_with(folder_view.pointer(), folder, count, cancellation, pidls) {
        Ok(Some(paths)) => {
            if let Some(progress) = progress {
                progress(ExplorerQueryProgress {
                    items_read: paths.len() as i32,
                    item_count: count as i32,
                    com_calls: 1,
                });
            }
            paths
        }
        Ok(None) => match read_per_item(folder_view.pointer(), folder, count, batch_size, progress, cancellation) {
            Ok(paths) => paths,
            Err(error) if error.canceled => return Err(error),
            Err(error) => return Ok(com_failure(folder, &error.type_name, error.hresult)),
        },
        Err(error) if error.canceled => return Err(error),
        Err(_) => match read_per_item(folder_view.pointer(), folder, count, batch_size, progress, cancellation) {
            Ok(paths) => paths,
            Err(error) if error.canceled => return Err(error),
            Err(error) => return Ok(com_failure(folder, &error.type_name, error.hresult)),
        },
    };
    let sort_columns = read_sort_columns(folder_view.pointer());
    let group_state = read_group_state(folder_view.pointer());
    Ok(ExplorerViewSnapshot {
        folder: folder.to_owned(),
        ordered_paths: paths,
        sort_columns,
        group_state,
        status: ExplorerOrderStatus::Available,
        reason: None,
        captured_utc: DateTimeUtc::now(),
    })
}

pub(crate) fn read_batched(
    folder_view: NonNull<c_void>,
    folder: &str,
    expected: usize,
    cancellation: &CancelToken,
) -> Result<Option<Vec<String>>, SelectionReadError> {
    read_batched_with(folder_view, folder, expected, cancellation, &ShellPidlApi)
}

pub(crate) trait PidlApi {
    fn parse_folder(&self, folder: &str) -> Option<NonNull<c_void>>;
    fn combine(&self, parent: NonNull<c_void>, child: NonNull<c_void>) -> Option<NonNull<c_void>>;
    fn file_system_path(&self, absolute: NonNull<c_void>) -> Option<NonNull<u16>>;
}

struct ShellPidlApi;

impl PidlApi for ShellPidlApi {
    fn parse_folder(&self, folder: &str) -> Option<NonNull<c_void>> {
        let encoded: Vec<u16> = folder.encode_utf16().chain(std::iter::once(0)).collect();
        let mut parent_raw = std::ptr::null_mut();
        let mut attributes = 0u32;
        let hr = unsafe {
            // SAFETY: `encoded` is null-terminated and remains alive through the synchronous shell call; output is writable.
            com::sh_parse_display_name(
                encoded.as_ptr(),
                std::ptr::null_mut(),
                &mut parent_raw,
                0,
                &mut attributes,
            )
        };
        let parent = NonNull::new(parent_raw)?;
        if hr < 0 {
            // SAFETY: SHParseDisplayName transfers any non-null output through the task allocator even on failure.
            unsafe { CoTaskMemFree(parent.as_ptr()) }
            None
        } else {
            Some(parent)
        }
    }

    fn combine(&self, parent: NonNull<c_void>, child: NonNull<c_void>) -> Option<NonNull<c_void>> {
        // SAFETY: both pointers are live PIDLs owned by the caller for this synchronous shell call.
        NonNull::new(unsafe { com::il_combine(parent.as_ptr(), child.as_ptr()) })
    }

    fn file_system_path(&self, absolute: NonNull<c_void>) -> Option<NonNull<u16>> {
        let mut name = std::ptr::null_mut();
        let hr = unsafe {
            // SAFETY: `absolute` is a live PIDL and `name` is a writable task-allocator output slot.
            com::sh_get_name_from_id_list(absolute.as_ptr(), com::ids::SIGDN_FILE_SYSTEM_PATH, &mut name)
        };
        let name = NonNull::new(name)?;
        if hr < 0 {
            // SAFETY: SHGetNameFromIDList returns task-allocator memory even if it also reports failure.
            unsafe { CoTaskMemFree(name.as_ptr().cast()) }
            None
        } else {
            Some(name)
        }
    }
}

pub(crate) fn read_batched_with(
    folder_view: NonNull<c_void>,
    folder: &str,
    expected: usize,
    cancellation: &CancelToken,
    pidls: &dyn PidlApi,
) -> Result<Option<Vec<String>>, SelectionReadError> {
    // Cancellation before starting avoids opening the enumerator or parsing the folder PIDL.
    if cancellation.is_canceled() {
        return Err(canceled_error());
    }
    let enumerator = match call_items(folder_view) {
        Ok(pointer) => OwnedCom(pointer),
        Err(_) => return Ok(None),
    };
    let Some(parent) = pidls.parse_folder(folder) else {
        return Ok(None);
    };
    let parent = TaskMem(parent);

    let batch_len = expected.clamp(1, MAX_BATCH);
    let mut children = vec![std::ptr::null_mut(); batch_len];
    let mut result = Vec::with_capacity(expected);
    loop {
        if cancellation.is_canceled() {
            free_pidls(&mut children);
            return Err(canceled_error());
        }
        let (hr, fetched) = enum_next(enumerator.pointer(), &mut children);
        let fetched = (fetched as usize).min(children.len());
        if hr < 0 {
            free_pidls(&mut children);
            return Ok(None);
        }
        for child in children.iter_mut().take(fetched) {
            if cancellation.is_canceled() {
                free_pidls(&mut children);
                return Err(canceled_error());
            }
            let Some(child_ptr) = NonNull::new(*child) else {
                continue;
            };
            let Some(absolute) = pidls.combine(parent.0, child_ptr) else {
                free_pidls(&mut children);
                return Ok(None);
            };
            let absolute = TaskMem(absolute);
            let Some(name) = pidls.file_system_path(absolute.0) else {
                free_pidls(&mut children);
                return Ok(None);
            };
            let name = TaskMem(name);
            match read_wide_string(name.as_ptr()) {
                Some(path) => result.push(path),
                None => {
                    free_pidls(&mut children);
                    return Ok(None);
                }
            }
        }
        free_pidls(&mut children);
        // C# TryReadViewOrderBatched: stop on S_FALSE (enumerator exhausted) or an empty read.
        if hr == com::ids::S_FALSE || fetched == 0 {
            break;
        }
    }
    Ok((result.len() == expected).then_some(result))
}

pub(crate) fn read_per_item(
    folder_view: NonNull<c_void>,
    folder: &str,
    count: usize,
    batch_size: usize,
    progress: Option<&(dyn Fn(ExplorerQueryProgress) + Sync)>,
    cancellation: &CancelToken,
) -> Result<Vec<String>, SelectionReadError> {
    let get_item = ExplorerNativeVtable::resolve_get_item(folder_view);
    let mut paths = Vec::with_capacity(count);
    let mut calls = 1i32;
    for index in 0..count {
        if cancellation.is_canceled() {
            return Err(canceled_error());
        }
        let item = match call_get_item(get_item, folder_view, index as i32) {
            Ok(item) => OwnedCom(item),
            Err(error) => return Err(native_error(folder, "IFolderView2.GetItem", index, error.hresult())),
        };
        let function = ExplorerNativeVtable::resolve_get_display_name(item.pointer());
        let name = match call_display_name(function, item.pointer()) {
            Ok(name) => TaskMem(name),
            Err(error) => {
                return Err(native_error(
                    folder,
                    "IShellItem.GetDisplayName",
                    index,
                    error.hresult(),
                ));
            }
        };
        let Some(path) = read_wide_string(name.as_ptr()) else {
            return Err(native_error(folder, "IShellItem.GetDisplayName", index, E_FAIL));
        };
        paths.push(path);
        calls += 2;
        if let Some(progress) = progress {
            progress(ExplorerQueryProgress {
                items_read: paths.len() as i32,
                item_count: count as i32,
                com_calls: calls,
            });
        }
        if batch_size != usize::MAX && batch_size > 0 && paths.len() % batch_size == 0 {
            thread::yield_now();
        }
    }
    Ok(paths)
}

pub(crate) fn read_sort_columns(view: NonNull<c_void>) -> Vec<ExplorerSortColumn> {
    let Ok(count) = ExplorerNativeVtable::get_sort_column_count(view) else {
        return vec![unknown_sort()];
    };
    if count < 0 || count as usize > MAX_SORT_COLUMNS {
        return vec![unknown_sort()];
    }
    if count == 0 {
        return Vec::new();
    }
    let mut columns = vec![com::SortColumn::default(); count as usize];
    if ExplorerNativeVtable::get_sort_columns(view, &mut columns).is_err() {
        return vec![unknown_sort()];
    }
    columns
        .into_iter()
        .map(|column| ExplorerSortColumn {
            property_set: from_windows_guid(column.propkey.fmtid),
            property_id: column.propkey.pid,
            direction: match column.direction {
                1 => ExplorerSortDirection::Ascending,
                -1 => ExplorerSortDirection::Descending,
                _ => ExplorerSortDirection::Unknown,
            },
        })
        .collect()
}

pub(crate) fn read_group_state(view: NonNull<c_void>) -> ExplorerGroupState {
    match ExplorerNativeVtable::get_group_by(view) {
        Ok((key, _)) if key.fmtid != GUID::zeroed() || key.pid != 0 => ExplorerGroupState::Active,
        Ok(_) => ExplorerGroupState::None,
        Err(_) => ExplorerGroupState::Unknown,
    }
}

fn unknown_sort() -> ExplorerSortColumn {
    ExplorerSortColumn {
        property_set: Guid([0; 16]),
        property_id: 0,
        direction: ExplorerSortDirection::Unknown,
    }
}

pub(crate) fn from_windows_guid(guid: GUID) -> Guid {
    let mut bytes = [0u8; 16];
    bytes[0..4].copy_from_slice(&guid.data1.to_le_bytes());
    bytes[4..6].copy_from_slice(&guid.data2.to_le_bytes());
    bytes[6..8].copy_from_slice(&guid.data3.to_le_bytes());
    bytes[8..16].copy_from_slice(&guid.data4);
    Guid(bytes)
}

fn unavailable(folder: &str, status: ExplorerOrderStatus, reason: &str) -> ExplorerViewSnapshot {
    ExplorerViewSnapshot {
        folder: folder.to_owned(),
        ordered_paths: Vec::new(),
        sort_columns: Vec::new(),
        group_state: ExplorerGroupState::Unknown,
        status,
        reason: Some(reason.to_owned()),
        captured_utc: DateTimeUtc::now(),
    }
}

fn com_failure(folder: &str, method: &str, hresult: i32) -> ExplorerViewSnapshot {
    unavailable(
        folder,
        ExplorerOrderStatus::NativeViewUnavailable,
        &explorer_reason::format(
            explorer_reason::COM_CALL_FAILED,
            &[method, &format!("0x{:08X}", hresult as u32)],
        ),
    )
}

fn native_error(_folder: &str, method: &str, index: usize, hresult: i32) -> SelectionReadError {
    SelectionReadError {
        type_name: format!("{method}({index})"),
        hresult,
        canceled: false,
    }
}

fn selection_error(error: ShellComError) -> SelectionReadError {
    SelectionReadError {
        type_name: "COMException".into(),
        hresult: error.hresult(),
        canceled: false,
    }
}

fn canceled_error() -> SelectionReadError {
    SelectionReadError {
        type_name: "OperationCanceledException".into(),
        hresult: 0x8013_153B_u32 as i32,
        canceled: true,
    }
}

struct OwnedCom(NonNull<c_void>);
impl OwnedCom {
    fn as_raw(&self) -> *mut c_void {
        self.0.as_ptr()
    }
    fn pointer(&self) -> NonNull<c_void> {
        self.0
    }
    fn into_raw(self) -> NonNull<c_void> {
        let pointer = self.0;
        std::mem::forget(self);
        pointer
    }
}
impl Drop for OwnedCom {
    fn drop(&mut self) {
        // SAFETY: this guard adopts exactly one owned COM reference returned by QueryInterface or a shell method.
        unsafe { ShellCom::release(self.0) }
    }
}

struct TaskMem<T>(NonNull<T>);
impl<T> TaskMem<T> {
    fn as_ptr(&self) -> *mut T {
        self.0.as_ptr()
    }
}
impl<T> Drop for TaskMem<T> {
    fn drop(&mut self) {
        // SAFETY: the shell functions used here return task-allocator memory, released through CoTaskMemFree.
        unsafe { CoTaskMemFree(self.0.as_ptr().cast()) }
    }
}

unsafe extern "system" {
    fn CoTaskMemFree(memory: *mut c_void);
}

fn query_interface(pointer: *mut c_void, iid: &GUID) -> Result<OwnedCom, ShellComError> {
    let pointer = NonNull::new(pointer).ok_or(ShellComError(E_FAIL))?;
    let mut result = std::ptr::null_mut();
    let hr = unsafe {
        // SAFETY: pointer is a live COM object; IID and writable result pointer live through QueryInterface.
        let table = *(pointer.as_ptr().cast::<*const *const c_void>());
        let query: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32 =
            std::mem::transmute(*table);
        query(pointer.as_ptr(), iid, &mut result)
    };
    finish_interface(hr, result)
}

fn query_service(pointer: *mut c_void, service: &GUID, iid: &GUID) -> Result<OwnedCom, ShellComError> {
    let pointer = NonNull::new(pointer).ok_or(ShellComError(E_FAIL))?;
    let mut result = std::ptr::null_mut();
    let hr = unsafe {
        // SAFETY: pointer is IServiceProvider; GUIDs and output pointer are valid for the synchronous call.
        let table = *(pointer.as_ptr().cast::<*const *const c_void>());
        let call: unsafe extern "system" fn(*mut c_void, *const GUID, *const GUID, *mut *mut c_void) -> i32 =
            std::mem::transmute(*table.add(3));
        call(pointer.as_ptr(), service, iid, &mut result)
    };
    finish_interface(hr, result)
}

fn query_active_shell_view(pointer: *mut c_void) -> Result<OwnedCom, ShellComError> {
    let pointer = NonNull::new(pointer).ok_or(ShellComError(E_FAIL))?;
    let mut result = std::ptr::null_mut();
    let hr = unsafe {
        // SAFETY: pointer is IShellBrowser; QueryActiveShellView uses slot 15 and a writable interface output.
        let table = *(pointer.as_ptr().cast::<*const *const c_void>());
        let call: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32 = std::mem::transmute(*table.add(15));
        call(pointer.as_ptr(), &mut result)
    };
    finish_interface(hr, result)
}

fn finish_interface(hr: i32, result: *mut c_void) -> Result<OwnedCom, ShellComError> {
    if hr < 0 {
        if let Some(result) = NonNull::new(result) {
            // SAFETY: a non-null interface pointer in a COM out slot carries one owned reference even on failure.
            unsafe { ShellCom::release(result) }
        }
        return Err(ShellComError(hr));
    }
    NonNull::new(result).map(OwnedCom).ok_or(ShellComError(E_FAIL))
}

fn call_items(view: NonNull<c_void>) -> Result<NonNull<c_void>, ShellComError> {
    let mut result = std::ptr::null_mut();
    let iid = com::ids::IID_ENUM_ID_LIST;
    let hr = unsafe {
        // SAFETY: view is IFolderView2; slot 8 accepts SVGIO flags, a valid IID and writable result pointer.
        let table = *(view.as_ptr().cast::<*const *const c_void>());
        let call: unsafe extern "system" fn(*mut c_void, u32, *const GUID, *mut *mut c_void) -> i32 =
            std::mem::transmute(*table.add(8));
        call(
            view.as_ptr(),
            com::ids::SVGIO_ALL_VIEW | com::ids::SVGIO_FLAG_VIEW_ORDER,
            &iid,
            &mut result,
        )
    };
    let result = finish_interface(hr, result)?;
    Ok(result.into_raw())
}

fn call_get_item(function: *const c_void, view: NonNull<c_void>, index: i32) -> Result<NonNull<c_void>, ShellComError> {
    let mut result = std::ptr::null_mut();
    let iid = com::ids::IID_SHELL_ITEM;
    let hr = unsafe {
        // SAFETY: function is this view's slot 29; arguments match IFolderView::GetItem and remain valid through the call.
        let call: unsafe extern "system" fn(*mut c_void, i32, *const GUID, *mut *mut c_void) -> i32 =
            std::mem::transmute(function);
        call(view.as_ptr(), index, &iid, &mut result)
    };
    let result = finish_interface(hr, result)?;
    Ok(result.into_raw())
}

fn call_display_name(function: *const c_void, item: NonNull<c_void>) -> Result<NonNull<u16>, ShellComError> {
    let mut name = std::ptr::null_mut();
    let hr = unsafe {
        // SAFETY: function is this item's slot 5; output is writable and uses SIGDN_FILESYSPATH.
        let call: unsafe extern "system" fn(*mut c_void, u32, *mut *mut u16) -> i32 = std::mem::transmute(function);
        call(item.as_ptr(), com::ids::SIGDN_FILE_SYSTEM_PATH, &mut name)
    };
    if hr < 0 {
        if let Some(name) = NonNull::new(name) {
            // SAFETY: a non-null name output is task-allocator memory transferred by the COM method even on failure.
            unsafe { CoTaskMemFree(name.as_ptr().cast()) }
        }
        return Err(ShellComError(hr));
    }
    NonNull::new(name).ok_or(ShellComError(E_FAIL))
}

fn enum_next(enumerator: NonNull<c_void>, children: &mut [*mut c_void]) -> (i32, u32) {
    let mut fetched = 0u32;
    let hr = unsafe {
        // SAFETY: enumerator is IEnumIDList, child array has count writable pointer slots, and fetched is writable.
        let table = *(enumerator.as_ptr().cast::<*const *const c_void>());
        let call: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void, *mut u32) -> i32 =
            std::mem::transmute(*table.add(3));
        call(
            enumerator.as_ptr(),
            children.len() as u32,
            children.as_mut_ptr(),
            &mut fetched,
        )
    };
    (hr, fetched)
}

fn free_pidls(children: &mut [*mut c_void]) {
    for child in children.iter_mut() {
        if let Some(child_ptr) = NonNull::new(*child) {
            // SAFETY: every non-null PIDL was returned by IEnumIDList::Next and is task-allocator owned by this loop.
            unsafe { CoTaskMemFree(child_ptr.as_ptr()) }
            *child = std::ptr::null_mut();
        }
    }
}

pub(crate) fn read_wide_string(pointer: *mut u16) -> Option<String> {
    const MAX_WIDE_UNITS: usize = 32_768;
    if pointer.is_null() {
        return None;
    }
    let mut length = 0;
    while length < MAX_WIDE_UNITS {
        let unit = unsafe {
            // SAFETY: shell-owned display-name strings are null-terminated UTF-16; length is bounded to avoid unbounded reads.
            *pointer.add(length)
        };
        if unit == 0 {
            let slice = unsafe {
                // SAFETY: the validated prefix is readable and contains no interior terminator by construction.
                std::slice::from_raw_parts(pointer, length)
            };
            return Some(String::from_utf16_lossy(slice));
        }
        length += 1;
    }
    None
}
