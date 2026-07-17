use std::{ffi::OsStr, os::windows::ffi::OsStrExt, path::Path};

use thiserror::Error;
use windows::{
    Win32::{
        Foundation::ERROR_FILE_NOT_FOUND,
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
            REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW,
            RegSetValueExW,
        },
    },
    core::PCWSTR,
};

use crate::AppEnvironment;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WindowsLaunchAtLoginError {
    #[error("the current executable path cannot be used for launch at login")]
    InvalidExecutable,
    #[error("Windows could not update launch at login: {0}")]
    System(String),
}

#[derive(Debug, Clone)]
pub struct WindowsLaunchAtLogin {
    value_name: String,
    command: String,
}

impl WindowsLaunchAtLogin {
    pub fn for_current_executable(
        environment: AppEnvironment,
    ) -> Result<Self, WindowsLaunchAtLoginError> {
        let executable = std::env::current_exe()
            .map_err(|error| WindowsLaunchAtLoginError::System(error.to_string()))?;
        Self::for_executable(environment, &executable)
    }

    pub fn for_executable(
        environment: AppEnvironment,
        executable: &Path,
    ) -> Result<Self, WindowsLaunchAtLoginError> {
        let executable = executable
            .to_str()
            .filter(|value| !value.contains('"'))
            .ok_or(WindowsLaunchAtLoginError::InvalidExecutable)?;
        let (value_name, environment_name) = match environment {
            AppEnvironment::Production => ("Saymore", "production"),
            AppEnvironment::Development => ("Saymore Dev", "development"),
        };
        Ok(Self {
            value_name: value_name.to_owned(),
            command: format!("\"{executable}\" --autostart --environment {environment_name}"),
        })
    }

    pub fn is_enabled(&self) -> Result<bool, WindowsLaunchAtLoginError> {
        let key = open_run_key()?;
        let result = read_string(&key, &self.value_name)
            .map(|value| value.as_deref() == Some(self.command.as_str()));
        drop(key);
        result
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<(), WindowsLaunchAtLoginError> {
        let key = open_run_key()?;
        let result = if enabled {
            set_string(&key, &self.value_name, &self.command)
        } else {
            delete_value(&key, &self.value_name)
        };
        drop(key);
        result
    }
}

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        // SAFETY: this handle is returned by RegCreateKeyExW and closed exactly once here.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

fn open_run_key() -> Result<RegistryKey, WindowsLaunchAtLoginError> {
    let subkey = wide(RUN_KEY);
    let mut key = HKEY::default();
    // SAFETY: subkey is NUL-terminated and key points to writable output storage.
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    status.ok().map(|()| RegistryKey(key)).map_err(system_error)
}

fn read_string(
    key: &RegistryKey,
    value_name: &str,
) -> Result<Option<String>, WindowsLaunchAtLoginError> {
    let value_name = wide(value_name);
    let mut byte_count = 0_u32;
    // SAFETY: value_name is NUL-terminated; this first call only requests the byte count.
    let status = unsafe {
        RegGetValueW(
            key.0,
            PCWSTR::null(),
            PCWSTR(value_name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut byte_count),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    status.ok().map_err(system_error)?;
    let mut buffer = vec![0_u16; byte_count.div_ceil(2) as usize];
    // SAFETY: buffer has the size returned by RegGetValueW and byte_count is writable.
    unsafe {
        RegGetValueW(
            key.0,
            PCWSTR::null(),
            PCWSTR(value_name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut byte_count),
        )
    }
    .ok()
    .map_err(system_error)?;
    let end = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    Ok(Some(String::from_utf16_lossy(&buffer[..end])))
}

fn set_string(
    key: &RegistryKey,
    value_name: &str,
    value: &str,
) -> Result<(), WindowsLaunchAtLoginError> {
    let value_name = wide(value_name);
    let value = wide(value);
    let bytes = unsafe {
        std::slice::from_raw_parts(value.as_ptr().cast::<u8>(), value.len() * size_of::<u16>())
    };
    // SAFETY: value name and REG_SZ data are NUL-terminated and valid for this call.
    unsafe {
        RegSetValueExW(
            key.0,
            PCWSTR(value_name.as_ptr()),
            None,
            REG_SZ,
            Some(bytes),
        )
    }
    .ok()
    .map_err(system_error)
}

fn delete_value(key: &RegistryKey, value_name: &str) -> Result<(), WindowsLaunchAtLoginError> {
    let value_name = wide(value_name);
    // SAFETY: value_name is a NUL-terminated UTF-16 string.
    let status = unsafe { RegDeleteValueW(key.0, PCWSTR(value_name.as_ptr())) };
    if status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        status.ok().map_err(system_error)
    }
}

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

fn system_error(error: windows::core::Error) -> WindowsLaunchAtLoginError {
    WindowsLaunchAtLoginError::System(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_quotes_paths_and_keeps_environments_distinct() {
        let path = Path::new(r"C:\Program Files\Saymore\saymore-desktop.exe");
        let Ok(production) = WindowsLaunchAtLogin::for_executable(AppEnvironment::Production, path)
        else {
            panic!("a normal Windows executable path should be accepted");
        };
        let Ok(development) =
            WindowsLaunchAtLogin::for_executable(AppEnvironment::Development, path)
        else {
            panic!("a normal Windows executable path should be accepted");
        };

        assert_eq!("Saymore", production.value_name);
        assert_eq!("Saymore Dev", development.value_name);
        assert_eq!(
            r#""C:\Program Files\Saymore\saymore-desktop.exe" --autostart --environment production"#,
            production.command
        );
        assert_ne!(production.command, development.command);
    }

    struct TestRunValue(WindowsLaunchAtLogin);

    impl Drop for TestRunValue {
        fn drop(&mut self) {
            let _ = self.0.set_enabled(false);
        }
    }

    #[test]
    fn registry_enable_and_disable_are_idempotent() {
        let integration = TestRunValue(WindowsLaunchAtLogin {
            value_name: format!(
                "Saymore Test {} {}",
                std::process::id(),
                uuid::Uuid::new_v4()
            ),
            command:
                r#""C:\Saymore Test\saymore-desktop.exe" --autostart --environment development"#
                    .to_owned(),
        });

        assert_eq!(Ok(false), integration.0.is_enabled());
        assert_eq!(Ok(()), integration.0.set_enabled(false));
        assert_eq!(Ok(()), integration.0.set_enabled(true));
        assert_eq!(Ok(true), integration.0.is_enabled());
        assert_eq!(Ok(()), integration.0.set_enabled(true));
        assert_eq!(Ok(true), integration.0.is_enabled());
        assert_eq!(Ok(()), integration.0.set_enabled(false));
        assert_eq!(Ok(false), integration.0.is_enabled());
    }
}
