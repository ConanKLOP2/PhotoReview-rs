//! Port of `PhotoReview.Platform.Windows/Explorer/ExplorerComInterop.cs` (C# baseline d92d6aae).
//!
//! Raw shell interfaces are called through explicit SDK vtable slots. This avoids runtime-generated dispatch and keeps
//! reference ownership visible at every boundary. The old `ComEnumeration` abstraction is intentionally not present.

// Justification: W6-01d consumes the remaining slots after this independently owned unit is integrated.
#![allow(dead_code)]

use std::ffi::c_void;
use std::fmt;
use std::iter::FusedIterator;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicPtr, Ordering};

use windows_core::{BSTR, GUID};

/// COM identity and shell constants pinned to the C# baseline.
pub(crate) mod ids {
    use windows_core::GUID;

    pub(crate) const SID_TOP_LEVEL_BROWSER: GUID = GUID::from_u128(0x4c96_be40_915c_11cf_99d3_00aa_004a_e837);
    pub(crate) const IID_SERVICE_PROVIDER: GUID = GUID::from_u128(0x6d51_40c1_7436_11ce_8034_00aa_0060_09fa);
    pub(crate) const IID_SHELL_BROWSER: GUID = GUID::from_u128(0x0002_14e2_0000_0000_c000_0000_0000_0046);
    pub(crate) const IID_FOLDER_VIEW2: GUID = GUID::from_u128(0x1af3_a467_214f_4298_908e_06b0_3e0b_39f9);
    pub(crate) const IID_SHELL_ITEM: GUID = GUID::from_u128(0x4382_6d1e_e718_42ee_bc55_a1e2_61c3_7bfe);
    pub(crate) const IID_ENUM_ID_LIST: GUID = GUID::from_u128(0x0002_14f2_0000_0000_c000_0000_0000_0046);
    pub(crate) const CLSID_SHELL_WINDOWS: GUID = GUID::from_u128(0x9ba0_5972_f6a8_11cf_a442_00a0_c90a_8f39);
    pub(crate) const IID_SHELL_WINDOWS: GUID = GUID::from_u128(0x85cb_6900_4d95_11cf_960c_0080_c7f4_ee85);
    pub(crate) const IID_WEB_BROWSER_APP: GUID = GUID::from_u128(0x0002_df05_0000_0000_c000_0000_0000_0046);
    pub(crate) const CLASS_NOT_REGISTERED: i32 = 0x8004_0154_u32 as i32;
    pub(crate) const SVGIO_ALL_VIEW: u32 = 2;
    pub(crate) const SVGIO_FLAG_VIEW_ORDER: u32 = 0x8000_0000;
    pub(crate) const SIGDN_FILE_SYSTEM_PATH: u32 = 0x8005_8000;
    pub(crate) const S_FALSE: i32 = 1;
}

// Source-generated shell32 entry points declared by the C# interop source.
#[link(name = "shell32")]
unsafe extern "system" {
    #[link_name = "SHParseDisplayName"]
    pub(crate) fn sh_parse_display_name(
        name: *const u16,
        bind_context: *mut c_void,
        pidl: *mut *mut c_void,
        sfgao_in: u32,
        sfgao_out: *mut u32,
    ) -> i32;
    #[link_name = "ILCombine"]
    pub(crate) fn il_combine(parent: *mut c_void, child: *mut c_void) -> *mut c_void;
    #[link_name = "SHGetNameFromIDList"]
    pub(crate) fn sh_get_name_from_id_list(pidl: *mut c_void, sigdn: u32, name: *mut *mut u16) -> i32;
}

/// A failed shell HRESULT. The original value is retained so callers can preserve C# status classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ShellComError(pub(crate) i32);

impl ShellComError {
    pub(crate) const fn hresult(self) -> i32 {
        self.0
    }
}

impl fmt::Display for ShellComError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "shell COM call failed (HRESULT 0x{:08X})", self.0 as u32)
    }
}

impl std::error::Error for ShellComError {}

type ShellResult<T> = Result<T, ShellComError>;

/// Minimal raw-pointer boundary ported from `ShellCom.Check/CreateInstance` for this module's owned source.
pub(crate) struct ShellCom;

impl ShellCom {
    pub(crate) const CLSCTX_ALL: u32 = 0x17;

    pub(crate) fn check(hr: i32) -> ShellResult<()> {
        if hr < 0 { Err(ShellComError(hr)) } else { Ok(()) }
    }

    pub(crate) fn create_instance(clsid: &GUID, iid: &GUID) -> ShellResult<NonNull<c_void>> {
        let mut instance = std::ptr::null_mut();
        // SAFETY: CoCreateInstance is called with valid GUID references, no aggregation, and a writable out pointer.
        let hr = unsafe { CoCreateInstance(clsid, std::ptr::null_mut(), Self::CLSCTX_ALL, iid, &mut instance) };
        if let Err(error) = Self::check(hr) {
            if let Some(instance) = NonNull::new(instance) {
                // SAFETY: any non-null CoCreateInstance out pointer owns one reference, including on failure.
                unsafe { Self::release(instance) };
            }
            return Err(error);
        }
        NonNull::new(instance).ok_or(ShellComError(E_FAIL))
    }

    pub(crate) unsafe fn add_ref(pointer: NonNull<c_void>) {
        // SAFETY: caller guarantees `pointer` is a live COM interface pointer with the standard IUnknown prefix.
        let vtable = unsafe { *(pointer.as_ptr().cast::<*const *const c_void>()) };
        // SAFETY: IUnknown::AddRef is slot 1 in every COM interface vtable.
        let add_ref: unsafe extern "system" fn(*mut c_void) -> u32 = unsafe { std::mem::transmute(*vtable.add(1)) };
        // SAFETY: the live pointer and function address were obtained from its own vtable.
        unsafe { add_ref(pointer.as_ptr()) };
    }

    pub(crate) unsafe fn release(pointer: NonNull<c_void>) {
        // SAFETY: caller guarantees `pointer` is a live COM interface pointer with the standard IUnknown prefix.
        let vtable = unsafe { *(pointer.as_ptr().cast::<*const *const c_void>()) };
        // SAFETY: IUnknown::Release is slot 2 in every COM interface vtable.
        let release: unsafe extern "system" fn(*mut c_void) -> u32 = unsafe { std::mem::transmute(*vtable.add(2)) };
        // SAFETY: this releases exactly the reference owned by the caller.
        unsafe { release(pointer.as_ptr()) };
    }

    pub(crate) fn query_interface(pointer: NonNull<c_void>, iid: &GUID) -> ShellResult<NonNull<c_void>> {
        let mut result = std::ptr::null_mut();
        // SAFETY: pointer is an owned live COM pointer; iid and result are valid for QueryInterface's call duration.
        let hr = unsafe {
            let vtable = *(pointer.as_ptr().cast::<*const *const c_void>());
            let query: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32 =
                std::mem::transmute(*vtable);
            query(pointer.as_ptr(), iid, &mut result)
        };
        if let Err(error) = Self::check(hr) {
            if let Some(result) = NonNull::new(result) {
                // SAFETY: any non-null QueryInterface out pointer owns one reference, including on failure.
                unsafe { Self::release(result) };
            }
            return Err(error);
        }
        NonNull::new(result).ok_or(ShellComError(E_FAIL))
    }
}

const E_FAIL: i32 = 0x8000_4005_u32 as i32;

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoCreateInstance(
        clsid: *const GUID,
        outer: *mut c_void,
        context: u32,
        iid: *const GUID,
        result: *mut *mut c_void,
    ) -> i32;
}

/// Owned reference to an arbitrary COM interface. Dropping releases exactly one reference.
#[derive(Debug)]
pub(crate) struct OwnedUnknown(NonNull<c_void>);

impl OwnedUnknown {
    pub(crate) fn as_raw(&self) -> *mut c_void {
        self.0.as_ptr()
    }
    pub(crate) fn into_raw(self) -> *mut c_void {
        let pointer = self.0.as_ptr();
        std::mem::forget(self);
        pointer
    }
}

impl Drop for OwnedUnknown {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns exactly one live COM reference.
        unsafe { ShellCom::release(self.0) }
    }
}

/// One Explorer window. Disposing releases its owned IDispatch reference once.
pub(crate) trait IExplorerWindow {
    fn location_url(&self) -> ShellResult<Option<String>>;
    fn acquire_unknown(&self) -> ShellResult<OwnedUnknown>;
    fn dispose(&self);
}

/// The open Explorer window collection. Iteration is lazy and each result owns its returned dispatch pointer.
pub(crate) trait IExplorerWindowSource {
    type Windows<'a>: Iterator<Item = ShellResult<ShellWebBrowser>> + 'a
    where
        Self: 'a;
    fn windows(&self) -> Self::Windows<'_>;
}

/// Owns the `IShellWindows` reference returned by `CoCreateInstance`.
pub(crate) struct ShellWindowsSource {
    shell_windows: AtomicPtr<c_void>,
}

impl ShellWindowsSource {
    pub(crate) fn open() -> ShellResult<Self> {
        let pointer = ShellCom::create_instance(&ids::CLSID_SHELL_WINDOWS, &ids::IID_SHELL_WINDOWS)?;
        Ok(Self::from_owned(pointer))
    }

    fn from_owned(pointer: NonNull<c_void>) -> Self {
        Self {
            shell_windows: AtomicPtr::new(pointer.as_ptr()),
        }
    }

    fn pointer(&self) -> ShellResult<NonNull<c_void>> {
        NonNull::new(self.shell_windows.load(Ordering::Acquire)).ok_or(ShellComError(COR_E_OBJECTDISPOSED))
    }

    pub(crate) fn dispose(&self) {
        let pointer = self.shell_windows.swap(std::ptr::null_mut(), Ordering::AcqRel);
        if let Some(pointer) = NonNull::new(pointer) {
            // SAFETY: the swap transfers the source's one owned reference to this call exactly once.
            unsafe { ShellCom::release(pointer) }
        }
    }
}

impl IExplorerWindowSource for ShellWindowsSource {
    type Windows<'a> = ShellWindowsIter<'a>;

    fn windows(&self) -> Self::Windows<'_> {
        ShellWindowsIter {
            source: self,
            count: None,
            index: 0,
            failed: false,
        }
    }
}

impl Drop for ShellWindowsSource {
    fn drop(&mut self) {
        self.dispose();
    }
}

/// Lazy `get_Count`/`Item` enumeration. A failing HRESULT ends iteration and is returned once.
pub(crate) struct ShellWindowsIter<'a> {
    source: &'a ShellWindowsSource,
    count: Option<i32>,
    index: i32,
    failed: bool,
}

impl Iterator for ShellWindowsIter<'_> {
    type Item = ShellResult<ShellWebBrowser>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        if self.count.is_none() {
            let pointer = match self.source.pointer() {
                Ok(pointer) => pointer,
                Err(error) => {
                    self.failed = true;
                    return Some(Err(error));
                }
            };
            match ExplorerNativeVtable::shell_windows_count(pointer) {
                Ok(count) => self.count = Some(count),
                Err(error) => {
                    self.failed = true;
                    return Some(Err(error));
                }
            }
        }
        while self.index < self.count.unwrap_or_default() {
            let index = self.index;
            self.index += 1;
            let pointer = match self.source.pointer() {
                Ok(pointer) => pointer,
                Err(error) => {
                    self.failed = true;
                    return Some(Err(error));
                }
            };
            match ExplorerNativeVtable::shell_windows_item(pointer, index) {
                Ok(None) => continue,
                Ok(Some(dispatch)) => {
                    return Some(Ok(ShellWebBrowser {
                        dispatch: AtomicPtr::new(dispatch.as_ptr()),
                    }));
                }
                Err(error) => {
                    self.failed = true;
                    return Some(Err(error));
                }
            }
        }
        None
    }
}

impl FusedIterator for ShellWindowsIter<'_> {}

/// An `IWebBrowser2` dispatch reference returned by `IShellWindows::Item`.
#[derive(Debug)]
pub(crate) struct ShellWebBrowser {
    dispatch: AtomicPtr<c_void>,
}

impl ShellWebBrowser {
    fn pointer(&self) -> ShellResult<NonNull<c_void>> {
        NonNull::new(self.dispatch.load(Ordering::Acquire)).ok_or(ShellComError(COR_E_OBJECTDISPOSED))
    }

    pub(crate) fn dispose(&self) {
        let pointer = self.dispatch.swap(std::ptr::null_mut(), Ordering::AcqRel);
        if let Some(pointer) = NonNull::new(pointer) {
            // SAFETY: the swap transfers this wrapper's owned dispatch reference to this call exactly once.
            unsafe { ShellCom::release(pointer) }
        }
    }
}

impl IExplorerWindow for ShellWebBrowser {
    fn location_url(&self) -> ShellResult<Option<String>> {
        let dispatch = self.pointer()?;
        let app = ShellCom::query_interface(dispatch, &ids::IID_WEB_BROWSER_APP)?;
        let result = ExplorerNativeVtable::web_browser_location_url(app);
        // SAFETY: QueryInterface returned an owned reference, released whether the property call succeeds or fails.
        unsafe { ShellCom::release(app) };
        result
    }

    fn acquire_unknown(&self) -> ShellResult<OwnedUnknown> {
        let dispatch = self.pointer()?;
        // SAFETY: pointer is live and owned by self; AddRef creates the new caller-owned reference.
        unsafe { ShellCom::add_ref(dispatch) };
        Ok(OwnedUnknown(dispatch))
    }

    fn dispose(&self) {
        ShellWebBrowser::dispose(self)
    }
}

impl Drop for ShellWebBrowser {
    fn drop(&mut self) {
        self.dispose();
    }
}

const COR_E_OBJECTDISPOSED: i32 = 0x8013_1622_u32 as i32;
const VT_I4: u16 = 3;

/// ABI-compatible `VARIANT` for the `IShellWindows::Item` by-value parameter.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Variant {
    vt: u16,
    reserved: [u16; 3],
    data1: isize,
    data2: isize,
}

/// ABI-compatible `PROPERTYKEY` / `SORTCOLUMN` passed to the Explorer view methods.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct PropertyKey {
    pub(crate) fmtid: GUID,
    pub(crate) pid: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct SortColumn {
    pub(crate) propkey: PropertyKey,
    pub(crate) direction: i32,
}

/// Vtable slots used by W6-01a and its dependent W6-01d reader.
pub(crate) struct ExplorerNativeVtable;

impl ExplorerNativeVtable {
    unsafe fn slot(pointer: NonNull<c_void>, slot: usize) -> *const c_void {
        // SAFETY: callers provide a live COM interface whose vtable contains the SDK-defined slot.
        let vtable = unsafe { *(pointer.as_ptr().cast::<*const *const c_void>()) };
        // SAFETY: slot indices are constants from the interface ABI and caller guarantees interface identity.
        unsafe { *vtable.add(slot) }
    }

    pub(crate) fn shell_windows_count(pointer: NonNull<c_void>) -> ShellResult<i32> {
        let mut count = 0;
        // SAFETY: IShellWindows::get_Count is slot 7 and receives a valid out pointer.
        let hr = unsafe {
            let function: unsafe extern "system" fn(*mut c_void, *mut i32) -> i32 =
                std::mem::transmute(Self::slot(pointer, 7));
            function(pointer.as_ptr(), &mut count)
        };
        ShellCom::check(hr)?;
        Ok(count.max(0))
    }

    pub(crate) fn shell_windows_item(pointer: NonNull<c_void>, index: i32) -> ShellResult<Option<NonNull<c_void>>> {
        let mut dispatch = std::ptr::null_mut();
        let variant = Variant {
            vt: VT_I4,
            reserved: [0; 3],
            data1: index as isize,
            data2: 0,
        };
        // SAFETY: IShellWindows::Item is slot 8; VARIANT layout and by-value ABI match the SDK declaration.
        let hr = unsafe {
            let function: unsafe extern "system" fn(*mut c_void, Variant, *mut *mut c_void) -> i32 =
                std::mem::transmute(Self::slot(pointer, 8));
            function(pointer.as_ptr(), variant, &mut dispatch)
        };
        if let Err(error) = ShellCom::check(hr) {
            if let Some(dispatch) = NonNull::new(dispatch) {
                // SAFETY: Item returned an owned dispatch reference even though the HRESULT failed.
                unsafe { ShellCom::release(dispatch) };
            }
            return Err(error);
        }
        Ok(NonNull::new(dispatch))
    }

    pub(crate) fn web_browser_location_url(pointer: NonNull<c_void>) -> ShellResult<Option<String>> {
        let mut bstr = std::ptr::null_mut();
        // SAFETY: IWebBrowserApp::get_LocationURL is slot 30 and returns an owned BSTR through the out pointer.
        let hr = unsafe {
            let function: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> i32 =
                std::mem::transmute(Self::slot(pointer, 30));
            function(pointer.as_ptr(), &mut bstr)
        };
        if hr < 0 {
            if !bstr.is_null() {
                // SAFETY: a non-null out BSTR from this call is owned by us even if the HRESULT failed.
                drop(unsafe { BSTR::from_raw(bstr) });
            }
            return Err(ShellComError(hr));
        }
        if bstr.is_null() {
            return Ok(None);
        }
        // SAFETY: successful get_LocationURL transfers one BSTR; `from_raw` adopts it and Drop frees it.
        let value = unsafe { BSTR::from_raw(bstr) };
        Ok(Some(String::from_utf16_lossy(&value)))
    }

    pub(crate) fn query_service(pointer: NonNull<c_void>, service: &GUID, iid: &GUID) -> ShellResult<NonNull<c_void>> {
        let mut result = std::ptr::null_mut();
        // SAFETY: IServiceProvider::QueryService is slot 3; both GUIDs and the result pointer remain valid for the call.
        let hr = unsafe {
            let function: unsafe extern "system" fn(*mut c_void, *const GUID, *const GUID, *mut *mut c_void) -> i32 =
                std::mem::transmute(Self::slot(pointer, 3));
            function(pointer.as_ptr(), service, iid, &mut result)
        };
        ShellCom::check(hr)?;
        NonNull::new(result).ok_or(ShellComError(E_FAIL))
    }

    pub(crate) fn query_active_shell_view(pointer: NonNull<c_void>) -> ShellResult<NonNull<c_void>> {
        let mut result = std::ptr::null_mut();
        // SAFETY: IShellBrowser::QueryActiveShellView is slot 15 with a valid out pointer.
        let hr = unsafe {
            let function: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32 =
                std::mem::transmute(Self::slot(pointer, 15));
            function(pointer.as_ptr(), &mut result)
        };
        ShellCom::check(hr)?;
        NonNull::new(result).ok_or(ShellComError(E_FAIL))
    }

    pub(crate) fn item_count(pointer: NonNull<c_void>, flags: u32) -> ShellResult<i32> {
        let mut count = 0;
        // SAFETY: IFolderView::ItemCount is slot 7 with a valid out pointer.
        let hr = unsafe {
            let function: unsafe extern "system" fn(*mut c_void, u32, *mut i32) -> i32 =
                std::mem::transmute(Self::slot(pointer, 7));
            function(pointer.as_ptr(), flags, &mut count)
        };
        ShellCom::check(hr)?;
        Ok(count)
    }

    pub(crate) fn items(pointer: NonNull<c_void>, flags: u32, iid: &GUID) -> ShellResult<NonNull<c_void>> {
        let mut result = std::ptr::null_mut();
        // SAFETY: IFolderView::Items is slot 8 and receives a valid IID and result pointer.
        let hr = unsafe {
            let function: unsafe extern "system" fn(*mut c_void, u32, *const GUID, *mut *mut c_void) -> i32 =
                std::mem::transmute(Self::slot(pointer, 8));
            function(pointer.as_ptr(), flags, iid, &mut result)
        };
        ShellCom::check(hr)?;
        NonNull::new(result).ok_or(ShellComError(E_FAIL))
    }

    pub(crate) fn enum_next(pointer: NonNull<c_void>, count: u32, items: &mut [*mut c_void]) -> ShellResult<u32> {
        let mut fetched = 0;
        // SAFETY: IEnumIDList::Next is slot 3; the array has at least `count` writable entries.
        let hr = unsafe {
            let function: unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void, *mut u32) -> i32 =
                std::mem::transmute(Self::slot(pointer, 3));
            function(pointer.as_ptr(), count, items.as_mut_ptr(), &mut fetched)
        };
        ShellCom::check(hr)?;
        Ok(fetched)
    }

    pub(crate) fn get_group_by(pointer: NonNull<c_void>) -> ShellResult<(PropertyKey, bool)> {
        let mut key = PropertyKey::default();
        let mut ascending = 0;
        // SAFETY: IFolderView2::GetGroupBy is slot 18 and writes ABI-compatible output structs.
        let hr = unsafe {
            let function: unsafe extern "system" fn(*mut c_void, *mut PropertyKey, *mut i32) -> i32 =
                std::mem::transmute(Self::slot(pointer, 18));
            function(pointer.as_ptr(), &mut key, &mut ascending)
        };
        ShellCom::check(hr)?;
        Ok((key, ascending != 0))
    }

    pub(crate) fn get_sort_column_count(pointer: NonNull<c_void>) -> ShellResult<i32> {
        let mut count = 0;
        // SAFETY: IFolderView2::GetSortColumnCount is slot 26 with a valid out pointer.
        let hr = unsafe {
            let function: unsafe extern "system" fn(*mut c_void, *mut i32) -> i32 =
                std::mem::transmute(Self::slot(pointer, 26));
            function(pointer.as_ptr(), &mut count)
        };
        ShellCom::check(hr)?;
        Ok(count)
    }

    pub(crate) fn get_sort_columns(pointer: NonNull<c_void>, columns: &mut [SortColumn]) -> ShellResult<()> {
        // SAFETY: IFolderView2::GetSortColumns is slot 28; slice pointer and length describe writable ABI structs.
        let hr = unsafe {
            let function: unsafe extern "system" fn(*mut c_void, *mut SortColumn, i32) -> i32 =
                std::mem::transmute(Self::slot(pointer, 28));
            function(pointer.as_ptr(), columns.as_mut_ptr(), columns.len() as i32)
        };
        ShellCom::check(hr)
    }

    pub(crate) fn resolve_get_item(pointer: NonNull<c_void>) -> *const c_void {
        // SAFETY: IFolderView::GetItem is slot 29; caller invokes the returned address only with this view pointer.
        unsafe { Self::slot(pointer, 29) }
    }

    pub(crate) fn get_item(
        function: *const c_void,
        pointer: NonNull<c_void>,
        index: i32,
        iid: &GUID,
    ) -> ShellResult<NonNull<c_void>> {
        let mut item = std::ptr::null_mut();
        // SAFETY: `function` came from this view's GetItem slot and all arguments match IFolderView::GetItem ABI.
        let hr = unsafe {
            let function: unsafe extern "system" fn(*mut c_void, i32, *const GUID, *mut *mut c_void) -> i32 =
                std::mem::transmute(function);
            function(pointer.as_ptr(), index, iid, &mut item)
        };
        ShellCom::check(hr)?;
        NonNull::new(item).ok_or(ShellComError(E_FAIL))
    }

    pub(crate) fn resolve_get_display_name(pointer: NonNull<c_void>) -> *const c_void {
        // SAFETY: IShellItem::GetDisplayName is slot 5; caller invokes it only with this item pointer.
        unsafe { Self::slot(pointer, 5) }
    }

    pub(crate) fn get_display_name(
        function: *const c_void,
        pointer: NonNull<c_void>,
        kind: u32,
    ) -> ShellResult<NonNull<u16>> {
        let mut name = std::ptr::null_mut();
        // SAFETY: `function` came from this item's GetDisplayName slot and output is a valid writable pointer.
        let hr = unsafe {
            let function: unsafe extern "system" fn(*mut c_void, u32, *mut *mut u16) -> i32 =
                std::mem::transmute(function);
            function(pointer.as_ptr(), kind, &mut name)
        };
        ShellCom::check(hr)?;
        NonNull::new(name).ok_or(ShellComError(E_FAIL))
    }
}

// ABI-compatible sort column array item.

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    const E_FAIL: i32 = 0x8000_4005_u32 as i32;
    const E_NOINTERFACE: i32 = 0x8000_4002_u32 as i32;
    const IID_IUNKNOWN: GUID = GUID::from_u128(0x0000_0000_0000_0000_c000_0000_0000_0046);

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct TestVariant {
        vt: u16,
        reserved: [u16; 3],
        data1: isize,
        data2: isize,
    }

    struct State {
        refs: usize,
        count: i32,
        count_hr: i32,
        item_hr: HashMap<i32, i32>,
        items: HashMap<i32, usize>,
        last: Option<TestVariant>,
        location_hr: i32,
        location: Option<String>,
        supports_app: bool,
    }
    struct Fake {
        pointer: NonNull<c_void>,
        table: NonNull<*const c_void>,
        state: Mutex<State>,
    }
    // SAFETY: the fake's mutable state is mutex-protected and its allocation is stable until explicit test cleanup.
    unsafe impl Send for Fake {}
    // SAFETY: the fake's mutable state is mutex-protected and its allocation is stable until explicit test cleanup.
    unsafe impl Sync for Fake {}

    fn registry() -> &'static Mutex<HashMap<usize, &'static Fake>> {
        static REGISTRY: OnceLock<Mutex<HashMap<usize, &'static Fake>>> = OnceLock::new();
        REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
    }

    impl Fake {
        fn new() -> &'static Self {
            let mut table = vec![std::ptr::null::<c_void>(); 31];
            table[0] = query as *const c_void;
            table[1] = add_ref as *const c_void;
            table[2] = release as *const c_void;
            table[7] = count as *const c_void;
            table[8] = item as *const c_void;
            table[30] = location as *const c_void;
            let table = NonNull::new(Box::into_raw(table.into_boxed_slice()).cast::<*const c_void>()).unwrap();
            let pointer = NonNull::new(Box::into_raw(Box::new(table.as_ptr())).cast::<c_void>()).unwrap();
            let fake = Box::leak(Box::new(Self {
                pointer,
                table,
                state: Mutex::new(State {
                    refs: 0,
                    count: 0,
                    count_hr: 0,
                    item_hr: HashMap::new(),
                    items: HashMap::new(),
                    last: None,
                    location_hr: 0,
                    location: None,
                    supports_app: true,
                }),
            }));
            registry().lock().unwrap().insert(pointer.as_ptr() as usize, fake);
            fake
        }

        fn add_owned(&self) -> NonNull<c_void> {
            self.state.lock().unwrap().refs += 1;
            self.pointer
        }
        fn refs(&self) -> usize {
            self.state.lock().unwrap().refs
        }
        fn free(&'static self) {
            registry().lock().unwrap().remove(&(self.pointer.as_ptr() as usize));
            // SAFETY: the fake is removed from the registry and no test-owned wrapper may still call its vtable.
            unsafe {
                drop(Box::from_raw(self.pointer.as_ptr().cast::<*const c_void>()));
            }
            // SAFETY: `table` was allocated by `Box::into_raw` in `new` and has exactly 31 entries.
            unsafe {
                drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                    self.table.as_ptr(),
                    31,
                )));
            }
            // SAFETY: Fake was leaked only to keep a stable address in the test registry.
            unsafe {
                drop(Box::from_raw(self as *const Self as *mut Self));
            }
        }
    }

    fn fake(pointer: *mut c_void) -> &'static Fake {
        registry().lock().unwrap()[&(pointer as usize)]
    }

    unsafe extern "system" fn query(this: *mut c_void, iid: *const GUID, output: *mut *mut c_void) -> i32 {
        let fake = fake(this);
        // SAFETY: QueryInterface supplies a valid IID pointer for the duration of this fake call.
        let iid = unsafe { *iid };
        let mut state = fake.state.lock().unwrap();
        if iid == IID_IUNKNOWN || (iid == ids::IID_WEB_BROWSER_APP && state.supports_app) {
            state.refs += 1;
            // SAFETY: the fake caller supplied a writable output pointer as required by QueryInterface.
            unsafe { *output = this };
            0
        } else {
            // SAFETY: the fake caller supplied a writable output pointer as required by QueryInterface.
            unsafe { *output = std::ptr::null_mut() };
            E_NOINTERFACE
        }
    }
    unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
        let fake = fake(this);
        let mut state = fake.state.lock().unwrap();
        state.refs += 1;
        state.refs as u32
    }
    unsafe extern "system" fn release(this: *mut c_void) -> u32 {
        let fake = fake(this);
        let mut state = fake.state.lock().unwrap();
        state.refs -= 1;
        state.refs as u32
    }
    unsafe extern "system" fn count(this: *mut c_void, output: *mut i32) -> i32 {
        let fake = fake(this);
        let state = fake.state.lock().unwrap();
        // SAFETY: the fake caller supplied a writable count pointer as required by get_Count.
        unsafe { *output = state.count };
        state.count_hr
    }
    unsafe extern "system" fn item(this: *mut c_void, variant: TestVariant, output: *mut *mut c_void) -> i32 {
        let fake = fake(this);
        let mut state = fake.state.lock().unwrap();
        state.last = Some(variant);
        let index = variant.data1 as i32;
        let hr = *state.item_hr.get(&index).unwrap_or(&0);
        if let Some(pointer) = state.items.get(&index).copied() {
            let found = registry().lock().unwrap()[&pointer];
            found.state.lock().unwrap().refs += 1;
            // SAFETY: the fake caller supplied a writable dispatch output pointer as required by Item.
            unsafe { *output = pointer as *mut c_void };
        } else {
            // SAFETY: the fake caller supplied a writable dispatch output pointer as required by Item.
            unsafe { *output = std::ptr::null_mut() };
        }
        hr
    }
    unsafe extern "system" fn location(this: *mut c_void, output: *mut *mut u16) -> i32 {
        let fake = fake(this);
        let state = fake.state.lock().unwrap();
        if let Some(value) = &state.location {
            let bstr = BSTR::from(value.as_str());
            // SAFETY: the fake caller supplied a writable BSTR output pointer as required by get_LocationURL.
            unsafe { *output = bstr.into_raw().cast_mut() };
        } else {
            // SAFETY: the fake caller supplied a writable BSTR output pointer as required by get_LocationURL.
            unsafe { *output = std::ptr::null_mut() };
        }
        state.location_hr
    }

    fn setup() -> (&'static Fake, &'static Fake) {
        (Fake::new(), Fake::new())
    }

    // C#: ShellWindows_WalksCountItems_ReadsLocation_AndReleasesEverything
    #[test]
    fn shell_windows_walks_count_items_reads_location_and_releases_everything() {
        let (collection, browser) = setup();
        collection.state.lock().unwrap().count = 2;
        collection
            .state
            .lock()
            .unwrap()
            .items
            .insert(0, browser.pointer.as_ptr() as usize);
        collection
            .state
            .lock()
            .unwrap()
            .items
            .insert(1, browser.pointer.as_ptr() as usize);
        browser.state.lock().unwrap().location = Some("file:///C:/photos/".into());
        let source = ShellWindowsSource::from_owned(collection.add_owned());
        let locations = source
            .windows()
            .map(|window| window.unwrap().location_url().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(locations, vec![Some("file:///C:/photos/".into()); 2]);
        drop(source);
        assert_eq!(collection.refs(), 0);
        assert_eq!(browser.refs(), 0);
        collection.free();
        browser.free();
    }

    // C#: ShellWindows_IndexIsPassedAsAnI4Variant
    #[test]
    fn shell_windows_index_is_passed_as_vt_i4() {
        let (collection, browser) = setup();
        collection.state.lock().unwrap().count = 1;
        collection
            .state
            .lock()
            .unwrap()
            .items
            .insert(0, browser.pointer.as_ptr() as usize);
        let source = ShellWindowsSource::from_owned(collection.add_owned());
        drop(source.windows().next().unwrap().unwrap());
        let variant = collection.state.lock().unwrap().last.unwrap();
        assert_eq!(variant.vt, VT_I4);
        assert_eq!(variant.data1, 0);
        drop(source);
        assert_eq!(collection.refs(), 0);
        assert_eq!(browser.refs(), 0);
        collection.free();
        browser.free();
    }

    // C#: ShellWindows_ANullItem_IsSkipped
    #[test]
    fn shell_windows_null_item_is_skipped() {
        let (collection, browser) = setup();
        collection.state.lock().unwrap().count = 3;
        collection
            .state
            .lock()
            .unwrap()
            .items
            .insert(0, browser.pointer.as_ptr() as usize);
        collection
            .state
            .lock()
            .unwrap()
            .items
            .insert(2, browser.pointer.as_ptr() as usize);
        let source = ShellWindowsSource::from_owned(collection.add_owned());
        assert_eq!(source.windows().count(), 2);
        drop(source);
        assert_eq!(collection.refs(), 0);
        assert_eq!(browser.refs(), 0);
        collection.free();
        browser.free();
    }

    // C#: ShellWindows_ItemFailingMidWalk_SurfacesAsComExceptionFromTheEnumerator
    #[test]
    fn shell_windows_item_failure_surfaces_from_iterator() {
        let collection = Fake::new();
        let browser = Fake::new();
        collection.state.lock().unwrap().count = 2;
        collection
            .state
            .lock()
            .unwrap()
            .items
            .insert(0, browser.pointer.as_ptr() as usize);
        collection.state.lock().unwrap().item_hr.insert(1, E_FAIL);
        let source = ShellWindowsSource::from_owned(collection.add_owned());
        {
            let mut iter = source.windows();
            drop(iter.next().unwrap().unwrap());
            assert_eq!(iter.next().unwrap().unwrap_err(), ShellComError(E_FAIL));
            assert!(iter.next().is_none());
        }
        drop(source);
        assert_eq!(collection.refs(), 0);
        assert_eq!(browser.refs(), 0);
        collection.free();
        browser.free();
    }

    #[test]
    fn shell_windows_iterator_never_requests_an_index_after_count() {
        let collection = Fake::new();
        let browser = Fake::new();
        collection.state.lock().unwrap().count = 1;
        collection
            .state
            .lock()
            .unwrap()
            .items
            .insert(0, browser.pointer.as_ptr() as usize);
        let source = ShellWindowsSource::from_owned(collection.add_owned());
        let mut windows = source.windows();
        drop(windows.next().unwrap().unwrap());
        assert!(windows.next().is_none());
        assert_eq!(collection.state.lock().unwrap().last.unwrap().data1, 0);
        drop(source);
        assert_eq!(collection.refs(), 0);
        assert_eq!(browser.refs(), 0);
        collection.free();
        browser.free();
    }

    // C#: ShellWindows_CountFailing_SurfacesAsComException
    #[test]
    fn shell_windows_count_failure_is_reported() {
        let collection = Fake::new();
        collection.state.lock().unwrap().count_hr = E_FAIL;
        let source = ShellWindowsSource::from_owned(collection.add_owned());
        assert_eq!(source.windows().next().unwrap().unwrap_err(), ShellComError(E_FAIL));
        drop(source);
        assert_eq!(collection.refs(), 0);
        collection.free();
    }

    // C#: ShellWindows_UseAfterDispose_Throws_AndDisposeIsIdempotent
    #[test]
    fn shell_windows_use_after_dispose_fails_and_dispose_releases_once() {
        let collection = Fake::new();
        let source = ShellWindowsSource::from_owned(collection.add_owned());
        source.dispose();
        source.dispose();
        assert_eq!(collection.refs(), 0);
        assert_eq!(
            source.windows().next().unwrap().unwrap_err(),
            ShellComError(COR_E_OBJECTDISPOSED)
        );
        drop(source);
        collection.free();
    }

    // C#: ShellWindow_LocationFailures_AreComExceptions_AndAnEmptyLocationIsNull
    #[test]
    fn shell_window_location_failure_preserves_hresult_null_is_none_and_releases_interface() {
        let browser = Fake::new();
        let window = ShellWebBrowser {
            dispatch: AtomicPtr::new(browser.add_owned().as_ptr()),
        };
        browser.state.lock().unwrap().supports_app = false;
        assert_eq!(window.location_url().unwrap_err(), ShellComError(E_NOINTERFACE));
        browser.state.lock().unwrap().supports_app = true;
        browser.state.lock().unwrap().location_hr = E_FAIL;
        assert_eq!(window.location_url().unwrap_err(), ShellComError(E_FAIL));
        browser.state.lock().unwrap().location_hr = 0;
        browser.state.lock().unwrap().location = None;
        assert_eq!(window.location_url().unwrap(), None);
        window.dispose();
        window.dispose();
        assert_eq!(window.location_url(), Err(ShellComError(COR_E_OBJECTDISPOSED)));
        drop(window);
        assert_eq!(browser.refs(), 0);
        browser.free();
    }

    // C#: ShellWindow_AcquireUnknown_IsANewReference_AndDisposeReleasesTheOwnedOneOnce
    #[test]
    fn shell_window_acquire_unknown_adds_one_owned_reference() {
        let browser = Fake::new();
        let window = ShellWebBrowser {
            dispatch: AtomicPtr::new(browser.add_owned().as_ptr()),
        };
        let unknown = window.acquire_unknown().unwrap();
        assert_eq!(browser.refs(), 2);
        drop(unknown);
        assert_eq!(browser.refs(), 1);
        window.dispose();
        window.dispose();
        assert_eq!(
            window.acquire_unknown().unwrap_err(),
            ShellComError(COR_E_OBJECTDISPOSED)
        );
        drop(window);
        assert_eq!(browser.refs(), 0);
        browser.free();
    }

    #[test]
    fn shell_com_accepts_success_and_s_false_and_retains_failure_hresult() {
        assert_eq!(ShellCom::check(0), Ok(()));
        assert_eq!(ShellCom::check(ids::S_FALSE), Ok(()));
        let error = ShellCom::check(E_FAIL).unwrap_err();
        assert_eq!(error, ShellComError(E_FAIL));
        assert_eq!(error.hresult(), E_FAIL);
        assert_eq!(error.to_string(), "shell COM call failed (HRESULT 0x80004005)");
    }

    #[test]
    fn owned_unknown_raw_accessors_preserve_the_owned_pointer() {
        let browser = Fake::new();
        let unknown = OwnedUnknown(browser.add_owned());
        let expected = browser.pointer.as_ptr();
        assert_eq!(unknown.as_raw(), expected);

        let raw = unknown.into_raw();
        assert_eq!(raw, expected);
        assert_eq!(browser.refs(), 1);
        let owned = NonNull::new(raw).expect("into_raw must return the owned pointer");
        // SAFETY: `into_raw` transferred this one live fake COM reference to the test, which releases it exactly once.
        unsafe { ShellCom::release(owned) };
        assert_eq!(browser.refs(), 0);
        browser.free();
    }

    #[test]
    fn shell_variant_layout_and_pinned_guids_match_the_windows_abi() {
        assert_eq!(
            std::mem::size_of::<Variant>(),
            if std::mem::size_of::<usize>() == 8 { 24 } else { 16 }
        );
        assert_eq!(
            ids::CLSID_SHELL_WINDOWS,
            GUID::from_u128(0x9ba0_5972_f6a8_11cf_a442_00a0_c90a_8f39)
        );
        assert_eq!(
            ids::IID_WEB_BROWSER_APP,
            GUID::from_u128(0x0002_df05_0000_0000_c000_0000_0000_0046)
        );
    }
}
