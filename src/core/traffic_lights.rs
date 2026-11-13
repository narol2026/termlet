//! macOS native traffic-light integration.
//!
//! The window uses `fullsize_content_view`, so AppKit's traffic lights overlap
//! the custom top bar, so this module grows the native title bar until the
//! buttons sit inside that bar at the app's own inset.
//!
//! Everything AppKit-specific stays here, behind a deliberately small surface:
//! [`sync`] is the only entry point, [`traffic_lights_reserved`] the only state
//! the view layer reads, [`TRAFFIC_LIGHTS_GUTTER`] the only layout value it
//! needs. Nothing else positions a window button or toggles the gutter by hand —
//! both are derived here from AppKit's notifications and the window's live
//! state, which is what keeps them consistent in windowed and fullscreen mode.
//!
//! # Why this is a state machine instead of "set the frame once"
//!
//! AppKit owns the title bar geometry: `NSThemeFrame` re-lays out
//! `NSTitlebarContainerView` and the three `NSButton`s whenever the window
//! geometry changes, including every frame of the fullscreen animation. Writing
//! our geometry from a notification handler is therefore a race — the write can
//! land *before* AppKit's own pass, which then overwrites it. Four rules make
//! the layout stick:
//!
//! * **Leave the geometry alone while the window is fullscreen or a transition
//!   is in flight.** AppKit owns the title bar there and the buttons slide away
//!   with it; they are hidden for the exit animation and only shown again once
//!   the geometry is final.
//! * **Re-apply once more on a later main-queue turn**, after AppKit has
//!   finished the layout pass the notification announced.
//! * **Never derive the next layout from the current one.** Spacing comes from
//!   constants, so a pass AppKit already reset cannot compound into drift.
//! * **Compare before writing, and guard against re-entrancy.** Writing frames
//!   feeds back into AppKit's layout; without the guard that is a loop.

/// Distance from the window's left edge to the close button.
const WINDOW_CONTROL_PAD_X: f64 = 15.0;

/// Distance between two adjacent buttons (AppKit's own default spacing).
const WINDOW_CONTROL_SPACING: f64 = 20.0;

/// Width of one standard traffic-light button.
const WINDOW_CONTROL_BUTTON_WIDTH: f64 = 14.0;

/// Gap between the last button and the top bar's first control.
const WINDOW_CONTROL_TRAILING_GAP: f64 = 7.0;

/// Vertical padding below the buttons. Together with the button height this
/// defines the height of the title bar we ask AppKit for.
const WINDOW_CONTROL_PAD_Y: f64 = 20.0;

/// Tolerance for frame comparisons, so sub-pixel churn does not re-apply.
const LAYOUT_EPSILON: f64 = 0.5;

/// Upper bound on how long a fullscreen transition may claim the title bar.
/// AppKit's animation is far shorter than this; the bound only exists because a
/// transition AppKit gives up on posts no notification we can observe, and
/// without it the window would stay "transitioning" for the rest of the
/// process.
const FULLSCREEN_TRANSITION_TIMEOUT_MS: u64 = 2_000;

/// Horizontal space the native traffic lights occupy at the window's left edge,
/// including a trailing gap before the top bar's own controls.
///
/// The top bar reserves exactly this much; it is derived from the constants
/// above so the two can not drift apart.
pub const TRAFFIC_LIGHTS_GUTTER: f32 = (WINDOW_CONTROL_PAD_X
    + 2.0 * WINDOW_CONTROL_SPACING
    + WINDOW_CONTROL_BUTTON_WIDTH
    + WINDOW_CONTROL_TRAILING_GAP) as f32;

use std::ffi::c_void;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use std::sync::OnceLock;

use std::time::Instant;

use dispatch2::DispatchQueue;

use objc2::{
    ClassType, DeclaredClass, msg_send_id, mutability,
    rc::{Allocated, Retained},
};

use objc2_app_kit::{
    NSAnimationContext, NSButton, NSView, NSWindow, NSWindowButton, NSWindowStyleMask,
};

use objc2_foundation::{
    MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize,
};

/// Installed once per process; termlet opens one window per process.
static OBSERVER_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Whether the zero-size view that observes effective-appearance changes has
/// been added to the window. Effective-appearance changes are what AppKit
/// triggers when iced/winit switches the native window appearance, so this is
/// the one place that can catch a theme change and re-apply the title bar.
static APPEARANCE_OBSERVER_INSTALLED: AtomicBool = AtomicBool::new(false);

/// Whether the top bar must keep a gutter for the traffic lights. Read by the
/// view layer; driven exclusively by the fullscreen state machine below.
static TRAFFIC_LIGHTS_RESERVED: AtomicBool = AtomicBool::new(true);

/// `apply_layout` mutates AppKit geometry, and that can trigger more layout.
static LAYOUT_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// Coalesces the deferred pass, so a burst of resize notifications only pays
/// for one extra layout.
static DEFERRED_APPLY_PENDING: AtomicBool = AtomicBool::new(false);

/// Deadline (in [`now_ms`] terms) until which AppKit owns the title bar because
/// it is animating a fullscreen transition. Zero means "not transitioning".
static FULLSCREEN_TRANSITION_UNTIL_MS: AtomicU64 = AtomicU64::new(0);

/// The module's single entry point: install the AppKit observer (once per
/// process) and apply the current traffic-light layout for `window`.
///
/// Safe to call as often as iced reports a resize — every pass is idempotent and
/// decides for itself whether the title bar is ours to lay out.
pub fn sync(window: &dyn iced::window::Window) {
    use objc2_foundation::is_main_thread;

    if !is_main_thread() {
        return;
    }

    let Some(ns_window) = ns_window(window) else {
        return;
    };
    disable_native_titlebar_drag(&ns_window);
    install_appearance_observer(&ns_window);
    schedule_layout(ns_window);

    if OBSERVER_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }

    install_observer();
}

fn set_traffic_lights_reserved(reserved: bool) {
    TRAFFIC_LIGHTS_RESERVED.store(reserved, Ordering::SeqCst);
}

/// Whether the top bar currently has to keep [`TRAFFIC_LIGHTS_GUTTER`] clear on
/// the left. Read by the view layer while it builds the top bar.
pub fn traffic_lights_reserved() -> bool {
    TRAFFIC_LIGHTS_RESERVED.load(Ordering::SeqCst)
}

/// Add a zero-size [`TrafficLightAppearanceView`] to the window's content view
/// once per process. Its `viewDidChangeEffectiveAppearance` override runs
/// whenever AppKit changes the window's effective appearance (which is how
/// iced/winit applies a dark/light app theme), letting us re-apply the native
/// traffic-light geometry after AppKit's own title-bar relayout.
fn install_appearance_observer(ns_window: &NSWindow) {
    if APPEARANCE_OBSERVER_INSTALLED.load(Ordering::SeqCst) {
        return;
    }

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(content_view) = ns_window.contentView() else {
        return;
    };

    // SAFETY: `mtm` proves this is the main thread, which is the thread
    // affinity `TrafficLightAppearanceView` declares, and `initWithFrame:` is
    // the initializer its superclass `NSView` provides.
    let probe: Retained<TrafficLightAppearanceView> = unsafe {
        msg_send_id![
            mtm.alloc::<TrafficLightAppearanceView>(),
            initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0))
        ]
    };
    let probe: Retained<NSView> = Retained::into_super(probe);

    // The view is zero-sized and hidden, so it can never affect drawing or hit
    // testing; it only exists to receive the effective-appearance callback.
    probe.setHidden(true);
    // SAFETY: both views are live objects on the main thread, and
    // `content_view` is the window's own content view, which retains what it is
    // given.
    unsafe { content_view.addSubview(&probe) };

    APPEARANCE_OBSERVER_INSTALLED.store(true, Ordering::SeqCst);
}

fn install_observer() {
    use objc2::sel;
    use objc2_app_kit::{
        NSWindowDidEnterFullScreenNotification, NSWindowDidExitFullScreenNotification,
        NSWindowDidResizeNotification, NSWindowWillEnterFullScreenNotification,
        NSWindowWillExitFullScreenNotification,
    };
    use objc2_foundation::NSNotificationCenter;

    let observer = TrafficLightObserver::new();
    // SAFETY: `defaultCenter` hands back the process-wide center, which stays
    // alive for the lifetime of the process. The registrations below run on the
    // main thread, which is where AppKit posts these window notifications.
    let center = unsafe { NSNotificationCenter::defaultCenter() };

    // The `Will…`/`Did…` pair brackets the fullscreen animation. AppKit posts
    // the `Did…` notifications only after the animation has fully completed,
    // which is the correct moment to take the title bar back over. `object` is
    // left as `None` on purpose: one observer serves every window this process
    // opens, and each notification carries its own window.
    // SAFETY: `center` is the live default center from above, `observer` is a
    // live instance whose methods match the selectors passed here (and which is
    // deliberately leaked below, so it outlives the registrations), and every
    // other argument is a `'static` notification name; `None` for `object` is
    // an explicit "all senders" that the API accepts.
    unsafe {
        center.addObserver_selector_name_object(
            &observer,
            sel!(handleWindowResize:),
            Some(NSWindowDidResizeNotification),
            None,
        );
        center.addObserver_selector_name_object(
            &observer,
            sel!(handleWindowWillEnterFullScreen:),
            Some(NSWindowWillEnterFullScreenNotification),
            None,
        );
        center.addObserver_selector_name_object(
            &observer,
            sel!(handleWindowDidEnterFullScreen:),
            Some(NSWindowDidEnterFullScreenNotification),
            None,
        );
        center.addObserver_selector_name_object(
            &observer,
            sel!(handleWindowWillExitFullScreen:),
            Some(NSWindowWillExitFullScreenNotification),
            None,
        );
        center.addObserver_selector_name_object(
            &observer,
            sel!(handleWindowDidExitFullScreen:),
            Some(NSWindowDidExitFullScreenNotification),
            None,
        );
    }

    // Termlet opens each window in a separate process, so leaking one observer
    // for the process lifetime is both bounded and intentional.
    std::mem::forget(observer);
}

fn ns_window(
    window: &dyn iced::window::Window,
) -> Option<objc2::rc::Retained<objc2_app_kit::NSWindow>> {
    use iced::window::raw_window_handle::RawWindowHandle;

    let window_handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(handle) = window_handle.as_raw() else {
        return None;
    };

    // SAFETY: The raw window handle points at the live content view for this
    // window.
    let ns_view_ptr: *mut NSView = handle.ns_view.as_ptr().cast();
    let ns_view = unsafe { Retained::retain(ns_view_ptr) }?;
    ns_view.window()
}

/// Apply the traffic-light layout now, and once more on a later main-queue
/// turn where AppKit's own layout is guaranteed to be finished.
fn schedule_layout(window: Retained<NSWindow>) {
    apply_layout(&window);
    schedule_deferred_layout(window);
}

/// Queue one geometry pass for the next main-queue turn, coalescing bursts.
fn schedule_deferred_layout(window: Retained<NSWindow>) {
    if DEFERRED_APPLY_PENDING.swap(true, Ordering::SeqCst) {
        return;
    }

    let context = Box::into_raw(Box::new(window)).cast::<c_void>();
    // SAFETY: `context` is a boxed window that `apply_deferred` reclaims exactly
    // once, and the main queue outlives the call.
    unsafe { DispatchQueue::main().exec_async_f(context, apply_deferred) };
}

/// Entry point for the deferred pass; owns the box handed to the main queue.
extern "C" fn apply_deferred(context: *mut c_void) {
    // SAFETY: `context` is the `Box<Retained<NSWindow>>` that
    // `schedule_deferred_layout` handed to the main queue. This runs on the
    // main thread, so the window is safe to use.
    let window = unsafe { Box::from_raw(context.cast::<Retained<NSWindow>>()) };

    DEFERRED_APPLY_PENDING.store(false, Ordering::SeqCst);

    if !is_windowed(&window) {
        return;
    }

    // The buttons are hidden for the whole fullscreen-exit animation. Only show
    // them here, in the same pass that puts them where the top bar expects
    // them, so they can never appear at AppKit's default position first.
    set_buttons_hidden(&window, false);
    apply_layout(&window);
}

/// Milliseconds since the first call, used as a monotonic clock for the
/// transition deadline.
fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Hand the title bar over to AppKit for the duration of a transition.
fn begin_fullscreen_transition() {
    FULLSCREEN_TRANSITION_UNTIL_MS.store(
        now_ms() + FULLSCREEN_TRANSITION_TIMEOUT_MS,
        Ordering::SeqCst,
    );
}

/// Take the title bar back once AppKit reports the transition has finished.
fn end_fullscreen_transition() {
    FULLSCREEN_TRANSITION_UNTIL_MS.store(0, Ordering::SeqCst);
}

fn in_fullscreen_transition() -> bool {
    let until = FULLSCREEN_TRANSITION_UNTIL_MS.load(Ordering::SeqCst);
    until != 0 && now_ms() < until
}

/// Whether AppKit's title bar is ours to manage: windowed, and not halfway
/// through a fullscreen transition.
fn is_windowed(window: &NSWindow) -> bool {
    !in_fullscreen_transition() && !window.styleMask().contains(NSWindowStyleMask::FullScreen)
}

/// Stop AppKit from starting a window drag when the mouse goes down on the
/// native title bar.
///
/// On macOS the native title bar is kept visible (hidden title text, but with
/// traffic lights) and, with `fullsize_content_view`, it overlaps termlet's own
/// top bar. AppKit would otherwise treat the whole title bar as movable, so a
/// press on a tab would start both a native window drag and the tab's iced drag
/// at the same time. Turning off the implicit title-bar drag keeps the traffic
/// lights working while leaving the app's explicit `Message::WindowDrag`
/// (`performWindowDragWithEvent:`) as the only way to move the window.
fn disable_native_titlebar_drag(ns_window: &NSWindow) {
    ns_window.setMovable(false);
}

/// Reset the re-entrancy guard on every exit path, including early returns.
struct LayoutGuard;

impl Drop for LayoutGuard {
    fn drop(&mut self) {
        LAYOUT_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

fn apply_layout(ns_window: &NSWindow) {
    if LAYOUT_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        return;
    }
    let _guard = LayoutGuard;

    if !is_windowed(ns_window) {
        return;
    }

    // AppKit can turn the title-bar drag back on as part of its window/fullscreen
    // bookkeeping. Reassert the app-owned window drag policy on every windowed
    // layout pass so fullscreen exits can never leave the native title bar as a
    // second draggable surface underneath the top bar.
    disable_native_titlebar_drag(ns_window);

    // Applying windowed geometry means the native buttons are on screen, so the
    // top bar has to keep the gutter. Deriving the flag here (rather than only
    // from the fullscreen notifications) also repairs the optimistic flip the
    // window handler does before a fullscreen toggle that then fails.
    set_traffic_lights_reserved(true);

    let Some(close) = ns_window.standardWindowButton(NSWindowButton::NSWindowCloseButton) else {
        return;
    };
    let Some(miniaturize) =
        ns_window.standardWindowButton(NSWindowButton::NSWindowMiniaturizeButton)
    else {
        return;
    };
    let Some(zoom) = ns_window.standardWindowButton(NSWindowButton::NSWindowZoomButton) else {
        return;
    };

    let close_view = button_view(close);
    let miniaturize_view = button_view(miniaturize);
    let zoom_view = button_view(zoom);

    // `close -> NSTitlebarView -> NSTitlebarContainerView -> NSThemeFrame`.
    // SAFETY: `superview` only reads the parent pointer. This walks up from a
    // standard window button of a live window on the main thread, and every
    // hop is `None`-checked instead of unwrapped.
    let Some(title_bar_view) = (unsafe { close_view.superview() }) else {
        return;
    };
    // SAFETY: as above — `title_bar_view` is a live view whose superview is
    // read the same way.
    let Some(title_bar_container_view) = (unsafe { title_bar_view.superview() }) else {
        return;
    };
    // SAFETY: as above — `title_bar_container_view` is a live view whose
    // superview is read the same way.
    let Some(theme_frame) = (unsafe { title_bar_container_view.superview() }) else {
        return;
    };

    let button_height = close_view.frame().size.height;
    let title_bar_frame_height = button_height + WINDOW_CONTROL_PAD_Y;

    let mut title_bar_rect = title_bar_container_view.frame();
    title_bar_rect.size.height = title_bar_frame_height;
    // The title bar keeps its top edge glued to the top of the theme frame and
    // grows downwards, into the full-size content view. Using the theme frame's
    // own bounds keeps this in one coordinate space; the window frame is in
    // screen coordinates and only coincides with it in the windowed case.
    title_bar_rect.origin.y = theme_frame.bounds().size.height - title_bar_frame_height;

    // SAFETY: every view here is a live, main-thread-only AppKit object.
    unsafe {
        // AppKit wraps parts of the resize/fullscreen machinery in an implicit
        // animation context; a zero-duration grouping keeps our writes from
        // being interpolated.
        NSAnimationContext::beginGrouping();
        NSAnimationContext::currentContext().setDuration(0.0);

        if !title_bar_settled(title_bar_container_view.frame(), title_bar_rect) {
            title_bar_container_view.setFrame(title_bar_rect);
        }

        for (i, button) in [close_view, miniaturize_view, zoom_view]
            .into_iter()
            .enumerate()
        {
            let mut rect = button.frame();
            let origin_x = WINDOW_CONTROL_PAD_X + i as f64 * WINDOW_CONTROL_SPACING;

            if (rect.origin.x - origin_x).abs() > LAYOUT_EPSILON {
                rect.origin.x = origin_x;
                button.setFrameOrigin(rect.origin);
            }
        }

        NSAnimationContext::endGrouping();
    }
}

/// Compare only the fields this module writes, so unrelated churn never forces
/// a write (and a write never feeds back into another layout pass).
fn title_bar_settled(current: NSRect, desired: NSRect) -> bool {
    (current.origin.y - desired.origin.y).abs() <= LAYOUT_EPSILON
        && (current.size.height - desired.size.height).abs() <= LAYOUT_EPSILON
}

fn set_buttons_hidden(ns_window: &NSWindow, hidden: bool) {
    for button in [
        NSWindowButton::NSWindowCloseButton,
        NSWindowButton::NSWindowMiniaturizeButton,
        NSWindowButton::NSWindowZoomButton,
    ] {
        let Some(button) = ns_window.standardWindowButton(button) else {
            continue;
        };
        button_view(button).setHidden(hidden);
    }
}

fn button_view(button: Retained<NSButton>) -> Retained<NSView> {
    Retained::into_super(Retained::into_super(button))
}

objc2::declare_class!(
    struct TrafficLightAppearanceView;

    // SAFETY: the class name is unique to termlet and the superclass (`NSView`)
    // and mutability (`MainThreadOnly`, matching AppKit's view rules) are the
    // ones that class requires.
    unsafe impl ClassType for TrafficLightAppearanceView {
        type Super = NSView;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "TermletTrafficLightAppearanceView";
    }

    impl DeclaredClass for TrafficLightAppearanceView {
        type Ivars = ();
    }

    // SAFETY: `viewDidChangeEffectiveAppearance` overrides a real `NSView`
    // method, so AppKit can call it with the `&self` receiver it is declared
    // with — on the main thread, which the declared mutability promises.
    unsafe impl TrafficLightAppearanceView {
        #[method(viewDidChangeEffectiveAppearance)]
        fn view_did_change_effective_appearance(&self) {
            let Some(window) = self.as_super().window() else {
                return;
            };

            // AppKit re-lays out the native title bar whenever the window's
            // effective appearance changes, so schedule the same two-pass
            // geometry application used after resize/fullscreen transitions.
            if is_windowed(&window) {
                schedule_layout(window);
            }
        }
    }
);

fn with_notification_window(
    notification: &objc2_foundation::NSNotification,
    f: impl FnOnce(&Retained<NSWindow>),
) {
    // SAFETY: `notification` is a live notification AppKit handed to the
    // observer; its `object` may be absent, which the `let … else` handles.
    let Some(object) = (unsafe { notification.object() }) else {
        return;
    };

    // SAFETY: These notifications are posted by `NSWindow`; the notification
    // object is therefore an NSWindow.
    let ns_window: Retained<NSWindow> = unsafe { Retained::cast(object) };
    f(&ns_window);
}

objc2::declare_class!(
    struct TrafficLightObserver;

    // SAFETY: as for `TrafficLightAppearanceView` — the class name is unique to
    // termlet and `NSObject` is the superclass AppKit requires of a notification
    // observer, while `InteriorMutable` is the mutability these handlers need.
    unsafe impl ClassType for TrafficLightObserver {
        type Super = NSObject;
        type Mutability = mutability::InteriorMutable;
        const NAME: &'static str = "TermletTrafficLightObserver";
    }

    impl DeclaredClass for TrafficLightObserver {}

    // SAFETY: every method below matches a selector AppKit sends to window
    // notification observers and takes the `NSNotification` argument those
    // selectors are declared with.
    unsafe impl TrafficLightObserver {
        #[method_id(init)]
        fn init(this: Allocated<Self>) -> Option<Retained<Self>> {
            // SAFETY: `this` is the freshly allocated instance the runtime
            // hands to `init`, and `set_ivars` installs the (empty) ivar
            // storage before `NSObject`'s `init` runs on it.
            unsafe { msg_send_id![super(this.set_ivars(())), init] }
        }

        #[method(handleWindowResize:)]
        fn handle_window_resize(&self, notification: &NSNotification) {
            with_notification_window(notification, |window| {
                // AppKit re-lays out the title bar on every geometry change.
                // Only the windowed state is ours to manage; while fullscreen
                // (or animating in or out of it) we stay away entirely.
                if is_windowed(window) {
                    schedule_layout(window.clone());
                }
            });
        }

        #[method(handleWindowWillEnterFullScreen:)]
        fn handle_window_will_enter_full_screen(&self, _notification: &NSNotification) {
            begin_fullscreen_transition();
            // The native fullscreen title bar has no traffic lights, so the top
            // bar spreads over the full width from this moment on.
            set_traffic_lights_reserved(false);
        }

        #[method(handleWindowDidEnterFullScreen:)]
        fn handle_window_did_enter_full_screen(&self, _notification: &NSNotification) {
            // Entering is done: the geometry is AppKit's business from here.
            end_fullscreen_transition();
            set_traffic_lights_reserved(false);
        }

        #[method(handleWindowWillExitFullScreen:)]
        fn handle_window_will_exit_full_screen(&self, notification: &NSNotification) {
            begin_fullscreen_transition();
            // Hide them for the whole exit animation; `apply_deferred` shows
            // them again once AppKit's layout has settled.
            with_notification_window(notification, |window| {
                set_buttons_hidden(window, true);
            });
        }

        #[method(handleWindowDidExitFullScreen:)]
        fn handle_window_did_exit_full_screen(&self, notification: &NSNotification) {
            end_fullscreen_transition();
            // The gutter comes back first, so the repaint that makes room for
            // the buttons happens before the buttons become visible.
            set_traffic_lights_reserved(true);
            with_notification_window(notification, |window| {
                if is_windowed(window) {
                    schedule_deferred_layout(window.clone());
                }
            });
        }
    }

    // SAFETY: `TrafficLightObserver` is an `NSObject` subclass, so it satisfies
    // the `NSObjectProtocol` that AppKit requires of notification observers.
    unsafe impl NSObjectProtocol for TrafficLightObserver {}
);

impl TrafficLightObserver {
    fn new() -> Retained<Self> {
        // SAFETY: `alloc` returns an uninitialised instance of this class and
        // the `init` defined above consumes it, returning a retained object.
        unsafe { msg_send_id![Self::alloc(), init] }
    }
}
