// Justification: this target includes adjacent production modules by path; it exercises only the native-view surface.
#![allow(dead_code)]

#[path = "../src/explorer_com_interop.rs"]
mod explorer_com_interop;
#[path = "../src/explorer_native_view.rs"]
mod explorer_native_view;
#[path = "../src/explorer_order_service.rs"]
mod explorer_order_service;
#[path = "../src/explorer_window_selector.rs"]
mod explorer_window_selector;

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

use explorer_com_interop::{PropertyKey, SortColumn};
use explorer_native_view::{
    from_windows_guid, read_batched, read_group_state, read_native_view, read_sort_columns, read_wide_string,
};
use pr_core::catalog::explorer_types::{ExplorerGroupState, ExplorerOrderStatus, ExplorerSortDirection};

const E_FAIL: i32 = 0x8000_4005_u32 as i32;

#[derive(Clone)]
struct ViewData {
    group_hr: i32,
    group: PropertyKey,
    count_hr: i32,
    columns_hr: i32,
    columns: Vec<SortColumn>,
    items_hr: i32,
    last_items_flags: u32,
}

fn views() -> &'static Mutex<HashMap<usize, ViewData>> {
    static VIEWS: OnceLock<Mutex<HashMap<usize, ViewData>>> = OnceLock::new();
    VIEWS.get_or_init(|| Mutex::new(HashMap::new()))
}

unsafe extern "system" fn group_by(this: *mut c_void, key: *mut PropertyKey, ascending: *mut i32) -> i32 {
    let values = views().lock().unwrap();
    let value = &values[&(this as usize)];
    // SAFETY: test fake is called only through slot 18 with valid writable output parameters.
    unsafe {
        *key = value.group;
        *ascending = 1;
    }
    value.group_hr
}

unsafe extern "system" fn sort_count(this: *mut c_void, count: *mut i32) -> i32 {
    let values = views().lock().unwrap();
    let value = &values[&(this as usize)];
    // SAFETY: test fake is called only through slot 26 with a valid writable output parameter.
    unsafe {
        *count = value.columns.len() as i32;
    }
    value.count_hr
}

unsafe extern "system" fn sort_columns(this: *mut c_void, output: *mut SortColumn, count: i32) -> i32 {
    let values = views().lock().unwrap();
    let value = &values[&(this as usize)];
    // SAFETY: test fake is called only through slot 28 with `count` writable ABI-compatible entries.
    unsafe {
        std::ptr::copy_nonoverlapping(
            value.columns.as_ptr(),
            output,
            count.max(0).min(value.columns.len() as i32) as usize,
        );
    }
    value.columns_hr
}

unsafe extern "system" fn items(
    this: *mut c_void,
    flags: u32,
    _iid: *const windows_core::GUID,
    output: *mut *mut c_void,
) -> i32 {
    let mut values = views().lock().unwrap();
    let value = values.get_mut(&(this as usize)).expect("fake view is registered");
    value.last_items_flags = flags;
    // SAFETY: IFolderView::Items receives a writable output slot; the fake intentionally returns no enumerator on failure.
    unsafe {
        *output = std::ptr::null_mut();
    }
    value.items_hr
}

unsafe extern "system" fn unsupported_query(
    _this: *mut c_void,
    _iid: *const windows_core::GUID,
    output: *mut *mut c_void,
) -> i32 {
    // SAFETY: fake IUnknown::QueryInterface receives the ABI's writable output slot.
    unsafe {
        *output = std::ptr::null_mut();
    }
    0x8000_4002_u32 as i32
}

struct FakeView {
    object: *mut *const *const c_void,
    table: Box<[*const c_void; 29]>,
}

impl FakeView {
    fn new(data: ViewData) -> Self {
        let mut table = Box::new([std::ptr::null(); 29]);
        table[18] = group_by as *const c_void;
        table[26] = sort_count as *const c_void;
        table[28] = sort_columns as *const c_void;
        table[8] = items as *const c_void;
        let object = Box::into_raw(Box::new(table.as_ptr()));
        views().lock().unwrap().insert(object as usize, data);
        Self { object, table }
    }

    fn pointer(&self) -> std::ptr::NonNull<c_void> {
        std::ptr::NonNull::new(self.object.cast()).unwrap()
    }
}

impl Drop for FakeView {
    fn drop(&mut self) {
        views().lock().unwrap().remove(&(self.object as usize));
        // SAFETY: this fake owns the object allocation and its vtable until Drop.
        unsafe {
            drop(Box::from_raw(self.object));
        }
    }
}

fn key(seed: u128, id: u32) -> PropertyKey {
    PropertyKey {
        fmtid: windows_core::GUID::from_u128(seed),
        pid: id,
    }
}

fn fake_data(columns: Vec<SortColumn>) -> ViewData {
    ViewData {
        group_hr: 0,
        group: key(0, 0),
        count_hr: 0,
        columns_hr: 0,
        columns,
        items_hr: E_FAIL,
        last_items_flags: 0,
    }
}

#[test]
fn guid_conversion_keeps_dotnet_byte_order() {
    let value = windows_core::GUID::from_u128(0x0011_2233_4455_6677_8899_aabb_ccdd_eeff);
    assert_eq!(
        from_windows_guid(value).0,
        [
            0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff
        ]
    );
}

#[test]
fn read_sort_columns_maps_directions_and_unknown_values() {
    // C#: ReadSortColumns_MapsDirections_AndFlagsUnknownOnes
    let fake = FakeView::new(fake_data(vec![
        SortColumn {
            propkey: key(0x0011_2233_4455_6677_8899_aabb_ccdd_eeff, 7),
            direction: 1,
        },
        SortColumn {
            propkey: key(0x1020_3040_5060_7080_90a0_b0c0_d0e0_f000, 8),
            direction: -1,
        },
        SortColumn {
            propkey: key(0, 0),
            direction: 4,
        },
    ]));
    let columns = read_sort_columns(fake.pointer());
    assert_eq!(columns.len(), 3);
    assert_eq!(columns[0].property_id, 7);
    assert_eq!(columns[0].direction, ExplorerSortDirection::Ascending);
    assert_eq!(columns[1].direction, ExplorerSortDirection::Descending);
    assert_eq!(columns[2].direction, ExplorerSortDirection::Unknown);
}

#[test]
fn read_sort_columns_distinguishes_failure_from_empty() {
    // C#: ReadSortColumns_FailuresAreUnknown_AndNoSortIsEmpty
    let mut failed_count = fake_data(vec![]);
    failed_count.count_hr = E_FAIL;
    let failed_count = FakeView::new(failed_count);
    let unknown = read_sort_columns(failed_count.pointer());
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0].direction, ExplorerSortDirection::Unknown);

    let mut failed_columns = fake_data(vec![SortColumn::default()]);
    failed_columns.columns_hr = E_FAIL;
    let failed_columns = FakeView::new(failed_columns);
    assert_eq!(
        read_sort_columns(failed_columns.pointer())[0].direction,
        ExplorerSortDirection::Unknown
    );

    let empty = FakeView::new(fake_data(vec![]));
    assert!(read_sort_columns(empty.pointer()).is_empty());
}

#[test]
fn read_group_state_distinguishes_none_active_and_failed() {
    // C#: ReadGroupState_DistinguishesNoGroupingFromAFailedQuery
    let none = FakeView::new(fake_data(vec![]));
    assert_eq!(read_group_state(none.pointer()), ExplorerGroupState::None);

    let mut active_data = fake_data(vec![]);
    active_data.group = key(0x0011_2233_4455_6677_8899_aabb_ccdd_eeff, 0);
    let active = FakeView::new(active_data);
    assert_eq!(read_group_state(active.pointer()), ExplorerGroupState::Active);

    let mut failed_data = fake_data(vec![]);
    failed_data.group_hr = E_FAIL;
    let failed = FakeView::new(failed_data);
    assert_eq!(read_group_state(failed.pointer()), ExplorerGroupState::Unknown);
}

#[test]
fn batched_read_uses_view_order_flags_and_falls_back_when_items_fails() {
    // C#: PerItemRead_WhenTheEnumeratorCannotBeOpened_ReadsEveryItemOneByOne
    let fake = FakeView::new(fake_data(vec![]));
    let token = pr_core::sync::CancelToken::default();
    assert!(matches!(read_batched(fake.pointer(), "unused", 3, &token), Ok(None)));
    let flags = views().lock().unwrap()[&(fake.object as usize)].last_items_flags;
    assert_eq!(
        flags,
        explorer_com_interop::ids::SVGIO_ALL_VIEW | explorer_com_interop::ids::SVGIO_FLAG_VIEW_ORDER
    );
}

#[test]
fn display_name_reader_handles_null_and_valid_utf16() {
    let text: Vec<u16> = "C:\\photos\\a.jpg".encode_utf16().chain([0]).collect();
    assert_eq!(read_wide_string(std::ptr::null_mut()), None);
    assert_eq!(
        read_wide_string(text.as_ptr() as *mut u16).as_deref(),
        Some("C:\\photos\\a.jpg")
    );
}

#[test]
fn window_without_service_provider_is_native_view_unavailable() {
    // C#: Window_WithoutIServiceProvider_IsNativeViewUnavailable
    let mut table = Box::new([std::ptr::null(); 3]);
    table[0] = unsupported_query as *const c_void;
    let object = Box::into_raw(Box::new(table.as_ptr()));
    // SAFETY: the Box allocation above contains a live pointer to the fake IUnknown vtable.
    let view = unsafe { std::ptr::NonNull::new_unchecked(object.cast::<c_void>()) };
    let token = pr_core::sync::CancelToken::default();
    let snapshot = read_native_view(view, r"C:\photos", None, usize::MAX, &token).unwrap();
    assert_eq!(snapshot.status, ExplorerOrderStatus::NativeViewUnavailable);
    assert_eq!(
        snapshot.reason.as_deref(),
        Some("com-call-failed|QueryInterface(IServiceProvider)|0x80004002")
    );
    // SAFETY: object and vtable allocations are owned by this test and no COM reference was acquired on QI failure.
    unsafe {
        drop(Box::from_raw(object));
    }
    drop(table);
}

#[test]
fn query_canceled_before_start_does_not_open_shell_automation() {
    // C#: Query_CancelledWhileTheQueryStarts_IsCanceledWithoutOpeningTheShell
    let token = pr_core::sync::CancelToken::new();
    token.cancel();
    let query = explorer_native_view::query_shell(
        r"C:\photos",
        None,
        16,
        &token,
        std::sync::Arc::new(pr_core::abstractions::NullLog),
    )
    .unwrap();
    assert_eq!(query.status, ExplorerOrderStatus::Canceled);
    assert_eq!(query.reason.as_deref(), Some("canceled"));
}
