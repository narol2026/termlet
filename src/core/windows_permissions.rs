//! Effective Windows file access for the local SFTP pane.
//!
//! Windows stores file permissions as ACL entries rather than Unix mode bits.
//! The table therefore shows what the current process user can read, write,
//! and execute; it does not pretend that the ACL has Unix owner/group/other
//! columns.

// Rust guideline compliant 2026-10-09

use std::{io, iter, os::windows::ffi::OsStrExt, path::Path, ptr, sync::OnceLock};

use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_SUCCESS, HANDLE, LocalFree, WIN32_ERROR},
    Security::{
        ACL,
        Authorization::{
            EXPLICIT_ACCESS_W, GetEffectiveRightsFromAclW, GetNamedSecurityInfoW,
            NO_MULTIPLE_TRUSTEE, SE_FILE_OBJECT, SET_ACCESS, SetEntriesInAclW,
            SetNamedSecurityInfoW, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
        },
        DACL_SECURITY_INFORMATION, GetTokenInformation, NO_INHERITANCE,
        PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
        SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::{
        FILE_ALL_ACCESS, FILE_APPEND_DATA, FILE_EXECUTE, FILE_READ_DATA, FILE_WRITE_DATA,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

#[cfg(test)]
use windows_sys::Win32::Security::{
    CONTAINER_INHERIT_ACE, GetAce, GetSecurityDescriptorControl, OBJECT_INHERIT_ACE,
    SE_DACL_PROTECTED,
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
    let view = DaclView::open(path)?;

    view.dacl().map_or(Some(AccessRights::FULL), |dacl| {
        let trustee = TRUSTEE_W {
            pMultipleTrustee: ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: sid.cast(),
        };
        let mut mask = 0_u32;

        // SAFETY: `dacl` belongs to `view`, `trustee` contains a SID owned by
        // the cached token buffer, and `mask` is a valid output pointer.
        let status = unsafe { GetEffectiveRightsFromAclW(dacl, &raw const trustee, &raw mut mask) };
        (status == ERROR_SUCCESS).then(|| AccessRights::from_mask(mask))
    })
}

/// Restrict `path` to the current user with a protected DACL.
///
/// Directories receive an inheritable entry so newly created app files keep
/// the same boundary. Files keep a non-inheritable entry.
pub(crate) fn restrict_to_current_user(path: &Path, is_dir: bool) -> io::Result<()> {
    let sid = current_user_sid()
        .ok_or_else(|| io::Error::other("failed to determine the current Windows user SID"))?;
    let inheritance = if is_dir {
        SUB_CONTAINERS_AND_OBJECTS_INHERIT
    } else {
        NO_INHERITANCE
    };
    let entry = EXPLICIT_ACCESS_W {
        grfAccessPermissions: FILE_ALL_ACCESS,
        grfAccessMode: SET_ACCESS,
        grfInheritance: inheritance,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: ptr::null_mut(),
            MultipleTrusteeOperation: NO_MULTIPLE_TRUSTEE,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_USER,
            ptstrName: sid.cast(),
        },
    };

    let mut new_acl: *mut ACL = ptr::null_mut();
    // SAFETY: `entry` contains one valid trustee, and `new_acl` is a valid
    // output pointer. No old ACL is supplied because the inherited ACEs should
    // be discarded rather than merged into the protected DACL.
    let status = unsafe { SetEntriesInAclW(1, &raw const entry, ptr::null(), &raw mut new_acl) };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let new_acl = OwnedAcl(new_acl);
    if new_acl.0.is_null() {
        return Err(io::Error::other(
            "Windows returned no DACL after building the access entries",
        ));
    }

    let path = wide_path(path);
    // SAFETY: `path` is NUL-terminated UTF-16, `new_acl` remains allocated for
    // the call, and null owner/group/SACL pointers leave those fields unchanged.
    let status = unsafe {
        SetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            new_acl.0,
            ptr::null(),
        )
    };
    (status == ERROR_SUCCESS)
        .then_some(())
        .ok_or_else(|| win32_error(status))
}

struct DaclView {
    descriptor: PSECURITY_DESCRIPTOR,
    dacl: *mut ACL,
}

impl DaclView {
    fn open(path: &Path) -> Option<Self> {
        let path = wide_path(path);
        let mut dacl = ptr::null_mut();
        let mut descriptor = ptr::null_mut();

        // SAFETY: `path` is NUL-terminated UTF-16, and the output pointers are
        // valid for the duration of the call. A successful call transfers a
        // LocalAlloc-allocated security descriptor to this view.
        let status = unsafe {
            GetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                &raw mut dacl,
                ptr::null_mut(),
                &raw mut descriptor,
            )
        };
        (status == ERROR_SUCCESS).then_some(Self { descriptor, dacl })
    }

    fn dacl(&self) -> Option<&ACL> {
        if self.dacl.is_null() {
            None
        } else {
            // SAFETY: `self.dacl` belongs to the live security descriptor and
            // is freed only when the view is dropped.
            Some(unsafe { &*self.dacl })
        }
    }

    #[cfg(test)]
    fn is_protected(&self) -> bool {
        let mut control = 0_u16;
        let mut revision = 0_u32;

        // SAFETY: `descriptor` is live, and both output pointers are valid.
        let success = unsafe {
            GetSecurityDescriptorControl(self.descriptor, &raw mut control, &raw mut revision)
        };
        success != 0 && control & SE_DACL_PROTECTED != 0
    }

    #[cfg(test)]
    fn first_ace_flags(&self) -> Option<u8> {
        let mut ace = ptr::null_mut();

        // SAFETY: GetAce returns a pointer into the live DACL for index zero,
        // and every ACE begins with an ACE_HEADER.
        let success = unsafe { GetAce(self.dacl, 0, &raw mut ace) };
        if success == 0 || ace.is_null() {
            return None;
        }

        // SAFETY: `ace` points to a live ACE owned by this view, and the ACE
        // header is the common prefix of every ACE layout.
        let header = unsafe { &*ace.cast::<windows_sys::Win32::Security::ACE_HEADER>() };
        Some(header.AceFlags)
    }
}

impl Drop for DaclView {
    fn drop(&mut self) {
        // SAFETY: GetNamedSecurityInfoW transferred this LocalAlloc allocation
        // to the view, and Drop releases it exactly once.
        unsafe { LocalFree(self.descriptor.cast()) };
    }
}

struct OwnedAcl(*mut ACL);

impl Drop for OwnedAcl {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: SetEntriesInAclW allocated this ACL with LocalAlloc, and
            // the wrapper releases it exactly once.
            unsafe { LocalFree(self.0.cast()) };
        }
    }
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect()
}

fn win32_error(status: WIN32_ERROR) -> io::Error {
    io::Error::from_raw_os_error(status as i32)
}

#[cfg(test)]
pub(crate) fn is_current_user_only(path: &Path) -> bool {
    DaclView::open(path).is_some_and(|view| {
        view.is_protected() && view.dacl().is_some_and(|dacl| dacl.AceCount == 1)
    })
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

    #[test]
    fn restricting_a_file_creates_a_protected_single_ace_dacl() {
        let dir =
            std::env::temp_dir().join(format!("termlet_windows_acl_file_{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create ACL test directory");
        let path = dir.join("private.txt");
        std::fs::write(&path, b"secret").expect("create ACL test file");

        restrict_to_current_user(&path, false).expect("restrict the test file");

        let view = DaclView::open(&path).expect("read the restricted DACL");
        let dacl = view.dacl().expect("restricted file has a DACL");
        assert!(view.is_protected(), "DACL must not inherit broader entries");
        assert_eq!(dacl.AceCount, 1, "DACL must grant only the current user");
        assert_eq!(
            view.first_ace_flags().expect("restricted file has one ACE")
                & (CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE) as u8,
            0,
            "file ACE must not grant inheritance"
        );

        std::fs::remove_dir_all(&dir).expect("remove ACL test directory");
    }

    #[test]
    fn restricting_a_directory_adds_an_inherited_current_user_ace() {
        let dir =
            std::env::temp_dir().join(format!("termlet_windows_acl_dir_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create ACL test directory");

        restrict_to_current_user(&dir, true).expect("restrict the test directory");

        let view = DaclView::open(&dir).expect("read the restricted DACL");
        let dacl = view.dacl().expect("restricted directory has a DACL");
        assert!(view.is_protected(), "DACL must not inherit broader entries");
        assert_eq!(dacl.AceCount, 1, "DACL must grant only the current user");
        assert_eq!(
            view.first_ace_flags()
                .expect("restricted directory has one ACE")
                & (CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE) as u8,
            (CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE) as u8,
            "directory ACE must inherit to files and subdirectories"
        );

        let child = dir.join("inherited.txt");
        std::fs::write(&child, b"inherited").expect("create inherited child file");
        let child_view = DaclView::open(&child).expect("read inherited child DACL");
        assert_eq!(
            child_view
                .dacl()
                .expect("inherited child has a DACL")
                .AceCount,
            1,
            "child must inherit only the current-user ACE"
        );

        std::fs::remove_dir_all(&dir).expect("remove ACL test directory");
    }
}
