//! Windows native title-bar hit testing.
//!
//! The window is borderless, so Windows normally treats the whole client area
//! as `HTCLIENT`. That leaves iced to emulate window dragging, and starting a
//! drag on the first press swallows the second press of a double-click. This
//! module teaches Windows about the one part of the client area that is really
//! a title bar: `WM_NCHITTEST` answers `HTCAPTION` for the top bar's empty
//! space, while the interactive rectangles recorded by the view keep answering
//! `HTCLIENT`.
//!
//! The maximize button is the exception: answering `HTMAXBUTTON` lets Windows
//! 11 show its Snap Layouts flyout on hover and keeps the native
//! maximize/restore behavior on click.
//!
//! The view owns the geometry. [`begin_frame`] resets the registry, and
//! `ui::native_titlebar` records the caption and client rectangles while the
//! top bar is drawn. The window procedure only reads that registry. Keeping the
//! geometry out of this module means the hit test follows the real widget
//! layout instead of duplicating tab widths and button padding here.
//!
//! Termlet opens one window per process, so a process-wide registry is enough.
//! If that ever changes, the registry has to be keyed by window handle.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use iced::{Point, Rectangle, Vector};

/// `WM_NCHITTEST`: asks where the pointer landed in a window.
const WM_NCHITTEST: u32 = 0x0084;
/// `WM_NCLBUTTONDBLCLK`: a double-click in a non-client area.
const WM_NCLBUTTONDBLCLK: u32 = 0x00A3;
/// `WM_NCLBUTTONDOWN`: a button press in a non-client area.
const WM_NCLBUTTONDOWN: u32 = 0x00A1;
/// `WM_NCLBUTTONUP`: a button release in a non-client area.
const WM_NCLBUTTONUP: u32 = 0x00A2;
/// `WM_NCDESTROY`: the window is going away.
const WM_NCDESTROY: u32 = 0x0082;
/// `WM_NCMOUSEMOVE`: pointer movement over the native caption.
const WM_NCMOUSEMOVE: u32 = 0x00A0;
/// `WM_NCMOUSELEAVE`: the pointer left the native caption.
const WM_NCMOUSELEAVE: u32 = 0x02A2;
/// Client-area counterparts used to keep iced's pointer and hover state live.
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_MOUSELEAVE: u32 = 0x02A3;

/// The pointer is over an ordinary client-control.
const HTCLIENT: isize = 1;
/// The pointer is over the title bar.
const HTCAPTION: isize = 2;
/// The pointer is over the maximize/restore button. Windows uses this hit to
/// show the Snap Layouts flyout on Windows 11.
const HTMAXBUTTON: isize = 9;

/// `SW_MAXIMIZE` for [`ShowWindow`].
const SW_MAXIMIZE: i32 = 3;
/// `SW_RESTORE` for [`ShowWindow`].
const SW_RESTORE: i32 = 9;

/// A private subclass id for this application.
const SUBCLASS_ID: usize = 0x5445_524D;

type Hwnd = *mut c_void;
type SubclassProc =
    Option<unsafe extern "system" fn(Hwnd, u32, usize, isize, usize, usize) -> isize>;

#[repr(C)]
struct WinPoint {
    x: i32,
    y: i32,
}

#[link(name = "comctl32")]
unsafe extern "system" {
    fn SetWindowSubclass(hwnd: Hwnd, proc: SubclassProc, id: usize, data: usize) -> i32;
    fn DefSubclassProc(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn RemoveWindowSubclass(hwnd: Hwnd, proc: SubclassProc, id: usize) -> i32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn ScreenToClient(hwnd: Hwnd, point: *mut WinPoint) -> i32;
    fn IsZoomed(hwnd: Hwnd) -> i32;
    fn ShowWindow(hwnd: Hwnd, command: i32) -> i32;
    fn SendMessageW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize;
    fn GetDpiForWindow(hwnd: Hwnd) -> u32;
}

/// Whether the subclass procedure has been installed for this process.
static INSTALLED: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
struct Zones {
    /// The top bar's complete bounds, in logical window coordinates.
    caption: Option<Rectangle>,
    /// Controls that must keep receiving ordinary mouse events.
    client: Vec<Rectangle>,
    /// The maximize/restore button. It is also covered by its parent client
    /// zone, so it has to win the hit test.
    maximize: Option<Rectangle>,
    /// Visible part of the horizontally scrollable tab strip. Tab rectangles
    /// are clipped to this so tabs scrolled out of view cannot punch holes in
    /// the native drag area.
    tab_strip_clip: Option<Rectangle>,
}

impl Zones {
    const fn new() -> Self {
        Self {
            caption: None,
            client: Vec::new(),
            maximize: None,
            tab_strip_clip: None,
        }
    }
}

impl Default for Zones {
    fn default() -> Self {
        Self::new()
    }
}

/// The last fully drawn frame. `WM_NCHITTEST` always reads this snapshot.
static ACTIVE: Mutex<Zones> = Mutex::new(Zones::new());

/// Rectangles collected while the current frame is being drawn.
static STAGING: Mutex<Zones> = Mutex::new(Zones::new());

fn active() -> MutexGuard<'static, Zones> {
    ACTIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn staging() -> MutexGuard<'static, Zones> {
    STAGING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Start a frame of top-bar geometry. Called by the top-bar view before any
/// widget records its bounds.
pub fn begin_frame() {
    let mut staging = staging();
    staging.caption = None;
    staging.client.clear();
    staging.maximize = None;
    staging.tab_strip_clip = None;
}

/// Record the complete top-bar bounds.
pub fn record_caption(bounds: Rectangle) {
    staging().caption = Some(bounds);
}

/// Record an interactive control that must remain `HTCLIENT`.
pub fn record_client(bounds: Rectangle) {
    staging().client.push(bounds);
}

/// Record the maximize/restore button as the native `HTMAXBUTTON` zone.
pub fn record_maximize(bounds: Rectangle) {
    staging().maximize = Some(bounds);
}

/// Record the visible tab-strip viewport. Tabs drawn after this call are
/// clipped to it.
pub fn record_tab_strip_clip(bounds: Rectangle) {
    staging().tab_strip_clip = Some(bounds);
}

/// Record an interactive tab drawn inside the horizontally scrolled tab
/// strip. `viewport` is the translated viewport the scrollable passes to its
/// content, which lets us recover the offset it is drawing with right now.
pub fn record_scrolled_client(bounds: Rectangle, viewport: Rectangle) {
    let mut zones = staging();
    let Some(clip) = zones.tab_strip_clip else {
        return;
    };
    let Some(visible) = scrolled_client_bounds(bounds, viewport, clip) else {
        return;
    };
    zones.client.push(visible);
}

fn scrolled_client_bounds(
    bounds: Rectangle,
    viewport: Rectangle,
    clip: Rectangle,
) -> Option<Rectangle> {
    let scroll = Vector::new(viewport.x - clip.x, viewport.y - clip.y);
    let shifted = Rectangle {
        x: bounds.x - scroll.x,
        y: bounds.y - scroll.y,
        ..bounds
    };
    shifted.intersection(&clip)
}

/// Promote the rectangles from the frame that just finished drawing.
pub fn commit_frame() {
    let mut staging = staging();
    if staging.caption.is_none() {
        return;
    }

    let mut active = active();
    *active = std::mem::take(&mut *staging);
}

/// Install the hit-test subclass for `window`.
///
/// Safe to call repeatedly; only the first successful call installs the
/// procedure.
pub fn sync(window: &dyn iced::window::Window) {
    use iced::window::raw_window_handle::RawWindowHandle;

    if INSTALLED.load(Ordering::SeqCst) {
        return;
    }

    let Ok(window_handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = window_handle.as_raw() else {
        return;
    };

    let hwnd = handle.hwnd.get() as Hwnd;
    // Cursor queries during a drag need the real window even before (or
    // without) the hit-test subclass, so record it independently of whether
    // `SetWindowSubclass` succeeds below.
    crate::core::platform::register_window(handle.hwnd.get());
    // SAFETY: `hwnd` belongs to the live window behind `window_handle`, which
    // is kept alive by the runtime for this callback. The subclass procedure
    // forwards every message it does not own through `DefSubclassProc`.
    let installed = unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0) };
    if installed != 0 {
        INSTALLED.store(true, Ordering::SeqCst);
    }
}

fn window_x(lparam: isize) -> i32 {
    i32::from((lparam as u32 & 0xffff) as u16 as i16)
}

fn window_y(lparam: isize) -> i32 {
    i32::from(((lparam as u32 >> 16) & 0xffff) as u16 as i16)
}

fn classify(
    point: Point,
    caption: Rectangle,
    maximize: Option<Rectangle>,
    client: &[Rectangle],
) -> Option<isize> {
    if !caption.contains(point) {
        return None;
    }

    if maximize.is_some_and(|bounds| bounds.contains(point)) {
        Some(HTMAXBUTTON)
    } else if client.iter().any(|bounds| bounds.contains(point)) {
        Some(HTCLIENT)
    } else {
        Some(HTCAPTION)
    }
}

/// Convert a screen-space message coordinate into packed client coordinates.
fn screen_to_client(hwnd: Hwnd, lparam: isize) -> Option<WinPoint> {
    let mut point = WinPoint {
        x: window_x(lparam),
        y: window_y(lparam),
    };
    // SAFETY: `hwnd` is the live window receiving the message, and `point`
    // points at a valid, writable POINT.
    let converted = unsafe { ScreenToClient(hwnd, &raw mut point) };
    if converted == 0 {
        return None;
    }
    Some(point)
}

fn client_lparam(hwnd: Hwnd, lparam: isize) -> Option<isize> {
    let point = screen_to_client(hwnd, lparam)?;
    let x = u32::from(point.x as u16);
    let y = u32::from(point.y as u16);
    Some(((y << 16) | x) as isize)
}

/// Convert screen-space message coordinates into the same logical coordinates
/// the view records its zones in.
fn logical_point(hwnd: Hwnd, lparam: isize) -> Option<Point> {
    let point = screen_to_client(hwnd, lparam)?;
    // SAFETY: `hwnd` is the live window receiving the mouse message.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
    Some(Point::new(point.x as f32 / scale, point.y as f32 / scale))
}

/// Translate a window message into a top-bar hit, if the registry is ready.
fn hit_test(hwnd: Hwnd, lparam: isize) -> Option<isize> {
    let zones = active();
    let caption = zones.caption?;

    let logical = logical_point(hwnd, lparam)?;
    classify(logical, caption, zones.maximize, &zones.client)
}

unsafe extern "system" fn subclass_proc(
    hwnd: Hwnd,
    msg: u32,
    wparam: usize,
    lparam: isize,
    _id: usize,
    _data: usize,
) -> isize {
    match msg {
        WM_NCHITTEST => {
            if let Some(hit) = hit_test(hwnd, lparam) {
                return hit;
            }
        }
        WM_NCLBUTTONDOWN if wparam == HTMAXBUTTON as usize => {
            // Returning `HTMAXBUTTON` makes Windows show Snap Layouts, but it
            // also means Windows no longer delivers an ordinary client click.
            // Swallow the press so DefWindowProc does not start a caption drag;
            // the release below restores the button's click behavior.
            return 0;
        }
        WM_NCLBUTTONUP if wparam == HTMAXBUTTON as usize => {
            // `wParam` carries the hit code for the release position, so this
            // only fires when the pointer is still over the button.
            // SAFETY: `hwnd` is the live window receiving the release.
            let maximized = unsafe { IsZoomed(hwnd) } != 0;
            // SAFETY: `hwnd` is live and both commands are documented
            // `ShowWindow` commands.
            unsafe { ShowWindow(hwnd, if maximized { SW_RESTORE } else { SW_MAXIMIZE }) };
            return 0;
        }
        WM_NCLBUTTONDBLCLK if wparam == HTMAXBUTTON as usize => {
            // The release that follows this message performs the toggle. Do
            // not let DefWindowProc apply extra caption behavior to the second
            // click of a double-click on the maximize button.
            return 0;
        }
        WM_NCLBUTTONDBLCLK if wparam == HTCAPTION as usize => {
            // SAFETY: `hwnd` is the live window receiving the double-click.
            let maximized = unsafe { IsZoomed(hwnd) } != 0;
            // SAFETY: `hwnd` is live and the command is one of the documented
            // `ShowWindow` commands.
            unsafe { ShowWindow(hwnd, if maximized { SW_RESTORE } else { SW_MAXIMIZE }) };
            return 0;
        }
        WM_NCMOUSEMOVE => {
            // iced still draws and animates the top bar itself, so it needs to
            // see pointer movement over the native caption. Without this,
            // hover state from the last client-area position would stick.
            // Deliver it synchronously: when the pointer is still moving as
            // the user presses, a queued move would arrive inside the native
            // move loop and can interrupt the drag.
            if let Some(point) = client_lparam(hwnd, lparam) {
                // SAFETY: `hwnd` is live and the packed coordinates are the
                // pointer's current client position.
                unsafe { SendMessageW(hwnd, WM_MOUSEMOVE, 0, point) };
            }
        }
        WM_NCMOUSELEAVE => {
            // SAFETY: `hwnd` is live; the message carries no payload.
            unsafe { SendMessageW(hwnd, WM_MOUSELEAVE, 0, 0) };
        }
        WM_NCDESTROY => {
            // SAFETY: this is the same window/procedure/id pair installed in
            // `sync`; removal is required before the handle becomes invalid.
            unsafe { RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID) };
            crate::core::platform::unregister_window(hwnd as isize);
            INSTALLED.store(false, Ordering::SeqCst);
        }
        _ => {}
    }

    // SAFETY: every message this procedure does not handle must continue down
    // the original window procedure chain.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Size;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(width, height))
    }

    #[test]
    fn client_zones_win_over_the_caption() {
        let caption = rect(0.0, 0.0, 800.0, 46.0);
        let client = [rect(10.0, 6.0, 120.0, 34.0)];

        assert_eq!(
            classify(Point::new(20.0, 20.0), caption, None, &client),
            Some(HTCLIENT)
        );
        assert_eq!(
            classify(Point::new(400.0, 20.0), caption, None, &client),
            Some(HTCAPTION)
        );
    }

    #[test]
    fn the_maximize_button_wins_over_its_parent_client_zone() {
        let caption = rect(0.0, 0.0, 800.0, 46.0);
        let controls = [rect(600.0, 0.0, 200.0, 46.0)];
        let maximize = rect(730.0, 0.0, 36.0, 46.0);

        assert_eq!(
            classify(Point::new(740.0, 20.0), caption, Some(maximize), &controls),
            Some(HTMAXBUTTON)
        );
        assert_eq!(
            classify(Point::new(620.0, 20.0), caption, Some(maximize), &controls),
            Some(HTCLIENT),
            "the rest of the window controls must stay client-side"
        );
    }

    #[test]
    fn points_outside_the_caption_stay_with_windows() {
        let caption = rect(0.0, 0.0, 800.0, 46.0);

        assert_eq!(classify(Point::new(20.0, 80.0), caption, None, &[]), None);
    }

    #[test]
    fn screen_coordinates_decode_as_signed_words() {
        let x = -1920i32;
        let y = 240i32;
        let lparam = ((u32::from(y as u16) << 16) | u32::from(x as u16)) as isize;

        assert_eq!(window_x(lparam), x);
        assert_eq!(window_y(lparam), y);
    }

    #[test]
    fn scrolled_tabs_are_translated_and_clipped_to_the_visible_strip() {
        let clip = rect(200.0, 6.0, 300.0, 34.0);
        let tab = rect(220.0, 6.0, 100.0, 34.0);

        let visible = scrolled_client_bounds(
            tab,
            Rectangle {
                x: clip.x + 80.0,
                ..clip
            },
            clip,
        )
        .expect("part of the tab is still visible");

        assert_eq!(visible.x, clip.x);
        assert_eq!(visible.width, 40.0);
        assert!(
            scrolled_client_bounds(
                tab,
                Rectangle {
                    x: clip.x + 600.0,
                    ..clip
                },
                clip
            )
            .is_none(),
            "scrolled-away tabs must not remain client zones"
        );
    }
}
