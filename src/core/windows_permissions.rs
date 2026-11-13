//! Effective Windows file access for the local SFTP pane.
//!
//! Windows stores file permissions as ACL entries rather than Unix mode bits.
//! The table therefore shows what the current process user can read, write,
//! and execute; it does not pretend that the ACL has Unix owner/group/other
//! columns.

use std::{iter, os::windows::ffi::OsStrExt, path::Path, ptr, sync::OnceLock};

use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, LocalFree},
    Security::{
        ACL,
        Authorization::{
            GetEffectiveRightsFromAclW, GetNamedSecurityInfoW, NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT,
            TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
        },
        DACL_SECURITY_INFORMATION, GetTokenInformation, PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY,
        TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::{FILE_APPEND_DATA, FILE_EXECUTE, FILE_READ_DATA, FILE_WRITE_DATA},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

use super::data::UNKNOWN_PERMISSIONS;

static CURRENT_USER_TOKEN: OnceLock<Option<Vec<usize>>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct AccessRights {
    read: bool,
    write: bool,
    execute: bool,
}

impl AccessRights {
    const FULL: Self = Self {
        read: true,
        write: true,
        execute: true,
    };

    fn from_mask(mask: u32) -> Self {
        Self {
            read: mask & FILE_READ_DATA != 0,
            write: mask & (FILE_WRITE_DATA | FILE_APPEND_DATA) != 0,
            execute: mask & FILE_EXECUTE != 0,
        }
    }
}

/// Format the current user's effective access to `path`.
///
/// `r`, `w`, and `x` are omitted independently when the ACL does not grant
/// them. An unavailable security descriptor is rendered as an em dash so it
/// cannot be confused with a real permission mode of zero.
pub(crate) fn format_permissions(path: &Path, is_dir: bool) -> String {
    effective_access(path).map_or_else(
        || UNKNOWN_PERMISSIONS.to_string(),
        |access| format_access(is_dir, access),
    )
}

fn format_access(is_dir: bool, access: AccessRights) -> String {
    let file_type = if is_dir { 'd' } else { '-' };
    let read = if access.read { 'r' } else { '-' };
    let write = if access.write { 'w' } else { '-' };
    let execute = if access.execute { 'x' } else { '-' };
    format!("{file_type} {read}{write}{execute}")
}

fn effective_access(path: &Path) -> Option<AccessRights> {
    let sid = current_user_sid()?;
    let path: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();

    let mut dacl: *mut ACL = ptr::null_mut();
    let mut security_descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();

    // SAFETY: `path` is NUL-terminated UTF-16, and the two output pointers are
    // valid for the duration of the call. A successful call transfers a
    // LocalAlloc-allocated security descriptor to this function.
    let status = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &raw mut dacl,
            ptr::null_mut(),
            &raw mut security_descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }

    let access = if dacl.is_null() {
        Some(AccessRights::FULL)
    } else {
        let trustee = TRUSTEE_W {
            pMultipleTrustee: ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: sid.cast(),
        };
        let mut mask = 0_u32;

        // SAFETY: `dacl` belongs to the live security descriptor, `trustee`
        // contains a SID owned by the cached token buffer, and `mask` is a
        // valid output pointer.
        let status = unsafe { GetEffectiveRightsFromAclW(dacl, &raw const trustee, &raw mut mask) };
        (status == ERROR_SUCCESS).then(|| AccessRights::from_mask(mask))
    };

    // SAFETY: GetNamedSecurityInfoW returned this pointer through
    // `security_descriptor`; LocalFree releases exactly that allocation once.
    unsafe { LocalFree(security_descriptor.cast()) };

    access
}

fn current_user_sid() -> Option<PSID> {
    let token = CURRENT_USER_TOKEN
        .get_or_init(query_current_user_token)
        .as_ref()?;

    // SAFETY: a successful GetTokenInformation(TokenUser) call returns a
    // TOKEN_USER at the start of this aligned buffer. The buffer owns the SID
    // it points to and remains alive in the process-wide cache.
    let user = unsafe { &*token.as_ptr().cast::<TOKEN_USER>() };
    Some(user.User.Sid)
}

fn query_current_user_token() -> Option<Vec<usize>> {
    let mut token: HANDLE = ptr::null_mut();

    // SAFETY: GetCurrentProcess returns a valid pseudo-handle, and
    // OpenProcessToken writes one token handle on success.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
        return None;
    }

    let mut length = 0_u32;

    // SAFETY: a null buffer with zero length is the documented size-query form.
    // The expected ERROR_INSUFFICIENT_BUFFER result populates `length`.
    unsafe { GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &raw mut length) };
    if length == 0 {
        // SAFETY: `token` was opened by OpenProcessToken above.
        unsafe { CloseHandle(token) };
        return None;
    }

    let words = (length as usize).div_ceil(size_of::<usize>());
    let mut buffer = vec![0_usize; words];

    // SAFETY: the buffer is aligned and at least `length` bytes long. The
    // handle remains valid until it is closed below.
    let success = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &raw mut length,
        )
    } != 0;

    // SAFETY: `token` was opened by OpenProcessToken above and is closed once.
    unsafe { CloseHandle(token) };

    success.then_some(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_summary_marks_each_granted_right() {
        assert_eq!(
            format_access(
                true,
                AccessRights {
                    read: true,
                    write: false,
                    execute: true,
                },
            ),
            "d r-x"
        );
        assert_eq!(format_access(false, AccessRights::FULL), "- rwx");
        assert_eq!(
            format_access(false, AccessRights::default()),
            "- ---",
            "a real empty ACL must remain distinguishable from unknown access"
        );
    }

    #[test]
    fn access_mask_maps_file_rights_to_the_summary() {
        assert_eq!(
            AccessRights::from_mask(FILE_READ_DATA | FILE_APPEND_DATA),
            AccessRights {
                read: true,
                write: true,
                execute: false,
            }
        );
    }

    #[test]
    fn the_current_executable_exposes_effective_access() {
        let executable = std::env::current_exe().expect("test executable path");
        let access = effective_access(&executable)
            .expect("the Windows ACL of the running executable should be readable");
        assert!(
            access.read,
            "the process user should be able to read the image it is running"
        );
    }
}
