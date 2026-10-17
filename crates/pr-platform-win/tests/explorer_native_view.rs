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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use explorer_com_interop::{PropertyKey, SortColumn};
use explorer_native_view::{
    from_windows_guid, map_shell_open_error, read_batched, read_group_state, read_native_view, read_per_item,
    read_sort_columns, read_wide_string, select_windows,
};
use pr_core::abstractions::NullLog;
use pr_core::catalog::explorer_types::{
    ExplorerGroupState, ExplorerOrderStatus, ExplorerSortDirection, ExplorerViewSnapshot,
};
use pr_core::sync::CancelToken;
use pr_core::time::DateTimeUtc;

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
    items_return_self: bool,
    item_hr: i32,
    item_return_self: bool,
    display_name_hr: i32,
    display_name_text: Vec<u16>,
    enum_items: Vec<usize>,
    enum_position: usize,
    enum_chunk: usize,
    enum_calls: usize,
    enum_hr: i32,
    service_hr: i32,
    service_guid: Option<windows_core::GUID>,
    service_iid: Option<windows_core::GUID>,
    active_hr: i32,
    query_count: usize,
    query_fail_at: Option<usize>,
    item_count_hr: i32,
    item_count: i32,
    refs: usize,
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
    if value.items_return_self {
        value.refs += 1;
        // SAFETY: the output is writable and this fake transfers one owned reference to itself.
        unsafe {
            *output = this;
        }
    } else {
        // SAFETY: IFolderView::Items receives a writable output slot; the fake returns no enumerator.
        unsafe {
            *output = std::ptr::null_mut();
        }
    }
    value.items_hr
}

unsafe extern "system" fn release(this: *mut c_void) -> u32 {
    let mut values = views().lock().unwrap();
    let value = values.get_mut(&(this as usize)).expect("fake view is registered");
    value.refs -= 1;
    value.refs as u32
}

unsafe extern "system" fn get_item(
    this: *mut c_void,
    _index: i32,
    _iid: *const windows_core::GUID,
    output: *mut *mut c_void,
) -> i32 {
    let mut values = views().lock().unwrap();
    let value = values.get_mut(&(this as usize)).expect("fake view is registered");
    if value.item_return_self {
        value.refs += 1;
        // SAFETY: output is writable and this fake transfers one owned COM reference.
        unsafe {
            *output = this;
        }
    } else {
        // SAFETY: GetItem output is writable; this fake returns no item.
        unsafe {
            *output = std::ptr::null_mut();
        }
    }
    value.item_hr
}

unsafe extern "system" fn display_name(this: *mut c_void, _kind: u32, output: *mut *mut u16) -> i32 {
    let values = views().lock().unwrap();
    let value = &values[&(this as usize)];
    if value.display_name_hr < 0 {
        // SAFETY: GetDisplayName receives a writable name pointer output.
        unsafe {
            *output = std::ptr::null_mut();
        }
    } else {
        let bytes = value.display_name_text.len() * std::mem::size_of::<u16>();
        // SAFETY: CoTaskMemAlloc returns task-allocator memory for this COM test fake's output string.
        let allocation = unsafe { CoTaskMemAlloc(bytes) }.cast::<u16>();
        if allocation.is_null() {
            return E_FAIL;
        }
        // SAFETY: allocation has exactly enough space for the UTF-16 payload copied from the immutable fake state.
        unsafe {
            std::ptr::copy_nonoverlapping(
                value.display_name_text.as_ptr(),
                allocation,
                value.display_name_text.len(),
            );
        }
        // SAFETY: output is writable and now receives ownership of the allocated name.
        unsafe {
            *output = allocation;
        }
    }
    value.display_name_hr
}

#[repr(C)]
struct FakePidl {
    item_index: usize,
}

unsafe extern "system" fn enum_next(
    this: *mut c_void,
    requested: u32,
    output: *mut *mut c_void,
    fetched: *mut u32,
) -> i32 {
    let mut values = views().lock().unwrap();
    let value = values.get_mut(&(this as usize)).expect("fake enumerator is registered");
    value.enum_calls += 1;
    if value.enum_hr < 0 {
        // SAFETY: fetched is the writable output argument supplied by IEnumIDList::Next.
        unsafe { *fetched = 0 };
        return value.enum_hr;
    }
    let count = (requested as usize)
        .min(value.enum_chunk)
        .min(value.enum_items.len() - value.enum_position);
    for offset in 0..count {
        let item = value.enum_items[value.enum_position + offset];
        // SAFETY: CoTaskMemAlloc reserves one FakePidl and the output array has `requested` writable slots.
        let allocation = unsafe { CoTaskMemAlloc(std::mem::size_of::<FakePidl>()) }.cast::<FakePidl>();
        if allocation.is_null() {
            // SAFETY: fetched is a writable IEnumIDList::Next output.
            unsafe { *fetched = offset as u32 };
            return E_FAIL;
        }
        // SAFETY: allocation is aligned and large enough for FakePidl.
        unsafe { allocation.write(FakePidl { item_index: item }) };
        // SAFETY: output points to `requested` writable pointer slots as provided by enum_next.
        unsafe { *output.add(offset) = allocation.cast() };
    }
    value.enum_position += count;
    // SAFETY: fetched is a writable IEnumIDList::Next output.
    unsafe { *fetched = count as u32 };
    if count < requested as usize {
        explorer_com_interop::ids::S_FALSE
    } else {
        0
    }
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoTaskMemAlloc(bytes: usize) -> *mut c_void;
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

unsafe extern "system" fn partial_query(
    this: *mut c_void,
    _iid: *const windows_core::GUID,
    output: *mut *mut c_void,
) -> i32 {
    let mut values = views().lock().unwrap();
    let value = values.get_mut(&(this as usize)).expect("fake view is registered");
    value.refs += 1;
    // SAFETY: the test fake writes the returned owned reference to QueryInterface's writable output slot.
    unsafe {
        *output = this;
    }
    E_FAIL
}

unsafe extern "system" fn self_query(
    this: *mut c_void,
    _iid: *const windows_core::GUID,
    output: *mut *mut c_void,
) -> i32 {
    let mut values = views().lock().unwrap();
    let value = values.get_mut(&(this as usize)).expect("fake view is registered");
    value.query_count += 1;
    if value.query_fail_at == Some(value.query_count) {
        // SAFETY: the output slot is writable and a failed QI returns no interface pointer.
        unsafe { *output = std::ptr::null_mut() };
        return 0x8000_4002_u32 as i32;
    }
    value.refs += 1;
    // SAFETY: the fake returns itself with one owned reference through QueryInterface's output slot.
    unsafe { *output = this };
    0
}

unsafe extern "system" fn query_service(
    this: *mut c_void,
    service: *const windows_core::GUID,
    iid: *const windows_core::GUID,
    output: *mut *mut c_void,
) -> i32 {
    let mut values = views().lock().unwrap();
    let value = values
        .get_mut(&(this as usize))
        .expect("fake service provider is registered");
    // SAFETY: IServiceProvider provides valid GUID pointers and a writable output slot for the call duration.
    value.service_guid = Some(unsafe { *service });
    // SAFETY: the IID pointer remains valid through the synchronous call.
    value.service_iid = Some(unsafe { *iid });
    if value.service_hr >= 0 {
        value.refs += 1;
        // SAFETY: the output slot is writable and receives one owned COM reference on success.
        unsafe { *output = this };
    } else {
        // SAFETY: the output slot is writable; this fake returns null on its configured failure.
        unsafe { *output = std::ptr::null_mut() };
    }
    value.service_hr
}

unsafe extern "system" fn active_view_failure(this: *mut c_void, output: *mut *mut c_void) -> i32 {
    let values = views().lock().unwrap();
    let value = &values[&(this as usize)];
    // SAFETY: IShellBrowser provides a writable active-view output slot.
    unsafe { *output = std::ptr::null_mut() };
    value.active_hr
}

unsafe extern "system" fn active_view_success(this: *mut c_void, output: *mut *mut c_void) -> i32 {
    let mut values = views().lock().unwrap();
    let value = values.get_mut(&(this as usize)).expect("fake browser is registered");
    value.refs += 1;
    // SAFETY: IShellBrowser provides a writable output slot, and this fake transfers one owned reference.
    unsafe { *output = this };
    0
}

unsafe extern "system" fn view_item_count(this: *mut c_void, _flags: u32, output: *mut i32) -> i32 {
    let values = views().lock().unwrap();
    let value = &values[&(this as usize)];
    // SAFETY: IFolderView::ItemCount provides a writable count output.
    unsafe { *output = value.item_count };
    value.item_count_hr
}

struct FakeView {
    object: *mut *const *const c_void,
    table: Box<[*const c_void; 30]>,
}

impl FakeView {
    fn new(data: ViewData) -> Self {
        let mut table = Box::new([std::ptr::null(); 30]);
        table[2] = release as *const c_void;
        table[3] = enum_next as *const c_void;
        table[15] = active_view_failure as *const c_void;
        table[5] = display_name as *const c_void;
        table[7] = view_item_count as *const c_void;
        table[18] = group_by as *const c_void;
        table[26] = sort_count as *const c_void;
        table[28] = sort_columns as *const c_void;
        table[8] = items as *const c_void;
        table[29] = get_item as *const c_void;
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
        items_return_self: false,
        item_hr: E_FAIL,
        item_return_self: false,
        display_name_hr: E_FAIL,
        display_name_text: Vec::new(),
        enum_items: Vec::new(),
        enum_position: 0,
        enum_chunk: usize::MAX,
        enum_calls: 0,
        enum_hr: 0,
        service_hr: E_FAIL,
        service_guid: None,
        service_iid: None,
        active_hr: E_FAIL,
        query_count: 0,
        query_fail_at: None,
        item_count_hr: 0,
        item_count: 0,
        refs: 1,
    }
}

struct FakePidlApi {
    paths: Vec<String>,
    parses: bool,
}

impl explorer_native_view::PidlApi for FakePidlApi {
    fn parse_folder(&self, _folder: &str) -> Option<std::ptr::NonNull<c_void>> {
        if !self.parses {
            return None;
        }
        // SAFETY: CoTaskMemAlloc creates a task-allocator-owned fake parent PIDL.
        let pointer = unsafe { CoTaskMemAlloc(std::mem::size_of::<FakePidl>()) }.cast::<FakePidl>();
        if pointer.is_null() {
            return None;
        }
        // SAFETY: pointer is aligned and large enough for FakePidl.
        unsafe { pointer.write(FakePidl { item_index: usize::MAX }) };
        std::ptr::NonNull::new(pointer.cast())
    }

    fn combine(
        &self,
        _parent: std::ptr::NonNull<c_void>,
        child: std::ptr::NonNull<c_void>,
    ) -> Option<std::ptr::NonNull<c_void>> {
        // SAFETY: test enumerator supplies a live CoTaskMem-allocated FakePidl child.
        let item_index = unsafe { child.cast::<FakePidl>().as_ref().item_index };
        // SAFETY: CoTaskMemAlloc creates a task-allocator-owned fake absolute PIDL.
        let pointer = unsafe { CoTaskMemAlloc(std::mem::size_of::<FakePidl>()) }.cast::<FakePidl>();
        if pointer.is_null() {
            return None;
        }
        // SAFETY: pointer is aligned and large enough for FakePidl.
        unsafe { pointer.write(FakePidl { item_index }) };
        std::ptr::NonNull::new(pointer.cast())
    }

    fn file_system_path(&self, absolute: std::ptr::NonNull<c_void>) -> Option<std::ptr::NonNull<u16>> {
        // SAFETY: test combiner supplies a live CoTaskMem-allocated FakePidl absolute PIDL.
        let item_index = unsafe { absolute.cast::<FakePidl>().as_ref().item_index };
        let path: Vec<u16> = self.paths.get(item_index)?.encode_utf16().chain([0]).collect();
        // SAFETY: CoTaskMemAlloc reserves enough space for the path including its terminator.
        let pointer = unsafe { CoTaskMemAlloc(path.len() * std::mem::size_of::<u16>()) }.cast::<u16>();
        if pointer.is_null() {
            return None;
        }
        // SAFETY: pointer has `path.len()` writable UTF-16 units.
        unsafe { std::ptr::copy_nonoverlapping(path.as_ptr(), pointer, path.len()) };
        std::ptr::NonNull::new(pointer)
    }
}

struct FakeSelectionWindow {
    location: Option<String>,
    snapshot: ExplorerViewSnapshot,
    releases: Arc<AtomicUsize>,
}

impl explorer_window_selector::ExplorerSelectionWindow for FakeSelectionWindow {
    fn location_url(&self) -> Result<Option<String>, explorer_window_selector::LocationReadError> {
        Ok(self.location.clone())
    }

    fn read(
        &self,
        _folder: &str,
        _cancellation: &CancelToken,
    ) -> Result<ExplorerViewSnapshot, explorer_window_selector::SelectionReadError> {
        Ok(self.snapshot.clone())
    }

    fn release(&self) {
        self.releases.fetch_add(1, Ordering::SeqCst);
    }
}

fn selection_window(location: &str, path: &str) -> (FakeSelectionWindow, Arc<AtomicUsize>) {
    let releases = Arc::new(AtomicUsize::new(0));
    let window = FakeSelectionWindow {
        location: Some(location.into()),
        snapshot: ExplorerViewSnapshot {
            folder: r"C:\photos".into(),
            ordered_paths: vec![path.into()],
            sort_columns: vec![],
            group_state: ExplorerGroupState::None,
            status: ExplorerOrderStatus::Available,
            reason: None,
            captured_utc: DateTimeUtc::MIN,
        },
        releases: releases.clone(),
    };
    (window, releases)
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

    let invalid_count = FakeView::new(fake_data(vec![SortColumn::default(); 33]));
    assert_eq!(
        read_sort_columns(invalid_count.pointer())[0].direction,
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
fn batched_read_collects_short_s_false_chunks_in_view_order() {
    // C#: BatchedRead_EnumeratorHandsOutItemsInSeveralCalls_CollectsAllOfThem
    let paths = vec![
        r"C:\photos\c.jpg".into(),
        r"C:\photos\a.jpg".into(),
        r"C:\photos\b.jpg".into(),
    ];
    let mut data = fake_data(vec![]);
    data.items_hr = 0;
    data.items_return_self = true;
    data.enum_items = vec![0, 1, 2];
    data.enum_chunk = 1;
    let fake = FakeView::new(data);
    let token = CancelToken::new();

    let result = explorer_native_view::read_batched_with(
        fake.pointer(),
        r"C:\photos",
        paths.len(),
        &token,
        &FakePidlApi {
            paths: paths.clone(),
            parses: true,
        },
    )
    .unwrap()
    .unwrap();

    assert_eq!(result, paths);
    let values = views().lock().unwrap();
    let value = &values[&(fake.object as usize)];
    assert_eq!(value.enum_calls, 4); // three S_FALSE chunks followed by the terminal empty read
    assert_eq!(value.refs, 1);
}

#[test]
fn batched_read_rejects_short_enumeration_so_caller_can_fallback() {
    // C#: PerItemRead_WhenTheEnumeratorYieldsFewerItemsThanTheView_FallsBackAndIsComplete
    let paths = vec![
        r"C:\photos\a.jpg".into(),
        r"C:\photos\b.jpg".into(),
        r"C:\photos\c.jpg".into(),
    ];
    let mut data = fake_data(vec![]);
    data.items_hr = 0;
    data.items_return_self = true;
    data.enum_items = vec![0, 1];
    data.enum_chunk = 1;
    let fake = FakeView::new(data);
    let token = CancelToken::new();

    let batched = explorer_native_view::read_batched_with(
        fake.pointer(),
        r"C:\photos",
        paths.len(),
        &token,
        &FakePidlApi { paths, parses: true },
    )
    .unwrap();

    assert_eq!(batched, None);
    assert_eq!(views().lock().unwrap()[&(fake.object as usize)].refs, 1);
}

#[test]
fn batched_read_falls_back_when_folder_pidls_cannot_be_parsed() {
    // C#: PerItemRead_WhenTheFolderCannotBeParsedToAPidl_FallsBack
    let mut data = fake_data(vec![]);
    data.items_hr = 0;
    data.items_return_self = true;
    let fake = FakeView::new(data);
    let token = CancelToken::new();

    let result = explorer_native_view::read_batched_with(
        fake.pointer(),
        r"C:\photos\missing",
        2,
        &token,
        &FakePidlApi {
            paths: Vec::new(),
            parses: false,
        },
    )
    .unwrap();

    assert_eq!(result, None);
    assert_eq!(views().lock().unwrap()[&(fake.object as usize)].refs, 1);
}

#[test]
fn batched_read_falls_back_when_enumerator_next_fails() {
    // C#: PerItemRead_WhenTheEnumeratorFails_FallsBack
    let mut data = fake_data(vec![]);
    data.items_hr = 0;
    data.items_return_self = true;
    data.enum_hr = E_FAIL;
    let fake = FakeView::new(data);
    let token = CancelToken::new();

    let result = explorer_native_view::read_batched_with(
        fake.pointer(),
        r"C:\photos",
        2,
        &token,
        &FakePidlApi {
            paths: vec![r"C:\photos\a.jpg".into(), r"C:\photos\b.jpg".into()],
            parses: true,
        },
    )
    .unwrap();

    assert_eq!(result, None);
    assert_eq!(views().lock().unwrap()[&(fake.object as usize)].refs, 1);
}

#[test]
fn failed_items_call_releases_a_non_null_partial_output() {
    let mut data = fake_data(vec![]);
    data.items_return_self = true;
    data.items_hr = E_FAIL;
    let fake = FakeView::new(data);
    let token = CancelToken::new();
    assert!(matches!(read_batched(fake.pointer(), "unused", 2, &token), Ok(None)));
    assert_eq!(views().lock().unwrap()[&(fake.object as usize)].refs, 1);
}

#[test]
fn batched_read_cancellation_before_first_batch_does_not_open_enumerator() {
    // C#: BatchedRead_CancelledBeforeTheFirstBatch_Throws
    let fake = FakeView::new(fake_data(vec![]));
    let token = CancelToken::new();
    token.cancel();

    let error = read_batched(fake.pointer(), r"C:\photos", 2, &token).unwrap_err();

    assert!(error.canceled);
    let values = views().lock().unwrap();
    assert_eq!(values[&(fake.object as usize)].last_items_flags, 0);
    assert_eq!(values[&(fake.object as usize)].refs, 1);
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
fn per_item_read_reports_get_item_failure_and_releases_partial_reference() {
    // C#: PerItemRead_GetItemFails_ReportsTheFailingIndexAndStops
    let mut data = fake_data(vec![]);
    data.item_return_self = true;
    data.item_hr = E_FAIL;
    let fake = FakeView::new(data);
    let token = pr_core::sync::CancelToken::new();
    let error = read_per_item(fake.pointer(), r"C:\photos", 4, usize::MAX, None, &token).unwrap_err();
    assert_eq!(error.type_name, "IFolderView2.GetItem(0)");
    assert_eq!(error.hresult, E_FAIL);
    assert_eq!(views().lock().unwrap()[&(fake.object as usize)].refs, 1);
}

#[test]
fn per_item_read_reports_display_name_failure_and_releases_item() {
    // C#: PerItemRead_GetDisplayNameFails_ReportsItAndStops
    let mut data = fake_data(vec![]);
    data.item_return_self = true;
    data.item_hr = 0;
    data.display_name_hr = E_FAIL;
    let fake = FakeView::new(data);
    let token = pr_core::sync::CancelToken::new();
    let error = read_per_item(fake.pointer(), r"C:\photos", 2, usize::MAX, None, &token).unwrap_err();
    assert_eq!(error.type_name, "IShellItem.GetDisplayName(0)");
    assert_eq!(error.hresult, E_FAIL);
    assert_eq!(views().lock().unwrap()[&(fake.object as usize)].refs, 1);
}

#[test]
fn per_item_read_returns_paths_and_reports_incremental_progress() {
    // C#: PerItemRead_WhenTheEnumeratorCannotBeOpened_ReadsEveryItemOneByOne
    let mut data = fake_data(vec![]);
    data.item_return_self = true;
    data.item_hr = 0;
    data.display_name_hr = 0;
    data.display_name_text = "C:\\photos\\one.jpg\0".encode_utf16().collect();
    let fake = FakeView::new(data);
    let token = pr_core::sync::CancelToken::new();
    let progress = Mutex::new(Vec::new());
    let paths = read_per_item(
        fake.pointer(),
        r"C:\photos",
        1,
        1,
        Some(&|event| progress.lock().unwrap().push(event)),
        &token,
    )
    .unwrap();
    assert_eq!(paths, [r"C:\photos\one.jpg"]);
    let progress = progress.lock().unwrap();
    assert_eq!(progress.len(), 1);
    assert_eq!(progress[0].items_read, 1);
    assert_eq!(progress[0].item_count, 1);
    assert_eq!(views().lock().unwrap()[&(fake.object as usize)].refs, 1);
}

#[test]
fn per_item_read_cancellation_releases_the_current_shell_item() {
    // C#: PerItemRead_CancelledMidWay_ThrowsAndStillReleasesEverything
    let mut data = fake_data(vec![]);
    data.item_return_self = true;
    data.item_hr = 0;
    data.display_name_hr = 0;
    data.display_name_text = "C:\\photos\\one.jpg\0".encode_utf16().collect();
    let fake = FakeView::new(data);
    let token = pr_core::sync::CancelToken::new();
    let cancellation = token.clone();
    let error = read_per_item(
        fake.pointer(),
        r"C:\photos",
        2,
        1,
        Some(&move |event| {
            if event.items_read == 1 {
                cancellation.cancel();
            }
        }),
        &token,
    )
    .unwrap_err();
    assert!(error.canceled);
    assert_eq!(views().lock().unwrap()[&(fake.object as usize)].refs, 1);
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
fn failed_service_provider_query_releases_a_non_null_partial_output() {
    let data = fake_data(vec![]);
    let mut table = Box::new([std::ptr::null(); 30]);
    table[0] = partial_query as *const c_void;
    table[2] = release as *const c_void;
    let object = Box::into_raw(Box::new(table.as_ptr()));
    let pointer = std::ptr::NonNull::new(object.cast::<c_void>()).unwrap();
    views().lock().unwrap().insert(object as usize, data.clone());
    let snapshot = read_native_view(pointer, r"C:\photos", None, usize::MAX, &CancelToken::new()).unwrap();
    assert_eq!(snapshot.status, ExplorerOrderStatus::NativeViewUnavailable);
    assert_eq!(views().lock().unwrap()[&(object as usize)].refs, 1);
    views().lock().unwrap().remove(&(object as usize));
    // SAFETY: test owns both heap allocations and no COM refs remain after the failure path.
    unsafe {
        drop(Box::from_raw(object));
    }
    drop(table);
}

#[test]
fn window_browser_service_failure_reports_query_service_and_requests_top_level_browser() {
    // C#: Window_BrowserServiceFails_ReportsQueryService
    let fake = FakeView::new(fake_data(vec![]));
    fake.table[0] = self_query as *const c_void;
    fake.table[3] = query_service as *const c_void;

    let snapshot = read_native_view(fake.pointer(), r"C:\photos", None, usize::MAX, &CancelToken::new()).unwrap();

    assert_eq!(snapshot.status, ExplorerOrderStatus::NativeViewUnavailable);
    assert_eq!(
        snapshot.reason.as_deref(),
        Some("com-call-failed|QueryService(IShellBrowser)|0x80004005")
    );
    let values = views().lock().unwrap();
    let value = &values[&(fake.object as usize)];
    assert_eq!(
        value.service_guid,
        Some(explorer_com_interop::ids::SID_TOP_LEVEL_BROWSER)
    );
    assert_eq!(value.service_iid, Some(explorer_com_interop::ids::IID_SHELL_BROWSER));
    assert_eq!(value.refs, 1);
}

#[test]
fn window_active_shell_view_failure_reports_the_exact_com_stage() {
    // C#: Window_ActiveShellViewFails_ReportsIt
    let mut data = fake_data(vec![]);
    data.service_hr = 0;
    let fake = FakeView::new(data);
    fake.table[0] = self_query as *const c_void;
    fake.table[3] = query_service as *const c_void;

    let snapshot = read_native_view(fake.pointer(), r"C:\photos", None, usize::MAX, &CancelToken::new()).unwrap();

    assert_eq!(snapshot.status, ExplorerOrderStatus::NativeViewUnavailable);
    assert_eq!(
        snapshot.reason.as_deref(),
        Some("com-call-failed|QueryActiveShellView|0x80004005")
    );
    assert_eq!(views().lock().unwrap()[&(fake.object as usize)].refs, 1);
}

#[test]
fn window_without_folder_view2_reports_interface_query_failure() {
    // C#: Window_ViewWithoutIFolderView2_ReportsIt
    let mut data = fake_data(vec![]);
    data.service_hr = 0;
    data.query_fail_at = Some(2);
    let fake = FakeView::new(data);
    fake.table[0] = self_query as *const c_void;
    fake.table[3] = query_service as *const c_void;
    fake.table[15] = active_view_success as *const c_void;

    let snapshot = read_native_view(fake.pointer(), r"C:\photos", None, usize::MAX, &CancelToken::new()).unwrap();

    assert_eq!(snapshot.status, ExplorerOrderStatus::NativeViewUnavailable);
    assert_eq!(
        snapshot.reason.as_deref(),
        Some("com-call-failed|QueryInterface(IFolderView2)|0x80004002")
    );
    assert_eq!(views().lock().unwrap()[&(fake.object as usize)].refs, 1);
}

#[test]
fn window_empty_or_uncountable_view_is_reported_as_empty() {
    // C#: Window_EmptyOrUncountableView_IsReportedAsEmptyView
    let mut empty_data = fake_data(vec![]);
    empty_data.service_hr = 0;
    empty_data.item_count = 0;
    let empty = FakeView::new(empty_data);
    empty.table[0] = self_query as *const c_void;
    empty.table[3] = query_service as *const c_void;
    empty.table[15] = active_view_success as *const c_void;
    let empty_snapshot =
        read_native_view(empty.pointer(), r"C:\photos", None, usize::MAX, &CancelToken::new()).unwrap();
    assert_eq!(empty_snapshot.reason.as_deref(), Some("empty-view|0x00000000|0"));
    assert_eq!(views().lock().unwrap()[&(empty.object as usize)].refs, 1);

    let mut failed_data = fake_data(vec![]);
    failed_data.service_hr = 0;
    failed_data.item_count_hr = E_FAIL;
    let failed = FakeView::new(failed_data);
    failed.table[0] = self_query as *const c_void;
    failed.table[3] = query_service as *const c_void;
    failed.table[15] = active_view_success as *const c_void;
    let failed_snapshot =
        read_native_view(failed.pointer(), r"C:\photos", None, usize::MAX, &CancelToken::new()).unwrap();
    assert_eq!(failed_snapshot.reason.as_deref(), Some("empty-view|0x80004005|0"));
    assert_eq!(views().lock().unwrap()[&(failed.object as usize)].refs, 1);
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

#[test]
fn query_picks_the_window_showing_the_requested_folder() {
    // C#: Query_PicksTheWindowShowingTheFolderAndReadsItsView
    let (other, other_releases) = selection_window("file:///C:/other", r"C:\other\x.jpg");
    let (matching, matching_releases) = selection_window("file:///C:/photos", r"C:\photos\one.jpg");
    let windows = vec![other, matching]
        .into_iter()
        .map(Ok::<_, Box<dyn std::error::Error + Send + Sync>>);
    let snapshot = select_windows(r"C:\photos", windows, &NullLog, &CancelToken::new());
    assert_eq!(snapshot.status, ExplorerOrderStatus::Available);
    assert_eq!(snapshot.ordered_paths, [r"C:\photos\one.jpg"]);
    assert_eq!(other_releases.load(Ordering::SeqCst), 1);
    assert_eq!(matching_releases.load(Ordering::SeqCst), 1);
}

#[test]
fn query_without_matching_window_reports_no_matching_window() {
    // C#: Query_NoWindowShowsTheFolder_IsNoMatchingWindow
    let (other, releases) = selection_window("file:///C:/other", r"C:\other\x.jpg");
    let windows = vec![other]
        .into_iter()
        .map(Ok::<_, Box<dyn std::error::Error + Send + Sync>>);
    let snapshot = select_windows(r"C:\photos", windows, &NullLog, &CancelToken::new());
    assert_eq!(snapshot.status, ExplorerOrderStatus::NoMatchingWindow);
    assert_eq!(snapshot.reason.as_deref(), Some("no-matching-window|1"));
    assert_eq!(releases.load(Ordering::SeqCst), 1);
}

#[test]
fn query_maps_shell_automation_missing_and_opening_failures() {
    // C#: Query_ShellAutomationMissing_IsNativeViewUnavailable
    // C#: Query_OpeningTheShellThrows_IsReportedAsFailed
    let unavailable = map_shell_open_error(r"C:\photos", explorer_com_interop::ids::CLASS_NOT_REGISTERED).unwrap();
    assert_eq!(unavailable.status, ExplorerOrderStatus::NativeViewUnavailable);
    assert_eq!(unavailable.reason.as_deref(), Some("shell-unavailable"));

    let failure = map_shell_open_error(r"C:\photos", E_FAIL).unwrap_err();
    assert!(failure.shell_open);
    assert_eq!(failure.hresult, Some(E_FAIL));
}
