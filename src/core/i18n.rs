//! i18n helpers for the rust-i18n crate.
//!
//! The `t!` macro from `rust_i18n` v4 returns `Cow<'static, str>`.  For
//! non-interpolated key lookups this is always `Cow::Borrowed(&'static str)`.
//! [`t_static`] extracts the `&'static str` so label methods can keep their
//! existing return type.

use std::borrow::Cow;

/// Extract a `&'static str` from a non-interpolated `t!()` lookup.
///
/// # Panics
///
/// Panics if the [`Cow`] is `Owned`.  Only call this helper with simple key
/// lookups — never with interpolation (`t!("key", var = val)`) or dynamic
/// message arguments.
// The `i18n!` macro generates the call sites, so this has to keep the owned
// `Cow` the macro hands over rather than borrowing it.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the `i18n!` macro generates every call site and passes an owned `Cow`"
)]
pub fn t_static(key: Cow<'static, str>) -> &'static str {
    match key {
        Cow::Borrowed(s) => s,
        Cow::Owned(_) => {
            panic!("t_static called on an owned Cow — use t!().as_ref() for interpolated strings")
        }
    }
}
