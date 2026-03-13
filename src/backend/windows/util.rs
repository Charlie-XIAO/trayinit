use std::ffi::OsStr;
use std::os::windows::prelude::OsStrExt;
use std::sync::Once;

use once_cell::sync::Lazy;
use windows_sys::Win32::{
    Foundation::{FARPROC, HWND},
    System::LibraryLoader::{GetProcAddress, LoadLibraryW},
    UI::{
        HiDpi::{
            DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE,
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, PROCESS_DPI_AWARENESS,
            PROCESS_PER_MONITOR_DPI_AWARE,
        },
        WindowsAndMessaging::WINDOW_LONG_PTR_INDEX,
    },
};

pub fn encode_wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value
        .as_ref()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

// Reference: tray-icon/src/platform_impl/windows/util.rs::get_instance_handle.
pub fn get_instance_handle() -> windows_sys::Win32::Foundation::HMODULE {
    unsafe extern "C" {
        static __ImageBase: windows_sys::Win32::System::SystemServices::IMAGE_DOS_HEADER;
    }

    unsafe { &__ImageBase as *const _ as _ }
}

fn get_function_impl(library: &str, function: &str) -> FARPROC {
    let library = encode_wide(library);
    assert_eq!(function.chars().last(), Some('\0'));

    let module = unsafe { LoadLibraryW(library.as_ptr()) };
    if module.is_null() {
        return None;
    }

    unsafe { GetProcAddress(module, function.as_ptr()) }
}

type SetProcessDPIAware = unsafe extern "system" fn() -> windows_sys::core::BOOL;
type SetProcessDpiAwareness =
    unsafe extern "system" fn(PROCESS_DPI_AWARENESS) -> windows_sys::core::HRESULT;
type SetProcessDpiAwarenessContext =
    unsafe extern "system" fn(DPI_AWARENESS_CONTEXT) -> windows_sys::core::BOOL;

static SET_PROCESS_DPI_AWARENESS_CONTEXT: Lazy<Option<SetProcessDpiAwarenessContext>> =
    Lazy::new(|| {
        get_function_impl("user32.dll", "SetProcessDpiAwarenessContext\0")
            .map(|function| unsafe { std::mem::transmute(function) })
    });
static SET_PROCESS_DPI_AWARENESS: Lazy<Option<SetProcessDpiAwareness>> = Lazy::new(|| {
    get_function_impl("shcore.dll", "SetProcessDpiAwareness\0")
        .map(|function| unsafe { std::mem::transmute(function) })
});
static SET_PROCESS_DPI_AWARE: Lazy<Option<SetProcessDPIAware>> = Lazy::new(|| {
    get_function_impl("user32.dll", "SetProcessDPIAware\0")
        .map(|function| unsafe { std::mem::transmute(function) })
});

pub fn become_dpi_aware() {
    static ENABLE_DPI_AWARENESS: Once = Once::new();
    ENABLE_DPI_AWARENESS.call_once(|| unsafe {
        if let Some(set_process_dpi_awareness_context) = *SET_PROCESS_DPI_AWARENESS_CONTEXT {
            // Reference: winit/src/platform_impl/windows/dpi.rs::become_dpi_aware.
            if set_process_dpi_awareness_context(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) == 0 {
                let _ = set_process_dpi_awareness_context(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE);
            }
        } else if let Some(set_process_dpi_awareness) = *SET_PROCESS_DPI_AWARENESS {
            let _ = set_process_dpi_awareness(PROCESS_PER_MONITOR_DPI_AWARE);
        } else if let Some(set_process_dpi_aware) = *SET_PROCESS_DPI_AWARE {
            let _ = set_process_dpi_aware();
        }
    });
}

#[inline(always)]
pub unsafe fn get_window_long(hwnd: HWND, nindex: WINDOW_LONG_PTR_INDEX) -> isize {
    #[cfg(target_pointer_width = "64")]
    {
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, nindex) }
    }

    #[cfg(target_pointer_width = "32")]
    {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongW(hwnd, nindex) as isize
        }
    }
}

#[inline(always)]
pub unsafe fn set_window_long(
    hwnd: HWND,
    nindex: WINDOW_LONG_PTR_INDEX,
    dwnewlong: isize,
) -> isize {
    #[cfg(target_pointer_width = "64")]
    {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(hwnd, nindex, dwnewlong)
        }
    }

    #[cfg(target_pointer_width = "32")]
    {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongW(
                hwnd,
                nindex,
                dwnewlong as i32,
            ) as isize
        }
    }
}
