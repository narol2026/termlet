//! First-frame presentation for the Windows main window.
//!
//! iced creates its window hidden, then makes it visible just before the
//! first redraw is presented. On Windows that gap can leak an unpainted,
//! borderless client area through DWM. Keep the window cloaked while it is
//! made visible so it can receive `WM_PAINT`, then remove the cloak only after
//! iced has drawn the first frame.

use std::sync::atomic::{AtomicBool, Ordering};

use iced::window::raw_window_handle::RawWindowHandle;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAK, DwmFlush, DwmSetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{
    RDW_ALLCHILDREN, RDW_INVALIDATE, RDW_UPDATENOW, RedrawWindow,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{SW_SHOWNOACTIVATE, ShowWindow};

/// Whether this process hid the window with DWM's cloak attribute.
static CLOAKED: AtomicBool = AtomicBool::new(false);

/// Make the window visible in Win32 terms so it receives its first paint,
/// while DWM keeps it off screen until [`reveal_first_frame`] runs.
pub(crate) fn show_cloaked(window: &dyn iced::window::Window) {
    let Some(hwnd) = win32_hwnd(window) else {
        return;
    };

    let cloak = windows_sys::core::BOOL::from(true);
    // SAFETY: `hwnd` comes from the live window behind `window`, and `cloak`
    // points to an initialized BOOL of the exact size passed to the API.
    let result = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_CLOAK as u32,
            (&raw const cloak).cast(),
            size_of_val(&cloak) as u32,
        )
    };
    if result >= 0 {
        CLOAKED.store(true, Ordering::SeqCst);
        // Make sure DWM has applied the cloak before the window becomes
        // WS_VISIBLE; otherwise the blank surface could still leak for a frame.
        //
        // SAFETY: DwmFlush takes no pointers and only waits for the compositor.
        unsafe { DwmFlush() };
    }

    // Match winit's first `set_visible(true)` call. If cloaking is unavailable,
    // showing the window is still preferable to leaving the app invisible.
    //
    // SAFETY: `hwnd` is a live top-level window and SW_SHOWNOACTIVATE is a
    // documented ShowWindow command.
    unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };

    // A cloaked window does not naturally receive the first WM_PAINT. Trigger
    // it explicitly so iced runs its redraw path and can reveal us afterwards.
    //
    // SAFETY: `hwnd` is live; a null update rectangle invalidates the whole
    // client area, and the flags request a synchronous paint.
    unsafe {
        RedrawWindow(
            hwnd,
            std::ptr::null(),
            std::ptr::null_mut(),
            RDW_INVALIDATE | RDW_UPDATENOW | RDW_ALLCHILDREN,
        )
    };
}

/// Reveal the window after iced has presented its first frame.
pub(crate) fn reveal_first_frame(window: &dyn iced::window::Window) {
    if !CLOAKED.swap(false, Ordering::SeqCst) {
        return;
    }

    let Some(hwnd) = win32_hwnd(window) else {
        return;
    };
    let cloak = windows_sys::core::BOOL::from(false);
    // SAFETY: `hwnd` comes from the live window behind `window`, and `cloak`
    // points to an initialized BOOL of the exact size passed to the API.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_CLOAK as u32,
            (&raw const cloak).cast(),
            size_of_val(&cloak) as u32,
        )
    };
}

fn win32_hwnd(window: &dyn iced::window::Window) -> Option<HWND> {
    let window_handle = window.window_handle().ok()?;
    let RawWindowHandle::Win32(handle) = window_handle.as_raw() else {
        return None;
    };
    Some(handle.hwnd.get() as HWND)
}
