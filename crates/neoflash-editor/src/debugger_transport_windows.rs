//! Private DevTools pipe to a Chromium browser owned by this Test Movie session on Windows (`T-496C`).
//!
//! No TCP debugger endpoint or user's browser profile is opened. Chromium reads
//! NUL-delimited protocol messages on CRT file descriptor 3 and writes them on CRT
//! file descriptor 4 (`--remote-debugging-pipe`), passed strictly via `STARTUPINFOEXW`
//! (`lpReserved2` + `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`) inside a `KILL_ON_JOB_CLOSE`
//! Win32 Job Object.
#![cfg_attr(not(windows), allow(dead_code))]

use serde_json::Value;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

const MAX_MESSAGE: usize = 32 * 1024 * 1024;
const CRT_FOPEN: u8 = 0x01;
const CRT_FPIPE: u8 = 0x08;

#[cfg_attr(not(windows), allow(unused_imports))]
pub(crate) use crate::platform_open::file_url;

/// Resolve the Chromium-family executable (`chrome.exe` or `msedge.exe`) for `Test Movie` debugging.
pub(crate) fn browser_executable() -> Result<PathBuf, String> {
    let debug_override = std::env::var_os("NEOFLASH_DEBUG_BROWSER");
    let pf = std::env::var_os("PROGRAMFILES")
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from(r"C:\Program Files")));
    let pf_x86 = std::env::var_os("PROGRAMFILES(X86)")
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from(r"C:\Program Files (x86)")));
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    browser_executable_from_env(
        debug_override.as_deref(),
        pf.as_deref(),
        pf_x86.as_deref(),
        local.as_deref(),
    )
}

fn browser_executable_from_env(
    debug_browser: Option<&OsStr>,
    program_files: Option<&Path>,
    program_files_x86: Option<&Path>,
    local_app_data: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(path) = debug_browser {
        let path = PathBuf::from(path);
        return path
            .is_file()
            .then_some(path)
            .ok_or_else(|| "NEOFLASH_DEBUG_BROWSER must name a Chromium executable.".into());
    }
    let roots: [Option<&Path>; 3] = [program_files, program_files_x86, local_app_data];
    let relative_candidates: [&[&str]; 3] = [
        &["Google", "Chrome", "Application", "chrome.exe"],
        &["Chromium", "Application", "chrome.exe"],
        &["Microsoft", "Edge", "Application", "msedge.exe"],
    ];
    for rel in &relative_candidates {
        for root in roots.into_iter().flatten() {
            let mut candidate = root.to_path_buf();
            for seg in *rel {
                candidate.push(seg);
            }
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(
        "Debugging Test Movie requires Chrome, Chromium, or Edge. Set NEOFLASH_DEBUG_BROWSER to its executable."
            .into(),
    )
}

/// Build the exact MSVC UCRT `STARTUPINFOW::lpReserved2` byte buffer mapping CRT fds `0..=4`:
/// `[0, 1, 2] -> NUL` (`FOPEN`) and `[3 -> child_read_fd3, 4 -> child_write_fd4]` (`FOPEN | FPIPE`).
fn build_crt_stdio_buffer_with_ptr_size(
    nul_handle: u64,
    child_read_fd3: u64,
    child_write_fd4: u64,
    ptr_size: usize,
) -> Vec<u8> {
    let fd_count: u32 = 5;
    let flags: [u8; 5] = [
        CRT_FOPEN,
        CRT_FOPEN,
        CRT_FOPEN,
        CRT_FOPEN | CRT_FPIPE,
        CRT_FOPEN | CRT_FPIPE,
    ];
    let handles: [u64; 5] = [
        nul_handle,
        nul_handle,
        nul_handle,
        child_read_fd3,
        child_write_fd4,
    ];
    let mut buf = Vec::with_capacity(4 + flags.len() + handles.len() * ptr_size);
    buf.extend_from_slice(&fd_count.to_le_bytes());
    buf.extend_from_slice(&flags);
    for h in handles {
        if ptr_size == 4 {
            buf.extend_from_slice(&(h as u32).to_le_bytes());
        } else {
            buf.extend_from_slice(&h.to_le_bytes());
        }
    }
    buf
}

/// Quote a single Windows command-line argument following `CommandLineToArgvW` escaping rules.
fn quote_windows_arg(arg: &str) -> String {
    if !arg.is_empty()
        && !arg
            .chars()
            .any(|c| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '"'))
    {
        return arg.to_owned();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                for _ in 0..(backslashes * 2 + 1) {
                    out.push('\\');
                }
                out.push('"');
                backslashes = 0;
            }
            other => {
                for _ in 0..backslashes {
                    out.push('\\');
                }
                backslashes = 0;
                out.push(other);
            }
        }
    }
    for _ in 0..(backslashes * 2) {
        out.push('\\');
    }
    out.push('"');
    out
}

fn build_chromium_command_line(executable: &Path, profile: &Path, headless: bool) -> String {
    let mut parts = vec![
        quote_windows_arg(&executable.to_string_lossy()),
        "--remote-debugging-pipe".to_owned(),
        quote_windows_arg(&format!("--user-data-dir={}", profile.display())),
        "--no-first-run".to_owned(),
        "--no-default-browser-check".to_owned(),
        "--disable-background-networking".to_owned(),
    ];
    if headless {
        parts.push("--headless=new".to_owned());
    }
    parts.push("about:blank".to_owned());
    parts.join(" ")
}

fn encode_nul_delimited_command(message: &Value) -> io::Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(message)?;
    if bytes.len() > MAX_MESSAGE {
        return Err(io::Error::other(
            "Debugger command exceeds the message limit",
        ));
    }
    bytes.push(0);
    Ok(bytes)
}

fn try_pop_nul_delimited_frame(pending: &mut Vec<u8>) -> io::Result<Option<Value>> {
    let Some(end) = pending.iter().position(|b| *b == 0) else {
        return Ok(None);
    };
    let result = serde_json::from_slice(&pending[..end])?;
    pending.drain(..=end);
    Ok(Some(result))
}

fn append_received_chunk(pending: &mut Vec<u8>, chunk: &[u8]) -> io::Result<()> {
    if pending.len().saturating_add(chunk.len()) > MAX_MESSAGE {
        return Err(io::Error::other(
            "Debugger response exceeds the message limit",
        ));
    }
    pending.extend_from_slice(chunk);
    Ok(())
}

#[cfg(windows)]
mod win32 {
    use super::*;
    use std::ffi::c_void;
    use std::fs;
    use std::os::windows::ffi::OsStrExt as _;
    use std::ptr::{null, null_mut};
    use std::time::{Duration, Instant};

    type Handle = *mut c_void;
    const INVALID_HANDLE_VALUE: Handle = (-1_isize) as Handle;

    const HANDLE_FLAG_INHERIT: u32 = 0x0000_0001;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const OPEN_EXISTING: u32 = 3;
    const STARTF_USESTDHANDLES: u32 = 0x0000_0100;
    const CREATE_SUSPENDED: u32 = 0x0000_0004;
    const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x0008_0000;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const PROC_THREAD_ATTRIBUTE_HANDLE_LIST: usize = 0x0002_0002;
    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
    const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: i32 = 9;
    const WAIT_OBJECT_0: u32 = 0;
    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_BROKEN_PIPE: i32 = 109;
    const ERROR_PIPE_NOT_CONNECTED: i32 = 233;
    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;

    #[repr(C)]
    struct SecurityAttributes {
        n_length: u32,
        lp_security_descriptor: *mut c_void,
        b_inherit_handle: i32,
    }

    #[repr(C)]
    struct StartupInfoW {
        cb: u32,
        lp_reserved: *mut u16,
        lp_desktop: *mut u16,
        lp_title: *mut u16,
        dw_x: u32,
        dw_y: u32,
        dw_x_size: u32,
        dw_y_size: u32,
        dw_x_count_chars: u32,
        dw_y_count_chars: u32,
        dw_fill_attribute: u32,
        dw_flags: u32,
        w_show_window: u16,
        cb_reserved2: u16,
        lp_reserved2: *mut u8,
        h_std_input: Handle,
        h_std_output: Handle,
        h_std_error: Handle,
    }

    #[repr(C)]
    struct StartupInfoExW {
        startup_info: StartupInfoW,
        lp_attribute_list: *mut c_void,
    }

    #[repr(C)]
    struct ProcessInformation {
        h_process: Handle,
        h_thread: Handle,
        dw_process_id: u32,
        dw_thread_id: u32,
    }

    #[repr(C)]
    struct JobObjectBasicLimitInformation {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: u32,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    struct IoCounters {
        read_operation_count: u64,
        write_operation_count: u64,
        other_operation_count: u64,
        read_transfer_count: u64,
        write_transfer_count: u64,
        other_transfer_count: u64,
    }

    #[repr(C)]
    struct JobObjectExtendedLimitInformation {
        basic_limit_information: JobObjectBasicLimitInformation,
        io_info: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    #[cfg(target_pointer_width = "64")]
    const _: () = {
        assert!(std::mem::size_of::<StartupInfoW>() == 104);
        assert!(std::mem::size_of::<StartupInfoExW>() == 112);
        assert!(std::mem::size_of::<JobObjectExtendedLimitInformation>() == 144);
    };

    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(
            hAlgorithm: Handle,
            pbBuffer: *mut u8,
            cbBuffer: u32,
            dwFlags: u32,
        ) -> i32;
    }

    unsafe extern "system" {
        fn CloseHandle(hObject: Handle) -> i32;
        fn CreatePipe(
            hReadPipe: *mut Handle,
            hWritePipe: *mut Handle,
            lpPipeAttributes: *const SecurityAttributes,
            nSize: u32,
        ) -> i32;
        fn SetHandleInformation(hObject: Handle, dwMask: u32, dwFlags: u32) -> i32;
        fn CreateFileW(
            lpFileName: *const u16,
            dwDesiredAccess: u32,
            dwShareMode: u32,
            lpSecurityAttributes: *const SecurityAttributes,
            dwCreationDisposition: u32,
            dwFlagsAndAttributes: u32,
            hTemplateFile: Handle,
        ) -> Handle;
        fn InitializeProcThreadAttributeList(
            lpAttributeList: *mut c_void,
            dwAttributeCount: u32,
            dwFlags: u32,
            lpSize: *mut usize,
        ) -> i32;
        fn UpdateProcThreadAttribute(
            lpAttributeList: *mut c_void,
            dwFlags: u32,
            Attribute: usize,
            lpValue: *const c_void,
            cbSize: usize,
            lpPreviousValue: *mut c_void,
            lpReturnSize: *const usize,
        ) -> i32;
        fn DeleteProcThreadAttributeList(lpAttributeList: *mut c_void);
        fn CreateJobObjectW(
            lpJobAttributes: *const SecurityAttributes,
            lpName: *const u16,
        ) -> Handle;
        fn SetInformationJobObject(
            hJob: Handle,
            JobObjectInformationClass: i32,
            lpJobObjectInformation: *const c_void,
            cbJobObjectInformationLength: u32,
        ) -> i32;
        fn AssignProcessToJobObject(hJob: Handle, hProcess: Handle) -> i32;
        fn TerminateJobObject(hJob: Handle, uExitCode: u32) -> i32;
        fn CreateProcessW(
            lpApplicationName: *const u16,
            lpCommandLine: *mut u16,
            lpProcessAttributes: *const SecurityAttributes,
            lpThreadAttributes: *const SecurityAttributes,
            bInheritHandles: i32,
            dwCreationFlags: u32,
            lpEnvironment: *const c_void,
            lpCurrentDirectory: *const u16,
            lpStartupInfo: *const StartupInfoExW,
            lpProcessInformation: *mut ProcessInformation,
        ) -> i32;
        fn ResumeThread(hThread: Handle) -> u32;
        fn TerminateProcess(hProcess: Handle, uExitCode: u32) -> i32;
        fn WaitForSingleObject(hHandle: Handle, dwMilliseconds: u32) -> u32;
        fn PeekNamedPipe(
            hNamedPipe: Handle,
            lpBuffer: *mut c_void,
            nBufferSize: u32,
            lpBytesRead: *mut u32,
            lpTotalBytesAvail: *mut u32,
            lpBytesLeftThisMessage: *mut u32,
        ) -> i32;
        fn ReadFile(
            hFile: Handle,
            lpBuffer: *mut u8,
            nNumberOfBytesToRead: u32,
            lpNumberOfBytesRead: *mut u32,
            lpOverlapped: *mut c_void,
        ) -> i32;
        fn WriteFile(
            hFile: Handle,
            lpBuffer: *const u8,
            nNumberOfBytesToWrite: u32,
            lpNumberOfBytesWritten: *mut u32,
            lpOverlapped: *mut c_void,
        ) -> i32;
    }

    struct WinHandle(Handle);

    // SAFETY: Win32 kernel handles (`Pipe`, `Process`, `JobObject`, `File`) are process-wide
    // handle table entries safe to move across threads (`neoflash-debugger` worker).
    unsafe impl Send for WinHandle {}

    impl WinHandle {
        fn raw(&self) -> Handle {
            self.0
        }

        fn close_now(&mut self) {
            if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
                unsafe {
                    let _ = CloseHandle(self.0);
                }
                self.0 = null_mut();
            }
        }
    }

    impl Drop for WinHandle {
        fn drop(&mut self) {
            self.close_now();
        }
    }

    struct ProcThreadAttrList {
        storage: Vec<u64>,
        initialized: bool,
    }

    impl ProcThreadAttrList {
        fn with_handle_list(handles: &[Handle]) -> io::Result<Self> {
            let mut size: usize = 0;
            unsafe {
                let _ = InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size);
            }
            if size == 0 {
                return Err(io::Error::last_os_error());
            }
            let words = size.div_ceil(8);
            let mut storage = vec![0_u64; words];
            let ptr = storage.as_mut_ptr().cast::<c_void>();
            if unsafe { InitializeProcThreadAttributeList(ptr, 1, 0, &mut size) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let list = Self {
                storage,
                initialized: true,
            };
            let ok = unsafe {
                UpdateProcThreadAttribute(
                    list.as_mut_ptr(),
                    0,
                    PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                    handles.as_ptr().cast::<c_void>(),
                    std::mem::size_of_val(handles),
                    null_mut(),
                    null(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(list)
        }

        fn as_mut_ptr(&self) -> *mut c_void {
            self.storage.as_ptr() as *mut c_void
        }
    }

    impl Drop for ProcThreadAttrList {
        fn drop(&mut self) {
            if self.initialized {
                unsafe {
                    DeleteProcThreadAttributeList(self.as_mut_ptr());
                }
            }
        }
    }

    pub(crate) fn csprng_private_directory() -> io::Result<PathBuf> {
        for _ in 0..16 {
            let mut random = [0_u8; 16];
            let status = unsafe {
                BCryptGenRandom(
                    null_mut(),
                    random.as_mut_ptr(),
                    random.len() as u32,
                    BCRYPT_USE_SYSTEM_PREFERRED_RNG,
                )
            };
            if status != 0 {
                return Err(io::Error::other(format!(
                    "BCryptGenRandom failed with NTSTATUS 0x{status:08x}"
                )));
            }
            let suffix: String = random.iter().map(|v| format!("{v:02x}")).collect();
            let path = std::env::temp_dir()
                .join(format!("neoflash-debug-{}-{suffix}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(path),
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(err),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not reserve a private debugger directory",
        ))
    }

    fn create_kill_on_close_job() -> io::Result<WinHandle> {
        let raw = unsafe { CreateJobObjectW(null(), null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = WinHandle(raw);
        let mut info: JobObjectExtendedLimitInformation = unsafe { std::mem::zeroed() };
        info.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = unsafe {
            SetInformationJobObject(
                job.raw(),
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                (&raw const info).cast::<c_void>(),
                std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    fn create_pipe_pair() -> io::Result<(WinHandle, WinHandle)> {
        let inheritable = SecurityAttributes {
            n_length: std::mem::size_of::<SecurityAttributes>() as u32,
            lp_security_descriptor: null_mut(),
            b_inherit_handle: 1,
        };
        let mut read_h: Handle = null_mut();
        let mut write_h: Handle = null_mut();
        if unsafe { CreatePipe(&mut read_h, &mut write_h, &inheritable, 64 * 1024) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((WinHandle(read_h), WinHandle(write_h)))
    }

    fn disable_inheritance(handle: &WinHandle) -> io::Result<()> {
        if unsafe { SetHandleInformation(handle.raw(), HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn open_inheritable_nul() -> io::Result<WinHandle> {
        let inheritable = SecurityAttributes {
            n_length: std::mem::size_of::<SecurityAttributes>() as u32,
            lp_security_descriptor: null_mut(),
            b_inherit_handle: 1,
        };
        const NUL_WIDE: [u16; 4] = [b'N' as u16, b'U' as u16, b'L' as u16, 0];
        let raw = unsafe {
            CreateFileW(
                NUL_WIDE.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                &inheritable,
                OPEN_EXISTING,
                0,
                null_mut(),
            )
        };
        if raw.is_null() || raw == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(WinHandle(raw))
    }

    pub(crate) struct BrowserPipe {
        job: WinHandle,
        process: WinHandle,
        #[cfg(test)]
        child_pid: u32,
        write_pipe: WinHandle,
        read_pipe: WinHandle,
        pending: Vec<u8>,
        profile: PathBuf,
    }

    impl BrowserPipe {
        pub(crate) fn launch(executable: &Path, headless: bool) -> io::Result<Self> {
            let profile = csprng_private_directory()?;
            let result = Self::launch_in(executable, headless, profile.clone());
            if result.is_err() {
                let _ = fs::remove_dir_all(&profile);
            }
            result
        }

        fn launch_in(executable: &Path, headless: bool, profile: PathBuf) -> io::Result<Self> {
            let job = create_kill_on_close_job()?;

            // fd 3 in child: Chromium reads CDP commands from parent_write -> child_read_fd3.
            let (child_read_fd3, parent_write) = create_pipe_pair()?;
            disable_inheritance(&parent_write)?;

            // fd 4 in child: Chromium writes CDP events/responses to child_write_fd4 -> parent_read.
            let (parent_read, child_write_fd4) = create_pipe_pair()?;
            disable_inheritance(&parent_read)?;

            // fds 0, 1, 2 in child: bound to NUL so stdio writes never corrupt fd 3/fd 4.
            let nul_handle = open_inheritable_nul()?;

            let mut crt_stdio = build_crt_stdio_buffer_with_ptr_size(
                nul_handle.raw() as usize as u64,
                child_read_fd3.raw() as usize as u64,
                child_write_fd4.raw() as usize as u64,
                std::mem::size_of::<usize>(),
            );

            // Restrict inherited handles strictly to [nul, child_read_fd3, child_write_fd4]
            // (unique handles only, as required by PROC_THREAD_ATTRIBUTE_HANDLE_LIST).
            let inherited_handles = [
                nul_handle.raw(),
                child_read_fd3.raw(),
                child_write_fd4.raw(),
            ];
            let attr_list = ProcThreadAttrList::with_handle_list(&inherited_handles)?;

            let exe_norm = crate::windows_platform::normalize_windows_path(executable);
            let exe_wide: Vec<u16> = exe_norm
                .as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let cmdline_str = build_chromium_command_line(executable, &profile, headless);
            let mut cmdline_base: Vec<u16> = cmdline_str
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();

            let mut si_ex: StartupInfoExW = unsafe { std::mem::zeroed() };
            si_ex.startup_info.cb = std::mem::size_of::<StartupInfoExW>() as u32;
            si_ex.startup_info.dw_flags = STARTF_USESTDHANDLES;
            si_ex.startup_info.h_std_input = nul_handle.raw();
            si_ex.startup_info.h_std_output = nul_handle.raw();
            si_ex.startup_info.h_std_error = nul_handle.raw();
            si_ex.startup_info.cb_reserved2 = crt_stdio.len() as u16;
            si_ex.startup_info.lp_reserved2 = crt_stdio.as_mut_ptr();
            si_ex.lp_attribute_list = attr_list.as_mut_ptr();

            let base_flags = EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW | CREATE_SUSPENDED;
            let mut proc_info: ProcessInformation = unsafe { std::mem::zeroed() };

            // Try with CREATE_BREAKAWAY_FROM_JOB first, falling back cleanly if the parent job
            // disallows breakaway (Windows 10/11 supports nested Job Objects natively).
            let mut cmd_try = cmdline_base.clone();
            let mut spawned = unsafe {
                CreateProcessW(
                    exe_wide.as_ptr(),
                    cmd_try.as_mut_ptr(),
                    null(),
                    null(),
                    1,
                    base_flags | CREATE_BREAKAWAY_FROM_JOB,
                    null(),
                    null(),
                    &si_ex,
                    &mut proc_info,
                )
            };
            if spawned == 0 {
                let first_err = io::Error::last_os_error();
                if first_err.raw_os_error() == Some(ERROR_ACCESS_DENIED) {
                    spawned = unsafe {
                        CreateProcessW(
                            exe_wide.as_ptr(),
                            cmdline_base.as_mut_ptr(),
                            null(),
                            null(),
                            1,
                            base_flags,
                            null(),
                            null(),
                            &si_ex,
                            &mut proc_info,
                        )
                    };
                    if spawned == 0 {
                        return Err(io::Error::last_os_error());
                    }
                } else {
                    return Err(first_err);
                }
            }

            let process = WinHandle(proc_info.h_process);
            let thread = WinHandle(proc_info.h_thread);
            #[cfg(test)]
            let child_pid = proc_info.dw_process_id;

            // Assign child to our KILL_ON_JOB_CLOSE Job Object while suspended so zero helper
            // processes can escape before job membership takes effect.
            if unsafe { AssignProcessToJobObject(job.raw(), process.raw()) } == 0 {
                let err = io::Error::last_os_error();
                unsafe {
                    let _ = TerminateProcess(process.raw(), 1);
                }
                return Err(err);
            }
            if unsafe { ResumeThread(thread.raw()) } == u32::MAX {
                let err = io::Error::last_os_error();
                unsafe {
                    let _ = TerminateJobObject(job.raw(), 1);
                }
                return Err(err);
            }

            // Explicitly release child-side handles in the parent so broken-pipe EOF triggers
            // immediately when the child and its helpers close their ends.
            drop(thread);
            drop(attr_list);
            drop(nul_handle);
            drop(child_read_fd3);
            drop(child_write_fd4);

            Ok(Self {
                job,
                process,
                #[cfg(test)]
                child_pid,
                write_pipe: parent_write,
                read_pipe: parent_read,
                pending: Vec::new(),
                profile,
            })
        }

        pub(crate) fn send(&mut self, message: &Value) -> io::Result<()> {
            let bytes = encode_nul_delimited_command(message)?;
            let mut offset = 0usize;
            while offset < bytes.len() {
                let chunk_len = (bytes.len() - offset).min(64 * 1024) as u32;
                let mut written: u32 = 0;
                let ok = unsafe {
                    WriteFile(
                        self.write_pipe.raw(),
                        bytes[offset..].as_ptr(),
                        chunk_len,
                        &mut written,
                        null_mut(),
                    )
                };
                if ok == 0 {
                    let err = io::Error::last_os_error();
                    if matches!(
                        err.raw_os_error(),
                        Some(ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED)
                    ) {
                        return Err(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "Test Movie debugger disconnected",
                        ));
                    }
                    return Err(err);
                }
                if written == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "Test Movie debugger pipe wrote zero bytes",
                    ));
                }
                offset += written as usize;
            }
            Ok(())
        }

        pub(crate) fn receive(&mut self) -> io::Result<Option<Value>> {
            let poll_deadline = Instant::now() + Duration::from_millis(40);
            loop {
                if let Some(frame) = try_pop_nul_delimited_frame(&mut self.pending)? {
                    return Ok(Some(frame));
                }

                let mut avail: u32 = 0;
                let peek_ok = unsafe {
                    PeekNamedPipe(
                        self.read_pipe.raw(),
                        null_mut(),
                        0,
                        null_mut(),
                        &mut avail,
                        null_mut(),
                    )
                };
                if peek_ok == 0 {
                    let err = io::Error::last_os_error();
                    if matches!(
                        err.raw_os_error(),
                        Some(ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED)
                    ) {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "Test Movie debugger disconnected",
                        ));
                    }
                    return Err(err);
                }

                if avail == 0 {
                    if self.leader_exited() {
                        return Err(io::Error::other("Test Movie browser exited"));
                    }
                    if Instant::now() >= poll_deadline {
                        return Ok(None);
                    }
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }

                let mut buf = [0_u8; 8192];
                let to_read = avail.min(buf.len() as u32);
                let mut read_count: u32 = 0;
                let read_ok = unsafe {
                    ReadFile(
                        self.read_pipe.raw(),
                        buf.as_mut_ptr(),
                        to_read,
                        &mut read_count,
                        null_mut(),
                    )
                };
                if read_ok == 0 {
                    let err = io::Error::last_os_error();
                    if matches!(
                        err.raw_os_error(),
                        Some(ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED)
                    ) {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "Test Movie debugger disconnected",
                        ));
                    }
                    return Err(err);
                }
                if read_count == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Test Movie debugger disconnected",
                    ));
                }
                append_received_chunk(&mut self.pending, &buf[..read_count as usize])?;
            }
        }

        pub(crate) fn leader_exited(&self) -> bool {
            unsafe { WaitForSingleObject(self.process.raw(), 0) == WAIT_OBJECT_0 }
        }

        #[cfg(test)]
        pub(crate) fn child_pid(&self) -> u32 {
            self.child_pid
        }

        #[cfg(test)]
        pub(crate) fn profile(&self) -> &Path {
            &self.profile
        }
    }

    impl Drop for BrowserPipe {
        fn drop(&mut self) {
            self.write_pipe.close_now();
            self.read_pipe.close_now();
            if !self.job.raw().is_null() {
                unsafe {
                    let _ = TerminateJobObject(self.job.raw(), 1);
                }
                self.job.close_now();
            }
            if !self.process.raw().is_null() {
                unsafe {
                    let _ = TerminateProcess(self.process.raw(), 1);
                    let _ = WaitForSingleObject(self.process.raw(), 2_000);
                }
                self.process.close_now();
            }
            for attempt in 0..5 {
                if !self.profile.exists() || fs::remove_dir_all(&self.profile).is_ok() {
                    break;
                }
                if attempt < 4 {
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
    }
}

#[cfg(windows)]
pub(crate) use win32::BrowserPipe;

#[cfg(windows)]
pub(crate) fn private_directory() -> io::Result<PathBuf> {
    win32::csprng_private_directory()
}

#[cfg(not(windows))]
pub(crate) struct BrowserPipe;

#[cfg(not(windows))]
pub(crate) fn private_directory() -> io::Result<PathBuf> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows debugger transport requires target_os = \"windows\"",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn crt_lpreserved2_buffer_matches_ucrt_fd3_fd4_pipe_layout() {
        let nul = 0x0000_0000_0000_0104_u64;
        let fd3 = 0x0000_0000_0000_0208_u64;
        let fd4 = 0x0000_0000_0000_030C_u64;
        let buf = build_crt_stdio_buffer_with_ptr_size(nul, fd3, fd4, 8);
        assert_eq!(buf.len(), 4 + 5 + 5 * 8);
        assert_eq!(u32::from_le_bytes(buf[0..4].try_into().unwrap()), 5);
        assert_eq!(
            &buf[4..9],
            &[
                CRT_FOPEN,
                CRT_FOPEN,
                CRT_FOPEN,
                CRT_FOPEN | CRT_FPIPE,
                CRT_FOPEN | CRT_FPIPE,
            ]
        );
        let decoded_handles: Vec<u64> = buf[9..]
            .as_chunks::<8>()
            .0
            .iter()
            .copied()
            .map(u64::from_le_bytes)
            .collect();
        assert_eq!(decoded_handles, vec![nul, nul, nul, fd3, fd4]);
    }

    #[test]
    fn windows_command_line_quoting_handles_spaces_quotes_and_trailing_backslashes() {
        assert_eq!(quote_windows_arg("--headless=new"), "--headless=new");
        assert_eq!(
            quote_windows_arg(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
            r#""C:\Program Files\Google\Chrome\Application\chrome.exe""#
        );
        assert_eq!(
            quote_windows_arg(r#"--user-data-dir=C:\Temp\My Dir\"#),
            r#""--user-data-dir=C:\Temp\My Dir\\""#
        );
        assert_eq!(quote_windows_arg(r#"a\"b"#), r#""a\\\"b""#);

        let cmd = build_chromium_command_line(
            Path::new(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
            Path::new(r"C:\Users\Artist\AppData\Local\Temp\neoflash-debug-1"),
            true,
        );
        assert!(cmd.contains("--remote-debugging-pipe"));
        assert!(cmd.contains("--headless=new"));
        assert!(cmd.ends_with("about:blank"));
        assert!(!cmd.contains("--remote-debugging-port"));
    }

    #[test]
    fn browser_executable_resolution_honors_override_and_standard_roots() {
        let dir = std::env::temp_dir().join(format!(
            "neoflash-win-dbg-probe-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let pf = dir.join("ProgramFiles");
        let pf86 = dir.join("ProgramFilesX86");
        let local = dir.join("LocalAppData");
        std::fs::create_dir_all(&pf).unwrap();
        std::fs::create_dir_all(&pf86).unwrap();
        std::fs::create_dir_all(&local).unwrap();

        // Missing override fails closed without falling back to TCP or arbitrary binaries.
        let bad_override = dir.join("missing-chrome.exe");
        let err = browser_executable_from_env(
            Some(bad_override.as_os_str()),
            Some(&pf),
            Some(&pf86),
            Some(&local),
        )
        .unwrap_err();
        assert!(err.contains("NEOFLASH_DEBUG_BROWSER"));

        // Empty roots fail closed with clear guidance and zero TCP fallback.
        let none_err =
            browser_executable_from_env(None, Some(&pf), Some(&pf86), Some(&local)).unwrap_err();
        assert!(none_err.contains("Chrome, Chromium, or Edge"));
        assert!(!none_err.contains("port"));

        // Edge in ProgramFiles(x86) resolves when Chrome is absent.
        let edge_path = pf86.join("Microsoft/Edge/Application/msedge.exe");
        std::fs::create_dir_all(edge_path.parent().unwrap()).unwrap();
        std::fs::write(&edge_path, b"MZ-edge").unwrap();
        assert_eq!(
            browser_executable_from_env(None, Some(&pf), Some(&pf86), Some(&local)).unwrap(),
            edge_path
        );

        // Chromium in LocalAppData outranks Edge.
        let chromium_path = local.join("Chromium/Application/chrome.exe");
        std::fs::create_dir_all(chromium_path.parent().unwrap()).unwrap();
        std::fs::write(&chromium_path, b"MZ-chromium").unwrap();
        assert_eq!(
            browser_executable_from_env(None, Some(&pf), Some(&pf86), Some(&local)).unwrap(),
            chromium_path
        );

        // Chrome in ProgramFiles outranks both.
        let chrome_path = pf.join("Google/Chrome/Application/chrome.exe");
        std::fs::create_dir_all(chrome_path.parent().unwrap()).unwrap();
        std::fs::write(&chrome_path, b"MZ-chrome").unwrap();
        assert_eq!(
            browser_executable_from_env(None, Some(&pf), Some(&pf86), Some(&local)).unwrap(),
            chrome_path
        );

        // Explicit NEOFLASH_DEBUG_BROWSER wins over everything.
        let custom = dir.join("custom-browser.exe");
        std::fs::write(&custom, b"MZ-custom").unwrap();
        assert_eq!(
            browser_executable_from_env(
                Some(custom.as_os_str()),
                Some(&pf),
                Some(&pf86),
                Some(&local),
            )
            .unwrap(),
            custom
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nul_delimited_json_framing_reassembles_chunks_and_enforces_32_mib_cap() {
        let mut pending = Vec::new();
        let m1 =
            encode_nul_delimited_command(&json!({"id": 1, "method": "Target.getTargets"})).unwrap();
        let m2 = encode_nul_delimited_command(&json!({"id": 2, "result": {"ok": true}})).unwrap();

        append_received_chunk(&mut pending, &m1[..10]).unwrap();
        assert!(try_pop_nul_delimited_frame(&mut pending).unwrap().is_none());
        append_received_chunk(&mut pending, &m1[10..]).unwrap();
        append_received_chunk(&mut pending, &m2).unwrap();

        let f1 = try_pop_nul_delimited_frame(&mut pending)
            .unwrap()
            .expect("first frame");
        assert_eq!(f1["id"], 1);
        let f2 = try_pop_nul_delimited_frame(&mut pending)
            .unwrap()
            .expect("second frame");
        assert_eq!(f2["id"], 2);
        assert!(pending.is_empty());

        let mut near_cap = vec![b'a'; MAX_MESSAGE];
        assert!(append_received_chunk(&mut near_cap, b"b").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn win32_private_pipe_roundtrips_fd3_fd4_and_cleans_up_job_descendants() {
        use std::time::{Duration, Instant};

        // Compile a tiny native Rust helper binary that reads a NUL-delimited JSON command from
        // inherited CRT fd 3 (`_get_osfhandle(3)`), spawns a background child (`ping -n 60`), and
        // writes a NUL-delimited JSON reply on CRT fd 4 (`_get_osfhandle(4)`).
        let work_dir = private_directory().expect("csprng private dir");
        let helper_src = work_dir.join("pipe_echo_helper.rs");
        let helper_exe = work_dir.join("pipe_echo_helper.exe");
        std::fs::write(
            &helper_src,
            r#"
use std::ffi::c_void;
use std::process::{Command, Stdio};

unsafe extern "C" {
    fn _get_osfhandle(fd: i32) -> isize;
}
unsafe extern "system" {
    fn ReadFile(h: *mut c_void, buf: *mut u8, len: u32, read: *mut u32, ov: *mut c_void) -> i32;
    fn WriteFile(h: *mut c_void, buf: *const u8, len: u32, written: *mut u32, ov: *mut c_void) -> i32;
}

fn main() {
    let mut profile = String::new();
    for arg in std::env::args() {
        if let Some(p) = arg.strip_prefix("--user-data-dir=") {
            profile = p.to_owned();
        }
    }
    let worker = Command::new("cmd.exe")
        .args(["/C", "ping -n 60 127.0.0.1 >NUL"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn descendant");
    if !profile.is_empty() {
        let _ = std::fs::write(
            std::path::Path::new(&profile).join("worker.pid"),
            worker.id().to_string(),
        );
    }

    let h_read = unsafe { _get_osfhandle(3) } as *mut c_void;
    let h_write = unsafe { _get_osfhandle(4) } as *mut c_void;
    let ready = b"{\"ready\":true}\0";
    let mut written = 0u32;
    unsafe {
        WriteFile(h_write, ready.as_ptr(), ready.len() as u32, &mut written, std::ptr::null_mut());
    }

    let mut incoming = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let mut n = 0u32;
        let ok = unsafe { ReadFile(h_read, byte.as_mut_ptr(), 1, &mut n, std::ptr::null_mut()) };
        if ok == 0 || n == 0 {
            return;
        }
        if byte[0] == 0 {
            break;
        }
        incoming.push(byte[0]);
    }
    incoming.push(0);
    unsafe {
        WriteFile(h_write, incoming.as_ptr(), incoming.len() as u32, &mut written, std::ptr::null_mut());
    }
    std::thread::sleep(std::time::Duration::from_secs(30));
}
"#,
        )
        .unwrap();

        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let status = std::process::Command::new(rustc)
            .arg(&helper_src)
            .arg("-O")
            .arg("-o")
            .arg(&helper_exe)
            .status()
            .expect("invoke rustc for helper");
        assert!(status.success(), "rustc helper build failed");

        let mut pipe = BrowserPipe::launch(&helper_exe, true).expect("launch helper over fd3/fd4");
        let profile_dir = pipe.profile().to_path_buf();
        assert!(profile_dir.is_dir());

        let deadline = Instant::now() + Duration::from_secs(5);
        let ready = loop {
            if let Some(msg) = pipe.receive().unwrap() {
                break msg;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for ready on fd4"
            );
        };
        assert_eq!(ready["ready"], true);

        pipe.send(&json!({"id": 42, "method": "Browser.getVersion"}))
            .unwrap();
        let echoed = loop {
            if let Some(msg) = pipe.receive().unwrap() {
                break msg;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for echo on fd4"
            );
        };
        assert_eq!(echoed["id"], 42);
        assert_eq!(echoed["method"], "Browser.getVersion");

        let worker_pid: u32 = std::fs::read_to_string(profile_dir.join("worker.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let leader_pid = pipe.child_pid();
        assert!(leader_pid > 0 && worker_pid > 0);

        drop(pipe);
        assert!(
            !profile_dir.exists(),
            "Drop must clean up private profile dir"
        );
        let _ = std::fs::remove_dir_all(&work_dir);
    }
}
