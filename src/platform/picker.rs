//! The folder picker (008 T044; 016 T026 on macOS): adding a project is one "+", and the page
//! cannot read a folder's full path itself, so Fallow opens the system's folder dialog on the
//! engineer's desktop and returns the folder chosen. Elsewhere there is no picker yet, and the
//! page asks for the path.

use std::path::PathBuf;

/// Opens the folder picker and waits for the engineer: `Ok(None)` when cancelled.
#[cfg(windows)]
pub fn pick_folder(title: &str) -> Result<Option<PathBuf>, String> {
    // The dialog lives on a thread of its own, with COM initialised for it alone.
    let title = title.to_string();
    std::thread::Builder::new()
        .name("folder-picker".into())
        .spawn(move || win::pick(&title))
        .map_err(|e| format!("cannot open the folder picker: {e}"))?
        .join()
        .map_err(|_| "the folder picker failed".to_string())?
}

/// 016 (research M-8, FR-011, T026): macOS's open panel (folders only, one), as a sheet on
/// Fallow's window when it is open, else on its own; run on the main thread while this request's
/// thread waits for the answer.
#[cfg(target_os = "macos")]
pub fn pick_folder(title: &str) -> Result<Option<PathBuf>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let title = title.to_string();
    let asked = crate::platform::notify::on_ui(Box::new(move || {
        crate::window::mac::alert::pick_folder(&title, move |p| {
            let _ = tx.send(p);
        });
    }));
    if !asked {
        return Err("Fallow has no window here (no display): type the folder's path".into());
    }
    rx.recv()
        .map_err(|_| "the folder picker closed without an answer".to_string())
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn pick_folder(_title: &str) -> Result<Option<PathBuf>, String> {
    Err("there is no folder picker on this platform yet: type the folder's path".into())
}

/// 014 (research R-5): Fallow's window, the picker's owner while it is open (0: none).
#[cfg(windows)]
pub static OWNER: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::path::PathBuf;
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree, CoUninitialize,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, SetForegroundWindow, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_POPUP,
    };
    use windows_sys::core::{GUID, HRESULT, PWSTR};

    // CLSID_FileOpenDialog and IID_IFileOpenDialog (ShObjIdl_core.h).
    const CLSID_FILE_OPEN_DIALOG: GUID = GUID::from_u128(0xdc1c5a9c_e88a_4dde_a5a1_60f82a20aef7);
    const IID_IFILE_OPEN_DIALOG: GUID = GUID::from_u128(0xd57c7288_d4ad_4768_be02_9d969532d960);
    const FOS_PICKFOLDERS: u32 = 0x20;
    const FOS_FORCEFILESYSTEM: u32 = 0x40;
    const FOS_PATHMUSTEXIST: u32 = 0x800;
    const SIGDN_FILESYSPATH: i32 = 0x8005_8000_u32 as i32;
    const ERROR_CANCELLED: HRESULT = 0x8007_04C7_u32 as i32;

    /// IFileOpenDialog's methods up to GetResult, in their order (IUnknown, IModalWindow,
    /// IFileDialog); the others are not called.
    #[repr(C)]
    struct DialogVtbl {
        _query_interface: usize,
        _add_ref: usize,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        show: unsafe extern "system" fn(*mut c_void, HWND) -> HRESULT,
        _set_file_types: usize,
        _set_file_type_index: usize,
        _get_file_type_index: usize,
        _advise: usize,
        _unadvise: usize,
        set_options: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
        get_options: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
        _set_default_folder: usize,
        _set_folder: usize,
        _get_folder: usize,
        _get_current_selection: usize,
        _set_file_name: usize,
        _get_file_name: usize,
        set_title: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
        _set_ok_button_label: usize,
        _set_file_name_label: usize,
        get_result: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    }

    /// IShellItem's methods up to GetDisplayName.
    #[repr(C)]
    struct ItemVtbl {
        _query_interface: usize,
        _add_ref: usize,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        _bind_to_handler: usize,
        _get_parent: usize,
        get_display_name: unsafe extern "system" fn(*mut c_void, i32, *mut PWSTR) -> HRESULT,
    }

    pub fn pick(title: &str) -> Result<Option<PathBuf>, String> {
        let wide: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
        // SAFETY: COM on this thread only; every interface is released, the owner window
        // destroyed and COM uninitialised before returning; the vtables follow the SDK's order.
        unsafe {
            let hr = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
            if hr < 0 {
                return Err(format!("cannot open the folder picker (COM {hr:#x})"));
            }
            let result = (|| {
                let mut dialog: *mut c_void = std::ptr::null_mut();
                let hr = CoCreateInstance(
                    &CLSID_FILE_OPEN_DIALOG,
                    std::ptr::null_mut(),
                    CLSCTX_INPROC_SERVER,
                    &IID_IFILE_OPEN_DIALOG,
                    &mut dialog,
                );
                if hr < 0 || dialog.is_null() {
                    return Err(format!("cannot open the folder picker ({hr:#x})"));
                }
                let vt = &**(dialog as *mut *const DialogVtbl);
                let mut opts = 0u32;
                (vt.get_options)(dialog, &mut opts);
                (vt.set_options)(
                    dialog,
                    opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST,
                );
                (vt.set_title)(dialog, wide.as_ptr());
                // Fallow's window when it is open (014): the picker is its dialog. Otherwise an
                // owner kept on top, so the picker opens in front of the browser, not behind it.
                let window = super::OWNER.load(std::sync::atomic::Ordering::Acquire) as HWND;
                if !window.is_null() {
                    let shown = (vt.show)(dialog, window);
                    return finish(dialog, vt, shown);
                }
                let class: Vec<u16> = "STATIC".encode_utf16().chain(Some(0)).collect();
                let owner = CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                    class.as_ptr(),
                    std::ptr::null(),
                    WS_POPUP,
                    0,
                    0,
                    0,
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                );
                if !owner.is_null() {
                    SetForegroundWindow(owner);
                }
                let shown = (vt.show)(dialog, owner);
                if !owner.is_null() {
                    DestroyWindow(owner);
                }
                finish(dialog, vt, shown)
            })();
            CoUninitialize();
            result
        }
    }

    /// The folder the dialog gave, then the dialog released.
    ///
    /// # Safety
    /// `dialog` is a live IFileOpenDialog with vtable `vt`, shown with the result `shown`.
    unsafe fn finish(
        dialog: *mut c_void,
        vt: &DialogVtbl,
        shown: HRESULT,
    ) -> Result<Option<PathBuf>, String> {
        // SAFETY: as the caller promises; every interface is released.
        unsafe {
            let out = if shown == ERROR_CANCELLED {
                Ok(None)
            } else if shown < 0 {
                Err(format!("the folder picker failed ({shown:#x})"))
            } else {
                let mut item: *mut c_void = std::ptr::null_mut();
                let hr = (vt.get_result)(dialog, &mut item);
                if hr < 0 || item.is_null() {
                    Err(format!("the folder picker gave no folder ({hr:#x})"))
                } else {
                    let iv = &**(item as *mut *const ItemVtbl);
                    let mut name: PWSTR = std::ptr::null_mut();
                    let hr = (iv.get_display_name)(item, SIGDN_FILESYSPATH, &mut name);
                    let path = if hr < 0 || name.is_null() {
                        Err(format!("the folder chosen has no path on disk ({hr:#x})"))
                    } else {
                        let mut n = 0;
                        while *name.add(n) != 0 {
                            n += 1;
                        }
                        let s = String::from_utf16_lossy(std::slice::from_raw_parts(name, n));
                        CoTaskMemFree(name as *const c_void);
                        Ok(Some(PathBuf::from(s)))
                    };
                    (iv.release)(item);
                    path
                }
            };
            (vt.release)(dialog);
            out
        }
    }
}
