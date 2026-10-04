use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminatePackageProcessesResult {
    pub matched: u32,
    pub terminated: u32,
    pub failed: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageProcessError {
    InvalidPackageFamily,
    EnumerationFailed,
}

impl std::fmt::Display for PackageProcessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPackageFamily => formatter.write_str("package family is invalid"),
            Self::EnumerationFailed => formatter.write_str("process enumeration failed"),
        }
    }
}

impl std::error::Error for PackageProcessError {}

pub struct PackageProcessManager;

impl PackageProcessManager {
    pub fn matching_process_count(package_family_name: &str) -> Result<u32, PackageProcessError> {
        validate_package_family(package_family_name)?;
        platform::matching_process_ids(package_family_name).map(|processes| processes.len() as u32)
    }

    pub fn terminate(
        package_family_name: &str,
    ) -> Result<TerminatePackageProcessesResult, PackageProcessError> {
        validate_package_family(package_family_name)?;
        platform::terminate(package_family_name)
    }
}

fn validate_package_family(value: &str) -> Result<(), PackageProcessError> {
    if !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        Ok(())
    } else {
        Err(PackageProcessError::InvalidPackageFamily)
    }
}

#[cfg(windows)]
mod platform {
    use std::mem::size_of;

    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::{
                CloseHandle, APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS,
                HANDLE, WAIT_OBJECT_0,
            },
            Storage::Packaging::Appx::GetPackageFamilyName,
            System::{
                Diagnostics::ToolHelp::{
                    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                    TH32CS_SNAPPROCESS,
                },
                Threading::{
                    OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_ACCESS_RIGHTS,
                    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
                },
            },
        },
    };

    use super::{PackageProcessError, TerminatePackageProcessesResult};

    const SYNCHRONIZE_PROCESS: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(0x0010_0000);
    const TERMINATION_WAIT_MS: u32 = 5_000;

    struct OwnedHandle(HANDLE);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub(super) fn matching_process_ids(
        package_family_name: &str,
    ) -> Result<Vec<u32>, PackageProcessError> {
        let snapshot = OwnedHandle(
            unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
                .map_err(|_| PackageProcessError::EnumerationFailed)?,
        );
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if unsafe { Process32FirstW(snapshot.0, &mut entry) }.is_err() {
            return Err(PackageProcessError::EnumerationFailed);
        }
        let current = std::process::id();
        let mut matches = Vec::new();
        loop {
            let process_id = entry.th32ProcessID;
            if process_id != 0 && process_id != current {
                if let Ok(handle) =
                    unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }
                {
                    let handle = OwnedHandle(handle);
                    if package_family_name_for_process(handle.0)
                        .ok()
                        .flatten()
                        .is_some_and(|candidate| {
                            candidate.eq_ignore_ascii_case(package_family_name)
                        })
                    {
                        matches.push(process_id);
                    }
                }
            }
            if unsafe { Process32NextW(snapshot.0, &mut entry) }.is_err() {
                break;
            }
        }
        Ok(matches)
    }

    pub(super) fn terminate(
        package_family_name: &str,
    ) -> Result<TerminatePackageProcessesResult, PackageProcessError> {
        let process_ids = matching_process_ids(package_family_name)?;
        let mut result = TerminatePackageProcessesResult {
            matched: process_ids.len() as u32,
            terminated: 0,
            failed: 0,
        };
        for process_id in process_ids {
            let access =
                PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE_PROCESS;
            let Ok(handle) = (unsafe { OpenProcess(access, false, process_id) }) else {
                result.failed += 1;
                continue;
            };
            let handle = OwnedHandle(handle);
            let still_matches = package_family_name_for_process(handle.0)
                .ok()
                .flatten()
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(package_family_name));
            if !still_matches {
                result.failed += 1;
                continue;
            }
            let terminated = unsafe { TerminateProcess(handle.0, 1) }.is_ok()
                && unsafe { WaitForSingleObject(handle.0, TERMINATION_WAIT_MS) } == WAIT_OBJECT_0;
            if terminated {
                result.terminated += 1;
            } else {
                result.failed += 1;
            }
        }
        Ok(result)
    }

    fn package_family_name_for_process(
        process: HANDLE,
    ) -> Result<Option<String>, PackageProcessError> {
        let mut length = 0_u32;
        let status = unsafe { GetPackageFamilyName(process, &mut length, None) };
        if status == APPMODEL_ERROR_NO_PACKAGE {
            return Ok(None);
        }
        if status != ERROR_INSUFFICIENT_BUFFER || length == 0 {
            return Err(PackageProcessError::EnumerationFailed);
        }
        let mut buffer = vec![0_u16; length as usize];
        let status =
            unsafe { GetPackageFamilyName(process, &mut length, Some(PWSTR(buffer.as_mut_ptr()))) };
        if status != ERROR_SUCCESS {
            return Err(PackageProcessError::EnumerationFailed);
        }
        let end = buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(buffer.len());
        String::from_utf16(&buffer[..end])
            .map(Some)
            .map_err(|_| PackageProcessError::EnumerationFailed)
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{PackageProcessError, TerminatePackageProcessesResult};

    pub(super) fn matching_process_ids(
        _package_family_name: &str,
    ) -> Result<Vec<u32>, PackageProcessError> {
        Err(PackageProcessError::EnumerationFailed)
    }

    pub(super) fn terminate(
        _package_family_name: &str,
    ) -> Result<TerminatePackageProcessesResult, PackageProcessError> {
        Err(PackageProcessError::EnumerationFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::{PackageProcessError, PackageProcessManager};

    #[test]
    fn rejects_untrusted_package_family_names_before_process_enumeration() {
        assert_eq!(
            PackageProcessManager::matching_process_count("Family\\Other"),
            Err(PackageProcessError::InvalidPackageFamily)
        );
    }
}
