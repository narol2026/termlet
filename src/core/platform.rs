//! Platform services that iced does not expose directly.
//!
//! The main one is a live pointer query: OS drag-and-drop callbacks deliver
//! files but not coordinates, and some drag loops do not emit ordinary cursor
//! events to the drop target.

use iced::Point;

/// The pointer in the app window's logical client coordinates.
///
/// Drag-and-drop callbacks identify the dropped file but not where the pointer
/// is. On Windows the OLE drag loop runs in the source application and the
/// drop target receives no `CursorMoved` events, so the last event position is
/// not a usable drop location. Ask the platform where the pointer actually is.
///
/// `window_position` and `scale_factor` are iced's logical window origin and
/// scale. Windows answers from the native window handle instead and ignores
/// them.
pub fn cursor_position_in_window(
    window_position: Option<Point>,
    scale_factor: f32,
) -> Option<Point> {
    #[cfg(target_os = "windows")]
    {
        win32::cursor_position_in_window(window_position, scale_factor)
    }
    #[cfg(not(target_os = "windows"))]
    {
        screen_to_window(global_cursor_position()?, window_position?, scale_factor)
    }
}

/// Register the native window whose client space [`cursor_position_in_window`]
/// answers in. Called once the platform's window hook has the real handle.
#[cfg(target_os = "windows")]
pub fn register_window(hwnd: isize) {
    win32::register_window(hwnd);
}

/// Drop the native handle when its window is being destroyed.
#[cfg(target_os = "windows")]
pub fn unregister_window(hwnd: isize) {
    win32::unregister_window(hwnd);
}

/// Map a screen-space point into the window's logical client space.
#[cfg(any(not(target_os = "windows"), test))]
fn screen_to_window(global: Point, window_position: Point, scale_factor: f32) -> Option<Point> {
    if !scale_factor.is_finite() || scale_factor <= 0.0 {
        return None;
    }

    Some(Point::new(
        global.x / scale_factor - window_position.x,
        global.y / scale_factor - window_position.y,
    ))
}

#[cfg(target_os = "windows")]
fn client_to_logical(x: i32, y: i32, dpi: u32) -> Point {
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
    Point::new(x as f32 / scale, y as f32 / scale)
}

#[cfg(target_os = "windows")]
mod win32 {
    use std::sync::atomic::{AtomicIsize, Ordering};

    use iced::Point;
    use windows_sys::Win32::Foundation::{HWND, POINT};
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    use super::client_to_logical;

    /// Termlet opens one window per process, so one process-wide handle is
    /// enough. It is registered by the Windows chrome setup as the window
    /// opens and cleared when that window is destroyed.
    static WINDOW_HANDLE: AtomicIsize = AtomicIsize::new(0);

    pub fn register_window(hwnd: isize) {
        WINDOW_HANDLE.store(hwnd, Ordering::SeqCst);
    }

    pub fn unregister_window(hwnd: isize) {
        let _ = WINDOW_HANDLE.compare_exchange(hwnd, 0, Ordering::SeqCst, Ordering::SeqCst);
    }

    /// Convert the live cursor from screen space straight into this window's
    /// client space. `ScreenToClient` uses the real window origin, so this does
    /// not depend on the cached window position being current.
    pub fn cursor_position_in_window(
        _window_position: Option<Point>,
        _scale_factor: f32,
    ) -> Option<Point> {
        let hwnd = WINDOW_HANDLE.load(Ordering::SeqCst);
        if hwnd == 0 {
            return None;
        }
        let hwnd = hwnd as HWND;

        let mut point = POINT { x: 0, y: 0 };
        // SAFETY: `point` is a valid writable POINT and GetCursorPos has no
        // additional preconditions.
        if unsafe { GetCursorPos(&raw mut point) } == 0 {
            return None;
        }
        // SAFETY: `hwnd` is the app window registered by the chrome setup and
        // `point` is a valid readable and writable POINT.
        if unsafe { ScreenToClient(hwnd, &raw mut point) } == 0 {
            return None;
        }
        // SAFETY: `hwnd` is live because ScreenToClient accepted it above.
        let dpi = unsafe { GetDpiForWindow(hwnd) };

        Some(client_to_logical(point.x, point.y, dpi))
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn global_cursor_position() -> Option<Point> {
    use std::ffi::{CStr, c_void};
    use std::mem;
    use std::os::raw::{c_char, c_int, c_uint, c_ulong};
    use std::ptr;

    #[allow(
        non_camel_case_types,
        reason = "mirrors the C type name it is declared against"
    )]
    enum Display {}

    type XOpenDisplay = unsafe extern "C" fn(*const c_char) -> *mut Display;
    type XCloseDisplay = unsafe extern "C" fn(*mut Display) -> c_int;
    type XDefaultRootWindow = unsafe extern "C" fn(*mut Display) -> c_ulong;
    type XQueryPointer = unsafe extern "C" fn(
        *mut Display,
        c_ulong,
        *mut c_ulong,
        *mut c_ulong,
        *mut c_int,
        *mut c_int,
        *mut c_int,
        *mut c_int,
        *mut c_uint,
    ) -> c_int;

    /// Resolve `name` in `library` and reinterpret the symbol as `T`.
    ///
    /// # Safety
    ///
    /// `library` must be an open handle returned by [`libc::dlopen`] and `T`
    /// must be the exact function-pointer type the symbol is declared with —
    /// `dlsym` cannot check either, and calling the result through any other
    /// signature is undefined behaviour.
    unsafe fn symbol<T>(library: *mut c_void, name: &CStr) -> Option<T> {
        // SAFETY: `library` is the handle opened below and still open for the
        // whole call, and every `name` passed in is a `'static`,
        // NUL-terminated C string.
        let pointer = unsafe { libc::dlsym(library, name.as_ptr()) };
        if pointer.is_null() {
            None
        } else {
            // SAFETY: each caller instantiates `T` with the `X*` function
            // pointer type declared for the symbol it looks up, which is the
            // contract documented above.
            Some(unsafe { mem::transmute_copy(&pointer) })
        }
    }

    // SAFETY: the library name is a `'static`, NUL-terminated C string and
    // `dlopen` returns either a handle or null, which is checked. Every symbol
    // is resolved through `symbol` before use and cast to the function-pointer
    // type declared for it above, so the calls below match the X11 ABI. The
    // handle is closed on each path that leaves the function.
    unsafe {
        let library = libc::dlopen(c"libX11.so.6".as_ptr(), libc::RTLD_LAZY);
        if library.is_null() {
            return None;
        }

        let open_display: XOpenDisplay = if let Some(function) = symbol(library, c"XOpenDisplay") {
            function
        } else {
            libc::dlclose(library);
            return None;
        };
        let close_display: XCloseDisplay = if let Some(function) = symbol(library, c"XCloseDisplay")
        {
            function
        } else {
            libc::dlclose(library);
            return None;
        };
        let default_root_window: XDefaultRootWindow =
            if let Some(function) = symbol(library, c"XDefaultRootWindow") {
                function
            } else {
                libc::dlclose(library);
                return None;
            };
        let query_pointer: XQueryPointer = if let Some(function) = symbol(library, c"XQueryPointer")
        {
            function
        } else {
            libc::dlclose(library);
            return None;
        };

        let display = open_display(ptr::null());
        if display.is_null() {
            libc::dlclose(library);
            return None;
        }

        let root = default_root_window(display);
        let mut root_return = 0;
        let mut child_return = 0;
        let mut root_x = 0;
        let mut root_y = 0;
        let mut win_x = 0;
        let mut win_y = 0;
        let mut mask = 0;
        let found = query_pointer(
            display,
            root,
            &raw mut root_return,
            &raw mut child_return,
            &raw mut root_x,
            &raw mut root_y,
            &raw mut win_x,
            &raw mut win_y,
            &raw mut mask,
        ) != 0;

        close_display(display);
        libc::dlclose(library);

        found.then(|| Point::new(root_x as f32, root_y as f32))
    }
}

#[cfg(target_os = "macos")]
fn global_cursor_position() -> Option<Point> {
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).ok()?;
    let event = CGEvent::new(source).ok()?;
    let point = event.location();
    Some(Point::new(point.x as f32, point.y as f32))
}

#[cfg(all(not(unix), not(target_os = "windows")))]
fn global_cursor_position() -> Option<Point> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_points_are_mapped_through_the_window_scale_and_origin() {
        assert_eq!(
            screen_to_window(Point::new(450.0, 300.0), Point::new(100.0, 50.0), 1.5),
            Some(Point::new(200.0, 150.0))
        );
    }

    #[test]
    fn invalid_scale_has_no_window_coordinates() {
        assert_eq!(
            screen_to_window(Point::new(10.0, 10.0), Point::new(0.0, 0.0), 0.0),
            None
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn physical_client_points_use_the_window_dpi() {
        assert_eq!(client_to_logical(300, 150, 144), Point::new(200.0, 100.0));
        assert_eq!(client_to_logical(300, 150, 0), Point::new(300.0, 150.0));
    }
}
