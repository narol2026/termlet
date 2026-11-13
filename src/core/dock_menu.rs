//! macOS Dock menu integration.
//!
//! AppKit builds the Dock's right-click menu from the application delegate's
//! `applicationDockMenu:` method. winit (and therefore iced) installs its own
//! `WinitApplicationDelegate` and verifies it during event dispatch, so the
//! delegate cannot be replaced: the `applicationDockMenu:` method and its
//! `newWindow:` action are added to that already-registered class with
//! `class_addMethod` instead. The single "New Window" item launches a fresh
//! `--new-window` process — an empty window that opts out of the primary
//! window's workspace recovery.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::{
    rc::Retained,
    runtime::{AnyObject, Sel},
    sel,
};
use objc2_app_kit::{NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSString, is_main_thread, ns_string};

use rust_i18n::t;

use crate::core::i18n::t_static;

/// Set once both Dock-menu methods have been added to winit's delegate class.
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Install the Dock menu. Safe to call repeatedly; the first successful call
/// on the main thread wins and every later call is a no-op.
pub fn install() {
    if !is_main_thread() || INSTALLED.load(Ordering::SeqCst) {
        return;
    }

    // winit registers this class during `EventLoop::build()`, before iced
    // reaches `App::new` (where `install` is invoked).
    // SAFETY: the name is a `'static`, NUL-terminated C string and
    // `objc_getClass` only reads it; it returns null for an unregistered name,
    // which is checked on the next line.
    let cls = unsafe { objc2::ffi::objc_getClass(c"WinitApplicationDelegate".as_ptr()) };
    if cls.is_null() {
        return;
    }

    // `applicationDockMenu:` provides the menu; `newWindow:` is its item
    // action and targets the delegate instance.
    // SAFETY: `cls` is the non-null class just returned, `application_dock_menu`
    // has the `(id, SEL, id) -> id` shape that `@@:@` describes, and both the
    // selector and the type encoding are `'static` C strings.
    let added = unsafe {
        objc2::ffi::class_addMethod(
            cls.cast_mut(),
            sel!(applicationDockMenu:).as_ptr(),
            application_dock_menu_imp(),
            c"@@:@".as_ptr(),
        )
    };
    // `BOOL` is `bool` on aarch64 Apple targets but `i8` on x86_64 macOS, so
    // compare against `NO` instead of negating the raw value.
    if added == objc2::ffi::NO {
        return;
    }

    // SAFETY: as above — `new_window` matches the `v@:@` encoding (a void
    // method taking the receiver, the selector, and one object), and the
    // selector and encoding are `'static` C strings.
    unsafe {
        objc2::ffi::class_addMethod(
            cls.cast_mut(),
            sel!(newWindow:).as_ptr(),
            new_window_imp(),
            c"v@:@".as_ptr(),
        );
    }

    INSTALLED.store(true, Ordering::SeqCst);
}

/// `- (NSMenu *)applicationDockMenu:(NSApplication *)sender`.
///
/// # Safety
///
/// Only valid as an Objective-C method implementation: `this` must be the
/// method's receiver (the winit application delegate) and the signature must
/// keep matching the `@@:@` encoding registered in [`install`]. It is called
/// by the Objective-C runtime through [`application_dock_menu_imp`], never
/// directly from Rust.
unsafe extern "C" fn application_dock_menu(
    this: *mut c_void,
    _cmd: Sel,
    _sender: *mut c_void,
) -> *mut c_void {
    match build_dock_menu(this) {
        Some(menu) => Retained::autorelease_return(menu).cast(),
        None => std::ptr::null_mut(),
    }
}

/// `- (void)newWindow:(id)sender`.
///
/// # Safety
///
/// Only valid as an Objective-C method implementation: it is called by the
/// runtime through [`new_window_imp`] when the Dock's "New Window" item is
/// picked, so its signature must keep matching the `v@:@` encoding.
unsafe extern "C" fn new_window(_this: *mut c_void, _cmd: Sel, _sender: *mut c_void) {
    if let Err(error) = crate::core::process::spawn_window(["--new-window"]) {
        crate::core::diagnostics::log(
            crate::core::diagnostics::Level::Warn,
            "DOCK",
            format!("Failed to open new window: {error}"),
        );
    }
}

fn application_dock_menu_imp() -> objc2::ffi::IMP {
    // SAFETY: `IMP` is an untyped code pointer, so erasing the concrete
    // signature is sound as long as the pointer is never called through the
    // erased type — and it is not: the runtime calls it with the arguments
    // described by the method's `@@:@` encoding.
    Some(unsafe {
        std::mem::transmute::<
            unsafe extern "C" fn(*mut c_void, Sel, *mut c_void) -> *mut c_void,
            unsafe extern "C" fn(),
        >(application_dock_menu)
    })
}

fn new_window_imp() -> objc2::ffi::IMP {
    // SAFETY: as above — the erased signature is never used to call the
    // pointer; the runtime invokes `new_window` per its `v@:@` encoding.
    Some(unsafe {
        std::mem::transmute::<
            unsafe extern "C" fn(*mut c_void, Sel, *mut c_void),
            unsafe extern "C" fn(),
        >(new_window)
    })
}

/// Build the Dock menu fresh on every request so the title follows the current
/// locale. The Dock appends its own items (Hide, Quit, …) after ours.
fn build_dock_menu(delegate: *mut c_void) -> Option<Retained<NSMenu>> {
    let mtm = MainThreadMarker::new()?;
    let menu = NSMenu::new(mtm);

    let item = NSMenuItem::new(mtm);
    let title = NSString::from_str(t_static(t!("dock.new_window")));

    // The delegate is the method receiver and lives for the process lifetime,
    // so it is a safe, already-retained menu-item target.
    // SAFETY: `delegate` is the receiver AppKit passed to
    // `applicationDockMenu:` — the live, already-retained winit delegate — so
    // it is a non-null object that outlives this borrow.
    let target: &AnyObject = unsafe { &*(delegate as *const AnyObject) };
    // SAFETY: this runs on the main thread (`MainThreadMarker::new()` above
    // re-checked it, and the Dock only calls `applicationDockMenu:` there), and
    // `item`, `title`, and `target` are live objects for the duration of the
    // calls.
    unsafe {
        item.setTitle(&title);
        item.setAction(Some(sel!(newWindow:)));
        item.setTarget(Some(target));
        item.setKeyEquivalent(ns_string!(""));
    }
    menu.addItem(&item);

    Some(menu)
}
