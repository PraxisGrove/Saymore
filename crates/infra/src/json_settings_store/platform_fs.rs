use std::{fs::File, path::Path};

use template_app::SettingsStoreError;

use super::io_error;

#[cfg(unix)]
pub(super) fn restrict_directory_permissions(path: &Path) -> Result<(), SettingsStoreError> {
    use std::{fs, os::unix::fs::PermissionsExt};

    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(io_error)
}

#[cfg(windows)]
pub(super) fn restrict_directory_permissions(path: &Path) -> Result<(), SettingsStoreError> {
    set_private_acl(path, true)
}

#[cfg(not(any(unix, windows)))]
pub(super) fn restrict_directory_permissions(_path: &Path) -> Result<(), SettingsStoreError> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn open_private_file(path: &Path) -> Result<File, SettingsStoreError> {
    use std::{fs::OpenOptions, os::unix::fs::OpenOptionsExt};

    OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(io_error)
}

#[cfg(windows)]
pub(super) fn open_private_file(path: &Path) -> Result<File, SettingsStoreError> {
    use std::fs::OpenOptions;

    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(io_error)?;
    if let Err(error) = set_private_acl(path, false) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(error);
    }
    Ok(file)
}

#[cfg(not(any(unix, windows)))]
pub(super) fn open_private_file(path: &Path) -> Result<File, SettingsStoreError> {
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(io_error)
}

#[cfg(windows)]
pub(super) fn atomic_replace(source: &Path, destination: &Path) -> Result<(), SettingsStoreError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        },
        core::PCWSTR,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both slices are stable, NUL-terminated UTF-16 paths for the duration of the call.
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| SettingsStoreError::Unavailable(error.to_string()))
}

#[cfg(not(windows))]
pub(super) fn atomic_replace(source: &Path, destination: &Path) -> Result<(), SettingsStoreError> {
    std::fs::rename(source, destination).map_err(io_error)
}

#[cfg(windows)]
fn set_private_acl(path: &Path, directory: bool) -> Result<(), SettingsStoreError> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows::{
        Win32::{
            Foundation::{HLOCAL, LocalFree},
            Security::{
                ACL,
                Authorization::{
                    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
                    SE_FILE_OBJECT, SetNamedSecurityInfoW,
                },
                DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl,
                PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
            },
        },
        core::{BOOL, PCWSTR, PWSTR},
    };

    let descriptor = if directory {
        "D:P(A;;FA;;;OW)(A;OICIIO;FA;;;CO)(A;OICI;FA;;;SY)"
    } else {
        "D:P(A;;FA;;;OW)(A;;FA;;;SY)"
    };
    let descriptor: Vec<u16> = descriptor.encode_utf16().chain(Some(0)).collect();
    let mut security_descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the input is a valid, NUL-terminated SDDL string and output is writable.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(descriptor.as_ptr()),
            SDDL_REVISION_1,
            &mut security_descriptor,
            None,
        )
    }
    .map_err(|error| SettingsStoreError::Unavailable(error.to_string()))?;

    let result = (|| {
        let mut present = BOOL::default();
        let mut defaulted = BOOL::default();
        let mut dacl: *mut ACL = ptr::null_mut();
        // SAFETY: security_descriptor is allocated and remains live for this call.
        unsafe {
            GetSecurityDescriptorDacl(security_descriptor, &mut present, &mut dacl, &mut defaulted)
        }
        .map_err(|error| SettingsStoreError::Unavailable(error.to_string()))?;
        if !present.as_bool() || dacl.is_null() {
            return Err(SettingsStoreError::Unavailable(
                "private Windows ACL did not contain a DACL".to_owned(),
            ));
        }
        let mut path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: path is writable NUL-terminated UTF-16 and dacl points into the descriptor.
        let status = unsafe {
            SetNamedSecurityInfoW(
                PWSTR(path.as_mut_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(dacl),
                None,
            )
        };
        status
            .ok()
            .map_err(|error| SettingsStoreError::Unavailable(error.to_string()))
    })();

    // SAFETY: the descriptor was allocated by LocalAlloc inside the conversion API.
    unsafe {
        let _ = LocalFree(Some(HLOCAL(security_descriptor.0)));
    }
    result
}

#[cfg(all(test, windows))]
mod tests {
    use std::os::windows::ffi::OsStrExt;

    use windows::{
        Win32::{
            Foundation::{HLOCAL, LocalFree},
            Security::{
                Authorization::{
                    ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
                    SDDL_REVISION_1, SE_FILE_OBJECT,
                },
                DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
            },
        },
        core::{PCWSTR, PWSTR},
    };

    use super::*;

    #[test]
    fn private_acl_excludes_broad_windows_principals() {
        let Ok(directory) = tempfile::tempdir() else {
            panic!("temporary directory should be available");
        };
        let path = directory.path().join("config.json");
        let Ok(file) = open_private_file(&path) else {
            panic!("private settings file should be creatable");
        };
        drop(file);
        let Ok(sddl) = dacl_sddl(&path) else {
            panic!("private settings DACL should be readable");
        };

        assert!(sddl.starts_with("D:P"));
        assert!(!sddl.contains(";;;WD)"));
        assert!(!sddl.contains(";;;BU)"));
        assert!(!sddl.contains(";;;AU)"));
    }

    fn dacl_sddl(path: &Path) -> Result<String, SettingsStoreError> {
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: path is NUL-terminated and descriptor is writable output storage.
        unsafe {
            GetNamedSecurityInfoW(
                PCWSTR(path.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                None,
                None,
                &mut descriptor,
            )
        }
        .ok()
        .map_err(|error| SettingsStoreError::Unavailable(error.to_string()))?;
        let mut text = PWSTR::null();
        // SAFETY: descriptor is live and text is writable output storage.
        let converted = unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                None,
            )
        };
        let result = converted
            .map_err(|error| SettingsStoreError::Unavailable(error.to_string()))
            .and_then(|()| unsafe {
                text.to_string()
                    .map_err(|error| SettingsStoreError::Unavailable(error.to_string()))
            });
        // SAFETY: both buffers are allocated by LocalAlloc and each is freed once.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
            let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        }
        result
    }
}
