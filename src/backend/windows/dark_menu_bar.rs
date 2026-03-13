// Reference: muda/src/platform_impl/windows/dark_menu_bar.rs.
// This file is an adapted port of muda's Windows dark menu drawing path.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::cell::Cell;
use std::sync::Once;

use once_cell::sync::Lazy;
use windows_sys::Win32::Foundation::{HWND, LPARAM, NTSTATUS, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows_sys::Win32::UI::Accessibility::HIGHCONTRASTA;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetMenuBarInfo, GetMenuItemInfoW, GetWindowRect, HMENU, MENUBARINFO,
    MENUITEMINFOW, MIIM_STRING, OBJID_MENU, SPI_GETHIGHCONTRAST, SystemParametersInfoA,
    WM_NCACTIVATE, WM_NCPAINT,
};
use windows_sys::s;

pub const WM_UAHDRAWMENU: u32 = 0x0091;
pub const WM_UAHDRAWMENUITEM: u32 = 0x0092;

#[repr(C)]
struct UAHMENUITEMMETRICS0 {
    cx: u32,
    cy: u32,
}

#[repr(C)]
struct UAHMENUITEMMETRICS {
    rgsizeBar: [UAHMENUITEMMETRICS0; 2],
    rgsizePopup: [UAHMENUITEMMETRICS0; 4],
}

#[repr(C)]
struct UAHMENUPOPUPMETRICS {
    rgcx: [u32; 4],
    fUpdateMaxWidths: u32,
}

#[repr(C)]
struct UAHMENU {
    hmenu: HMENU,
    hdc: HDC,
    dwFlags: u32,
}

#[repr(C)]
struct UAHMENUITEM {
    iPosition: u32,
    umim: UAHMENUITEMMETRICS,
    umpm: UAHMENUPOPUPMETRICS,
}

#[repr(C)]
struct UAHDRAWMENUITEM {
    dis: DRAWITEMSTRUCT,
    um: UAHMENU,
    umi: UAHMENUITEM,
}

#[derive(Debug)]
struct Win32Brush(Cell<HBRUSH>);

impl Win32Brush {
    const fn null() -> Self {
        Self(Cell::new(std::ptr::null_mut()))
    }

    fn get_or_set(&self, color: u32) -> HBRUSH {
        if self.0.get().is_null() {
            self.0.set(unsafe { CreateSolidBrush(color) });
        }
        self.0.get()
    }
}

impl Drop for Win32Brush {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.0.get() as _);
        }
    }
}

fn background_brush() -> HBRUSH {
    thread_local! {
        static BACKGROUND_BRUSH: Win32Brush = const { Win32Brush::null() };
    }
    const BACKGROUND_COLOR: u32 = 2829099;
    BACKGROUND_BRUSH.with(|brush| brush.get_or_set(BACKGROUND_COLOR))
}

fn selected_background_brush() -> HBRUSH {
    thread_local! {
        static SELECTED_BACKGROUND_BRUSH: Win32Brush = const { Win32Brush::null() };
    }
    const SELECTED_BACKGROUND_COLOR: u32 = 4276545;
    SELECTED_BACKGROUND_BRUSH.with(|brush| brush.get_or_set(SELECTED_BACKGROUND_COLOR))
}

pub fn draw(hwnd: HWND, msg: u32, _wparam: WPARAM, lparam: LPARAM) {
    match msg {
        WM_NCACTIVATE | WM_NCPAINT => {
            let mut menu_bar_info = MENUBARINFO {
                cbSize: std::mem::size_of::<MENUBARINFO>() as _,
                ..unsafe { std::mem::zeroed() }
            };
            unsafe {
                GetMenuBarInfo(hwnd, OBJID_MENU, 0, &mut menu_bar_info);
            }

            let mut client_rect: RECT = unsafe { std::mem::zeroed() };
            unsafe {
                GetClientRect(hwnd, &mut client_rect);
                MapWindowPoints(
                    hwnd,
                    std::ptr::null_mut(),
                    &mut client_rect as *mut _ as *mut _,
                    2,
                );
            }

            let mut window_rect: RECT = unsafe { std::mem::zeroed() };
            unsafe {
                GetWindowRect(hwnd, &mut window_rect);
                OffsetRect(&mut client_rect, -window_rect.left, -window_rect.top);
            }

            let mut annoying_rect = client_rect;
            annoying_rect.bottom = annoying_rect.top;
            annoying_rect.top -= 1;

            unsafe {
                let hdc = GetWindowDC(hwnd);
                FillRect(hdc, &annoying_rect, background_brush());
                ReleaseDC(hwnd, hdc);
            }
        },
        WM_UAHDRAWMENU => {
            let menu = lparam as *const UAHMENU;

            let rect = {
                let mut menu_bar_info = MENUBARINFO {
                    cbSize: std::mem::size_of::<MENUBARINFO>() as _,
                    ..unsafe { std::mem::zeroed() }
                };
                unsafe {
                    GetMenuBarInfo(hwnd, OBJID_MENU, 0, &mut menu_bar_info);
                }

                let mut window_rect: RECT = unsafe { std::mem::zeroed() };
                unsafe {
                    GetWindowRect(hwnd, &mut window_rect);
                }

                let mut rect = menu_bar_info.rcBar;
                unsafe {
                    OffsetRect(&mut rect, -window_rect.left, -window_rect.top);
                }
                rect.top -= 1;
                rect
            };

            unsafe {
                FillRect((*menu).hdc, &rect, background_brush());
            }
        },
        WM_UAHDRAWMENUITEM => {
            let menu_item = lparam as *mut UAHDRAWMENUITEM;

            let (label, label_len) = {
                let label = Vec::<u16>::with_capacity(256);
                let mut info: MENUITEMINFOW = unsafe { std::mem::zeroed() };
                info.cbSize = std::mem::size_of::<MENUITEMINFOW>() as _;
                info.fMask = MIIM_STRING;
                info.dwTypeData = label.as_ptr() as *mut _;
                info.cch = (label.capacity() - 1) as _;
                unsafe {
                    GetMenuItemInfoW(
                        (*menu_item).um.hmenu,
                        (*menu_item).umi.iPosition,
                        true.into(),
                        &mut info,
                    );
                }
                (label, info.cch)
            };

            let mut draw_flags = DT_CENTER | DT_SINGLELINE | DT_VCENTER;
            let mut text_state = MPI_NORMAL;
            let mut background_state = MPI_NORMAL;

            unsafe {
                if (*menu_item).dis.itemState & ODS_HOTLIGHT != 0 {
                    text_state = MPI_HOT;
                    background_state = MPI_HOT;
                }
                if (*menu_item).dis.itemState & ODS_SELECTED != 0 {
                    text_state = MPI_HOT;
                    background_state = MPI_HOT;
                }
                if ((*menu_item).dis.itemState & ODS_GRAYED) != 0
                    || ((*menu_item).dis.itemState & ODS_DISABLED) != 0
                {
                    text_state = MPI_DISABLED;
                    background_state = MPI_DISABLED;
                }
                if (*menu_item).dis.itemState & ODS_NOACCEL != 0 {
                    draw_flags |= DT_HIDEPREFIX;
                }

                let background = match background_state {
                    MPI_HOT => selected_background_brush(),
                    _ => background_brush(),
                };
                FillRect((*menu_item).um.hdc, &(*menu_item).dis.rcItem, background);

                const TEXT_COLOR: u32 = 16777215;
                const DISABLED_TEXT_COLOR: u32 = 7171437;
                let text_color = match text_state {
                    MPI_DISABLED => DISABLED_TEXT_COLOR,
                    _ => TEXT_COLOR,
                };

                SetBkMode((*menu_item).um.hdc, 0);
                SetTextColor((*menu_item).um.hdc, text_color);
                DrawTextW(
                    (*menu_item).um.hdc,
                    label.as_ptr(),
                    label_len as _,
                    &mut (*menu_item).dis.rcItem,
                    draw_flags,
                );
            }
        },
        _ => {},
    }
}

pub fn should_use_dark_mode(hwnd: HWND) -> bool {
    should_apps_use_dark_mode() && !is_high_contrast() && is_dark_mode_allowed_for_window(hwnd)
}

static HUXTHEME: Lazy<isize> = Lazy::new(|| unsafe { LoadLibraryA(s!("uxtheme.dll")) as _ });
static HNTDLL: Lazy<isize> = Lazy::new(|| unsafe { LoadLibraryA(s!("ntdll.dll")) as _ });
static WIN10_BUILD_VERSION: Lazy<Option<u32>> = Lazy::new(|| {
    // Reference: winit/src/platform_impl/windows/dark_mode.rs::WIN10_BUILD_VERSION.
    type RtlGetVersion = unsafe extern "system" fn(*mut OSVERSIONINFOW) -> NTSTATUS;
    static RTL_GET_VERSION: Lazy<Option<RtlGetVersion>> = Lazy::new(|| unsafe {
        if *HNTDLL == 0 {
            return None;
        }

        GetProcAddress((*HNTDLL) as *mut _, b"RtlGetVersion\0".as_ptr())
            .map(|handle| std::mem::transmute(handle))
    });

    if let Some(rtl_get_version) = *RTL_GET_VERSION {
        let mut version = OSVERSIONINFOW {
            dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as _,
            dwMajorVersion: 0,
            dwMinorVersion: 0,
            dwBuildNumber: 0,
            dwPlatformId: 0,
            szCSDVersion: [0; 128],
        };

        let status = unsafe { rtl_get_version(&mut version) };
        if status >= 0 && version.dwMajorVersion == 10 && version.dwMinorVersion == 0 {
            Some(version.dwBuildNumber)
        } else {
            None
        }
    } else {
        None
    }
});
static DARK_MODE_SUPPORTED: Lazy<bool> = Lazy::new(|| match *WIN10_BUILD_VERSION {
    Some(build) => build >= 17763,
    None => false,
});

pub fn enable_dark_mode_for_app() {
    static ENABLE_DARK_MODE_FOR_APP: Once = Once::new();
    ENABLE_DARK_MODE_FOR_APP.call_once(|| {
        if !*DARK_MODE_SUPPORTED {
            return;
        }

        // Reference:
        // tao/src/platform_impl/windows/dark_mode.rs::allow_dark_mode_for_app.
        const UXTHEME_ALLOWDARKMODEFORAPP_ORDINAL: u16 = 135;
        type AllowDarkModeForApp = unsafe extern "system" fn(bool) -> bool;
        static ALLOW_DARK_MODE_FOR_APP: Lazy<Option<AllowDarkModeForApp>> = Lazy::new(|| unsafe {
            if *HUXTHEME == 0 {
                return None;
            }

            GetProcAddress(
                (*HUXTHEME) as *mut _,
                UXTHEME_ALLOWDARKMODEFORAPP_ORDINAL as usize as *mut _,
            )
            .map(|handle| std::mem::transmute(handle))
        });

        #[allow(dead_code)]
        #[repr(C)]
        enum PreferredAppMode {
            Default,
            AllowDark,
        }

        const UXTHEME_SETPREFERREDAPPMODE_ORDINAL: u16 = 135;
        type SetPreferredAppMode = unsafe extern "system" fn(PreferredAppMode) -> PreferredAppMode;
        static SET_PREFERRED_APP_MODE: Lazy<Option<SetPreferredAppMode>> = Lazy::new(|| unsafe {
            if *HUXTHEME == 0 {
                return None;
            }

            GetProcAddress(
                (*HUXTHEME) as *mut _,
                UXTHEME_SETPREFERREDAPPMODE_ORDINAL as usize as *mut _,
            )
            .map(|handle| std::mem::transmute(handle))
        });

        match *WIN10_BUILD_VERSION {
            Some(build) if build < 18362 => {
                if let Some(allow_dark_mode_for_app) = *ALLOW_DARK_MODE_FOR_APP {
                    unsafe { allow_dark_mode_for_app(true) };
                }
            },
            Some(_) => {
                if let Some(set_preferred_app_mode) = *SET_PREFERRED_APP_MODE {
                    unsafe { set_preferred_app_mode(PreferredAppMode::AllowDark) };
                }
            },
            None => {},
        }

        refresh_immersive_color_policy_state();
    });
}

pub fn enable_dark_mode_for_window(hwnd: HWND) {
    // Reference:
    // tao/src/platform_impl/windows/dark_mode.rs::allow_dark_mode_for_window.
    const UXTHEME_ALLOWDARKMODEFORWINDOW_ORDINAL: u16 = 133;
    type AllowDarkModeForWindow = unsafe extern "system" fn(HWND, bool) -> bool;
    static ALLOW_DARK_MODE_FOR_WINDOW: Lazy<Option<AllowDarkModeForWindow>> =
        Lazy::new(|| unsafe {
            if *HUXTHEME == 0 {
                return None;
            }

            GetProcAddress(
                (*HUXTHEME) as *mut _,
                UXTHEME_ALLOWDARKMODEFORWINDOW_ORDINAL as usize as *mut _,
            )
            .map(|handle| std::mem::transmute(handle))
        });

    if *DARK_MODE_SUPPORTED {
        if let Some(allow_dark_mode_for_window) = *ALLOW_DARK_MODE_FOR_WINDOW {
            unsafe { allow_dark_mode_for_window(hwnd, true) };
        }
    }
}

fn refresh_immersive_color_policy_state() {
    // Reference:
    // tao/src/platform_impl/windows/dark_mode.
    // rs::refresh_immersive_color_policy_state.
    const UXTHEME_REFRESHIMMERSIVECOLORPOLICYSTATE_ORDINAL: u16 = 104;
    type RefreshImmersiveColorPolicyState = unsafe extern "system" fn();
    static REFRESH_IMMERSIVE_COLOR_POLICY_STATE: Lazy<Option<RefreshImmersiveColorPolicyState>> =
        Lazy::new(|| unsafe {
            if *HUXTHEME == 0 {
                return None;
            }

            GetProcAddress(
                (*HUXTHEME) as *mut _,
                UXTHEME_REFRESHIMMERSIVECOLORPOLICYSTATE_ORDINAL as usize as *mut _,
            )
            .map(|handle| std::mem::transmute(handle))
        });

    if let Some(refresh_immersive_color_policy_state) = *REFRESH_IMMERSIVE_COLOR_POLICY_STATE {
        unsafe { refresh_immersive_color_policy_state() };
    }
}

fn should_apps_use_dark_mode() -> bool {
    const UXTHEME_SHOULDAPPSUSEDARKMODE_ORDINAL: u16 = 132;
    type ShouldAppsUseDarkMode = unsafe extern "system" fn() -> bool;
    static SHOULD_APPS_USE_DARK_MODE: Lazy<Option<ShouldAppsUseDarkMode>> = Lazy::new(|| unsafe {
        if *HUXTHEME == 0 {
            return None;
        }

        GetProcAddress(
            (*HUXTHEME) as *mut _,
            UXTHEME_SHOULDAPPSUSEDARKMODE_ORDINAL as usize as *mut _,
        )
        .map(|handle| std::mem::transmute(handle))
    });

    SHOULD_APPS_USE_DARK_MODE
        .map(|function| unsafe { function() })
        .unwrap_or(false)
}

fn is_dark_mode_allowed_for_window(hwnd: HWND) -> bool {
    const UXTHEME_ISDARKMODEALLOWEDFORWINDOW_ORDINAL: u16 = 137;
    type IsDarkModeAllowedForWindow = unsafe extern "system" fn(HWND) -> bool;
    static IS_DARK_MODE_ALLOWED_FOR_WINDOW: Lazy<Option<IsDarkModeAllowedForWindow>> =
        Lazy::new(|| unsafe {
            if *HUXTHEME == 0 {
                return None;
            }

            GetProcAddress(
                (*HUXTHEME) as *mut _,
                UXTHEME_ISDARKMODEALLOWEDFORWINDOW_ORDINAL as usize as *mut _,
            )
            .map(|handle| std::mem::transmute(handle))
        });

    if let Some(function) = *IS_DARK_MODE_ALLOWED_FOR_WINDOW {
        unsafe { function(hwnd) }
    } else {
        false
    }
}

fn is_high_contrast() -> bool {
    const HCF_HIGHCONTRASTON: u32 = 1;

    let mut high_contrast = HIGHCONTRASTA {
        cbSize: 0,
        dwFlags: Default::default(),
        lpszDefaultScheme: std::ptr::null_mut(),
    };

    let ok = unsafe {
        SystemParametersInfoA(
            SPI_GETHIGHCONTRAST,
            std::mem::size_of_val(&high_contrast) as _,
            &mut high_contrast as *mut _ as _,
            Default::default(),
        )
    };

    ok != 0 && (HCF_HIGHCONTRASTON & high_contrast.dwFlags) != 0
}
