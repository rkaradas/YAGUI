//! Makes Windows show YAGUI's own icon in the taskbar.
//!
//! The window already sends its icon through eframe, but the taskbar groups windows by the
//! program's "app id" and may keep an older, cached or pinned picture. So YAGUI
//! - gives itself an explicit app id before any window exists, and
//! - sets the icon embedded in the .exe (resource 1, see build.rs) as the window's big and small
//!   icon and as its window-class icon, which the taskbar falls back to.

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GCLP_HICON, GCLP_HICONSM, GetSystemMetrics, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW, SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW, SetClassLongPtrW, WM_SETICON,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn set_app_id() {
        let id = wide("YAGUI.GitClient");
        // SAFETY: plain Win32 call with a valid, zero-terminated string.
        unsafe {
            SetCurrentProcessExplicitAppUserModelID(id.as_ptr());
        }
    }

    pub fn apply(hwnd: isize) {
        // SAFETY: Win32 calls on our own window with an icon from our own module.
        unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            if module.is_null() {
                return;
            }
            let res = 1usize as *const u16; // MAKEINTRESOURCE(1)
            let big = LoadImageW(module, res, IMAGE_ICON, GetSystemMetrics(SM_CXICON), GetSystemMetrics(SM_CYICON), LR_DEFAULTCOLOR);
            let small = LoadImageW(module, res, IMAGE_ICON, GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON), LR_DEFAULTCOLOR);
            let h = hwnd as *mut core::ffi::c_void;
            if !big.is_null() {
                SendMessageW(h, WM_SETICON, ICON_BIG as usize, big as isize);
                SetClassLongPtrW(h, GCLP_HICON, big as isize);
            }
            if !small.is_null() {
                SendMessageW(h, WM_SETICON, ICON_SMALL as usize, small as isize);
                SetClassLongPtrW(h, GCLP_HICONSM, small as isize);
            }
        }
    }
}

/// Call once at start, before the window is created.
pub fn set_app_id() {
    #[cfg(windows)]
    imp::set_app_id();
}

/// Call with the frame for the first frames: sets the taskbar and window icons.
pub fn apply(frame: &eframe::Frame) {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        if let Ok(h) = frame.window_handle() {
            if let RawWindowHandle::Win32(w) = h.as_raw() {
                imp::apply(w.hwnd.get());
            }
        }
    }
    #[cfg(not(windows))]
    let _ = frame;
}
