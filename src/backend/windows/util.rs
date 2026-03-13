use std::ffi::OsStr;
use std::os::windows::prelude::OsStrExt;

use windows_sys::Win32::{Foundation::HWND, UI::WindowsAndMessaging::WINDOW_LONG_PTR_INDEX};

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

#[inline(always)]
pub unsafe fn get_window_long(hwnd: HWND, nindex: WINDOW_LONG_PTR_INDEX) -> isize {
    #[cfg(target_pointer_width = "64")]
    {
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, nindex) }
    }

    #[cfg(target_pointer_width = "32")]
    {
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongW(hwnd, nindex) as isize }
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
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(hwnd, nindex, dwnewlong) }
    }

    #[cfg(target_pointer_width = "32")]
    {
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongW(hwnd, nindex, dwnewlong as i32)
                as isize
        }
    }
}

