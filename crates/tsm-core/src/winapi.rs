//! Thin, audited wrappers around the Win32 APIs we need. Every `unsafe`
//! block in the crate lives here.
#![allow(clippy::missing_safety_doc)]

use crate::error::{CoreError, CoreResult};
use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, LocalFree, FILETIME, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, TerminateProcess,
    WaitForSingleObject, INFINITE, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
fn wide_path(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

// ------------------------------------------------------------------ DPAPI

/// Stop this process's stdin/stdout/stderr from being inherited by servers we
/// start. Otherwise a shell that pipes the CLI's output (`tsm-cli … | more`)
/// waits until those servers exit. Rust's `Command` duplicates the handles it
/// really wants a child to inherit, so this is safe.
pub fn std_handles_not_inheritable() {
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        unsafe {
            let h = GetStdHandle(which);
            if !h.is_null() && h != INVALID_HANDLE_VALUE {
                SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

pub fn dpapi_protect(data: &[u8]) -> CoreResult<Vec<u8>> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
        let desc = wide("TOP Server Manager secret");
        let ok = CryptProtectData(&input, desc.as_ptr(), null(), null(), null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output);
        if ok == 0 {
            return Err(CoreError::Other(format!("CryptProtectData failed ({})", GetLastError())));
        }
        let v = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as *mut c_void);
        Ok(v)
    }
}

pub fn dpapi_unprotect(data: &[u8]) -> CoreResult<Vec<u8>> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut output = CRYPT_INTEGER_BLOB { cbData: 0, pbData: null_mut() };
        let ok = CryptUnprotectData(&input, null_mut(), null(), null(), null(), CRYPTPROTECT_UI_FORBIDDEN, &mut output);
        if ok == 0 {
            return Err(CoreError::Other(format!("CryptUnprotectData failed ({})", GetLastError())));
        }
        let v = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData as *mut c_void);
        Ok(v)
    }
}

// ------------------------------------------------------------------ handles

/// Owned Win32 handle closed on drop.
pub struct OwnedHandle(pub HANDLE);
unsafe impl Send for OwnedHandle {}
unsafe impl Sync for OwnedHandle {}
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

// ------------------------------------------------------------------ job objects

const JOB_RIGHTS: u32 = 0x0001 /*ASSIGN*/ | 0x0004 /*QUERY*/ | 0x0008 /*TERMINATE*/ | 0x0010 /*SET_ATTRIBUTES*/;

/// A named Windows Job Object. All processes of one server unit live in one
/// job, so stopping kills exactly that process tree – never other programs
/// that happen to share an executable name.
pub struct Job {
    handle: OwnedHandle,
    pub name: String,
}

impl Job {
    pub fn create(name: &str, kill_on_close: bool, max_memory_mb: Option<u64>, cpu_percent: Option<u8>) -> CoreResult<Job> {
        let wname = wide(name);
        unsafe {
            let h = CreateJobObjectW(null(), wname.as_ptr());
            if h.is_null() {
                return Err(CoreError::Other(format!("CreateJobObject failed ({})", GetLastError())));
            }
            let job = Job { handle: OwnedHandle(h), name: name.to_string() };
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            let mut flags = 0u32;
            if kill_on_close {
                flags |= JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            }
            if let Some(mb) = max_memory_mb {
                flags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
                info.JobMemoryLimit = (mb as usize) * 1024 * 1024;
            }
            info.BasicLimitInformation.LimitFlags = flags;
            if SetInformationJobObject(
                h,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                return Err(CoreError::Other(format!("SetInformationJobObject failed ({})", GetLastError())));
            }
            if let Some(pct) = cpu_percent {
                let pct = pct.clamp(1, 100) as u32;
                let mut cpu: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION = std::mem::zeroed();
                cpu.ControlFlags = JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
                cpu.Anonymous.CpuRate = pct * 100; // units of 1/100 percent
                SetInformationJobObject(
                    h,
                    JobObjectCpuRateControlInformation,
                    &cpu as *const _ as *const c_void,
                    std::mem::size_of::<JOBOBJECT_CPU_RATE_CONTROL_INFORMATION>() as u32,
                );
            }
            Ok(job)
        }
    }

    /// Re-open a job created by a previous run of the app.
    pub fn open(name: &str) -> Option<Job> {
        let wname = wide(name);
        unsafe {
            let h = OpenJobObjectW(JOB_RIGHTS, 0, wname.as_ptr());
            if h.is_null() { None } else { Some(Job { handle: OwnedHandle(h), name: name.to_string() }) }
        }
    }

    pub fn assign_pid(&self, pid: u32) -> CoreResult<()> {
        unsafe {
            let p = OpenProcess(0x0100 /*SET_QUOTA*/ | PROCESS_TERMINATE, 0, pid);
            if p.is_null() {
                return Err(CoreError::Other(format!("OpenProcess({pid}) failed ({})", GetLastError())));
            }
            let p = OwnedHandle(p);
            if AssignProcessToJobObject(self.handle.0, p.0) == 0 {
                return Err(CoreError::Other(format!("AssignProcessToJobObject failed ({})", GetLastError())));
            }
        }
        Ok(())
    }

    pub fn terminate(&self, exit_code: u32) -> bool {
        unsafe { TerminateJobObject(self.handle.0, exit_code) != 0 }
    }

    /// PIDs currently inside the job.
    pub fn pids(&self) -> Vec<u32> {
        const MAX: usize = 512;
        #[repr(C)]
        struct Buf {
            assigned: u32,
            in_list: u32,
            ids: [usize; MAX],
        }
        unsafe {
            let mut buf: Buf = std::mem::zeroed();
            let ok = QueryInformationJobObject(
                self.handle.0,
                JobObjectBasicProcessIdList,
                &mut buf as *mut _ as *mut c_void,
                std::mem::size_of::<Buf>() as u32,
                null_mut(),
            );
            if ok == 0 {
                return vec![];
            }
            buf.ids[..(buf.in_list as usize).min(MAX)].iter().map(|p| *p as u32).collect()
        }
    }
}

// ------------------------------------------------------------------ processes

/// Identity of a running process: (creation time as FILETIME u64, image path).
/// Used to prove that a PID recorded earlier still belongs to *our* process
/// (PIDs are recycled by Windows).
pub fn process_identity(pid: u32) -> Option<(u64, PathBuf)> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let h = OwnedHandle(h);
        let mut c: FILETIME = std::mem::zeroed();
        let mut e: FILETIME = std::mem::zeroed();
        let mut k: FILETIME = std::mem::zeroed();
        let mut u: FILETIME = std::mem::zeroed();
        if GetProcessTimes(h.0, &mut c, &mut e, &mut k, &mut u) == 0 {
            return None;
        }
        let mut code = 0u32;
        if GetExitCodeProcess(h.0, &mut code) != 0 && code != 259 {
            return None; // exited (STILL_ACTIVE = 259)
        }
        let created = ((c.dwHighDateTime as u64) << 32) | c.dwLowDateTime as u64;
        let mut buf = vec![0u16; 32768];
        let mut size = buf.len() as u32;
        if QueryFullProcessImageNameW(h.0, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut size) == 0 {
            return Some((created, PathBuf::new()));
        }
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        Some((created, PathBuf::from(path)))
    }
}

/// Block until the process exits; returns its exit code. Call from
/// `spawn_blocking`.
pub fn wait_pid(pid: u32) -> Option<u32> {
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let h = OwnedHandle(h);
        if WaitForSingleObject(h.0, INFINITE) != WAIT_OBJECT_0 {
            return None;
        }
        let mut code = 0u32;
        GetExitCodeProcess(h.0, &mut code);
        Some(code)
    }
}

pub fn terminate_pid(pid: u32, code: u32) -> bool {
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if h.is_null() {
            return false;
        }
        let h = OwnedHandle(h);
        TerminateProcess(h.0, code) != 0
    }
}

// ------------------------------------------------------------------ Authenticode

/// Verify the Authenticode signature of a file (chain to a trusted root).
pub fn verify_authenticode(path: &Path) -> bool {
    use windows_sys::Win32::Security::WinTrust::*;
    let wpath = wide_path(path);
    unsafe {
        let mut file_info: WINTRUST_FILE_INFO = std::mem::zeroed();
        file_info.cbStruct = std::mem::size_of::<WINTRUST_FILE_INFO>() as u32;
        file_info.pcwszFilePath = wpath.as_ptr();
        let mut data: WINTRUST_DATA = std::mem::zeroed();
        data.cbStruct = std::mem::size_of::<WINTRUST_DATA>() as u32;
        data.dwUIChoice = WTD_UI_NONE;
        data.fdwRevocationChecks = WTD_REVOKE_NONE;
        data.dwUnionChoice = WTD_CHOICE_FILE;
        data.Anonymous.pFile = &mut file_info;
        data.dwStateAction = WTD_STATEACTION_VERIFY;
        let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
        let status = WinVerifyTrust(null_mut(), &mut action, &mut data as *mut _ as *mut c_void);
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        WinVerifyTrust(null_mut(), &mut action, &mut data as *mut _ as *mut c_void);
        status == 0
    }
}

// ------------------------------------------------------------------ elevation

/// Run a program elevated (UAC prompt shown to the user) and wait for it.
/// Used only for user-approved firewall rule changes.
pub fn run_elevated(program: &str, params: &str) -> CoreResult<u32> {
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    let verb = wide("runas");
    let file = wide(program);
    let p = wide(params);
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = p.as_ptr();
        info.nShow = 0; // SW_HIDE
        if ShellExecuteExW(&mut info) == 0 {
            let err = GetLastError();
            if err == 1223 {
                return Err(CoreError::PermissionDenied("the administrator prompt was cancelled".into()));
            }
            return Err(CoreError::Other(format!("ShellExecuteEx failed ({err})")));
        }
        if info.hProcess.is_null() {
            return Ok(0);
        }
        let h = OwnedHandle(info.hProcess);
        WaitForSingleObject(h.0, 60_000);
        let mut code = 0u32;
        GetExitCodeProcess(h.0, &mut code);
        Ok(code)
    }
}
