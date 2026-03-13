use std::{io, ptr};

use windows_sys::Win32::Foundation::RECT;
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GetDC, HBITMAP, ReleaseDC, SelectObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIcon, DI_NORMAL, DestroyIcon, DrawIconEx, HICON,
};

use crate::{Error, Icon};

/// Owned `HICON` created from the crate's public RGBA icon.
#[derive(Debug)]
pub(crate) struct OwnedIcon {
    handle: HICON,
}

impl OwnedIcon {
    pub(crate) fn handle(&self) -> HICON {
        self.handle
    }

    pub(crate) fn from_icon(icon: &Icon) -> Result<Self, Error> {
        // Reference:
        // tray-icon/src/platform_impl/windows/icon.rs::RgbaIcon::into_windows_icon
        // and muda/src/platform_impl/windows/icon.rs::RgbaIcon::into_windows_icon.
        let mut bgra = icon.rgba().to_vec();
        let pixel_count = bgra.len() / 4;
        let mut and_mask = Vec::with_capacity(pixel_count);

        for pixel in bgra.chunks_exact_mut(4) {
            and_mask.push(pixel[3].wrapping_sub(u8::MAX));
            pixel.swap(0, 2);
        }

        let handle = unsafe {
            CreateIcon(
                ptr::null_mut(),
                icon.width() as i32,
                icon.height() as i32,
                1,
                32,
                and_mask.as_ptr(),
                bgra.as_ptr(),
            )
        };

        if handle.is_null() {
            Err(Error::Os(io::Error::last_os_error()))
        } else {
            Ok(Self { handle })
        }
    }
}

impl Drop for OwnedIcon {
    fn drop(&mut self) {
        unsafe {
            DestroyIcon(self.handle);
        }
    }
}

/// Owned menu bitmap derived from an icon.
#[derive(Debug)]
pub(crate) struct OwnedBitmap {
    handle: HBITMAP,
}

impl OwnedBitmap {
    pub(crate) fn handle(&self) -> HBITMAP {
        self.handle
    }

    pub(crate) fn from_icon(icon: &Icon) -> Result<Self, Error> {
        // Reference: muda/src/platform_impl/windows/icon.rs::WinIcon::to_hbitmap.
        let icon = OwnedIcon::from_icon(icon)?;
        let hdc = unsafe { CreateCompatibleDC(ptr::null_mut()) };
        if hdc.is_null() {
            return Err(Error::Os(io::Error::last_os_error()));
        }

        let rc = RECT {
            left: 0,
            top: 0,
            right: 16,
            bottom: 16,
        };

        let mut bitmap_info: BITMAPINFO = unsafe { std::mem::zeroed() };
        bitmap_info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as _;
        bitmap_info.bmiHeader.biWidth = rc.right;
        bitmap_info.bmiHeader.biHeight = rc.bottom;
        bitmap_info.bmiHeader.biPlanes = 1;
        bitmap_info.bmiHeader.biBitCount = 32;
        bitmap_info.bmiHeader.biCompression = BI_RGB as _;

        let screen_dc = unsafe { GetDC(ptr::null_mut()) };
        let hbitmap = unsafe {
            CreateDIBSection(
                screen_dc,
                &bitmap_info,
                DIB_RGB_COLORS,
                ptr::null_mut(),
                ptr::null_mut(),
                0,
            )
        };
        unsafe {
            ReleaseDC(ptr::null_mut(), screen_dc);
        }

        if hbitmap.is_null() {
            unsafe {
                DeleteDC(hdc);
            }
            return Err(Error::Os(io::Error::last_os_error()));
        }

        let old = unsafe { SelectObject(hdc, hbitmap as _) };
        unsafe {
            DrawIconEx(
                hdc,
                0,
                0,
                icon.handle(),
                rc.right,
                rc.bottom,
                0,
                ptr::null_mut(),
                DI_NORMAL,
            );
            SelectObject(hdc, old);
            DeleteDC(hdc);
        }

        Ok(Self { handle: hbitmap })
    }
}

impl Drop for OwnedBitmap {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.handle as _);
        }
    }
}
