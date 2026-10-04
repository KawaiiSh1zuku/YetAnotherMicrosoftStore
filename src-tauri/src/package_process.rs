use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProcessDescriptor {
    pub pid: u32,
    pub name: String,
}

impl ProcessDescriptor {
    pub fn is_safe(&self) -> bool {
        self.pid != 0
            && !self.name.is_empty()
            && self.name.len() <= 520
            && !self.name.chars().any(char::is_control)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TerminatePackageProcessesResult {
    pub matched: Vec<ProcessDescriptor>,
    pub terminated: Vec<ProcessDescriptor>,
    pub remaining: Vec<ProcessDescriptor>,
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
    pub fn matching_processes(
        package_family_name: &str,
    ) -> Result<Vec<ProcessDescriptor>, PackageProcessError> {
        validate_package_family(package_family_name)?;
        platform::matching_processes(package_family_name)
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

    use super::{PackageProcessError, ProcessDescriptor, TerminatePackageProcessesResult};

    const SYNCHRONIZE_PROCESS: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(0x0010_0000);
    const TERMINATION_WAIT_MS: u32 = 5_000;
    const MAX_MATCHED_PROCESSES: usize = 128;

    struct OwnedHandle(HANDLE);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    pub(super) fn matching_processes(
        package_family_name: &str,
    ) -> Result<Vec<ProcessDescriptor>, PackageProcessError> {
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
                        matches.push(ProcessDescriptor {
                            pid: process_id,
                            name: process_name(&entry),
                        });
                        if matches.len() >= MAX_MATCHED_PROCESSES {
                            break;
                        }
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
        let matched = matching_processes(package_family_name)?;
        let mut result = TerminatePackageProcessesResult {
            matched: matched.clone(),
            terminated: Vec::new(),
            remaining: Vec::new(),
        };
        for descriptor in matched {
            let access =
                PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE_PROCESS;
            let Ok(handle) = (unsafe { OpenProcess(access, false, descriptor.pid) }) else {
                result.remaining.push(descriptor);
                continue;
            };
            let handle = OwnedHandle(handle);
            let still_matches = package_family_name_for_process(handle.0)
                .ok()
                .flatten()
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(package_family_name));
            if !still_matches {
                result.remaining.push(descriptor);
                continue;
            }
            let terminated = unsafe { TerminateProcess(handle.0, 1) }.is_ok()
                && unsafe { WaitForSingleObject(handle.0, TERMINATION_WAIT_MS) } == WAIT_OBJECT_0;
            if terminated {
                result.terminated.push(descriptor);
            } else {
                result.remaining.push(descriptor);
            }
        }
        Ok(result)
    }

    fn process_name(entry: &PROCESSENTRY32W) -> String {
        let end = entry
            .szExeFile
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
        let sanitized = name
            .chars()
            .filter(|character| !character.is_control())
            .take(260)
            .collect::<String>();
        if sanitized.is_empty() {
            "unknown".to_owned()
        } else {
            sanitized
        }
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
    use super::{PackageProcessError, ProcessDescriptor, TerminatePackageProcessesResult};

    pub(super) fn matching_processes(
        _package_family_name: &str,
    ) -> Result<Vec<ProcessDescriptor>, PackageProcessError> {
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
            PackageProcessManager::matching_processes("Family\\Other"),
            Err(PackageProcessError::InvalidPackageFamily)
        );
    }
}
