//! Private DevTools pipe to a browser owned by this Test Movie session.
//!
//! No TCP debugger endpoint or user's browser profile is opened. Chromium reads
//! NUL-delimited protocol messages on fd 3 and writes them on fd 4.
use serde_json::Value;
use std::fs::{self, DirBuilder, File};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const MAX_MESSAGE: usize = 32 * 1024 * 1024;

pub(crate) struct BrowserPipe {
    child: Child,
    stream: UnixStream,
    pending: Vec<u8>,
    profile: PathBuf,
}

pub(crate) fn browser_executable() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("NEOFLASH_DEBUG_BROWSER") {
        let path = PathBuf::from(path);
        return path
            .is_file()
            .then_some(path)
            .ok_or_else(|| "NEOFLASH_DEBUG_BROWSER must name a Chromium executable.".into());
    }
    [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
    .ok_or_else(|| "Debugging Test Movie requires Chrome or Chromium. Set NEOFLASH_DEBUG_BROWSER to its executable.".into())
}

pub(crate) fn private_directory() -> io::Result<PathBuf> {
    let mut random = [0_u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut random)?;
    let suffix: String = random.iter().map(|v| format!("{v:02x}")).collect();
    let path = std::env::temp_dir().join(format!("neoflash-debug-{}-{suffix}", std::process::id()));
    DirBuilder::new().mode(0o700).create(&path)?;
    Ok(path)
}

impl BrowserPipe {
    pub(crate) fn launch(executable: &Path, headless: bool) -> io::Result<Self> {
        let profile = private_directory()?;
        let result = Self::launch_in(executable, headless, profile.clone());
        if result.is_err() {
            let _ = fs::remove_dir_all(profile);
        }
        result
    }

    fn launch_in(executable: &Path, headless: bool, profile: PathBuf) -> io::Result<Self> {
        let (stream, child_stream) = UnixStream::pair()?;
        stream.set_read_timeout(Some(Duration::from_millis(40)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        // Reserve a descriptor above 3/4 before fork, avoiding collisions with
        // Command's own stdio pipe setup. Only async-signal-safe dup2 runs below.
        let raw = unsafe { libc::fcntl(child_stream.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 5) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        let child_fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let mut command = Command::new(executable);
        command
            .arg("--remote-debugging-pipe")
            .arg(format!("--user-data-dir={}", profile.display()))
            .args([
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
                "about:blank",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        if headless {
            command.arg("--headless=new");
        }
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(raw, 3) < 0 || libc::dup2(raw, 4) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        drop(child_fd);
        drop(child_stream);
        Ok(Self {
            child,
            stream,
            pending: Vec::new(),
            profile,
        })
    }

    pub(crate) fn send(&mut self, message: &Value) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(message)?;
        if bytes.len() > MAX_MESSAGE {
            return Err(io::Error::other(
                "Debugger command exceeds the message limit",
            ));
        }
        bytes.push(0);
        self.stream.write_all(&bytes)
    }

    pub(crate) fn receive(&mut self) -> io::Result<Option<Value>> {
        loop {
            if let Some(end) = self.pending.iter().position(|b| *b == 0) {
                let result = serde_json::from_slice(&self.pending[..end])?;
                self.pending.drain(..=end);
                return Ok(Some(result));
            }
            let mut bytes = [0_u8; 8192];
            match self.stream.read(&mut bytes) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Test Movie debugger disconnected",
                    ));
                }
                Ok(count) => {
                    if self.pending.len() + count > MAX_MESSAGE {
                        return Err(io::Error::other(
                            "Debugger response exceeds the message limit",
                        ));
                    }
                    self.pending.extend_from_slice(&bytes[..count]);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    if self.leader_exited()? {
                        return Err(io::Error::other("Test Movie browser exited"));
                    }
                    return Ok(None);
                }
                Err(error) => return Err(error),
            }
        }
    }

    // Keep the leader unreaped until its group is terminated. Child::try_wait
    // would release its PID, making later group cleanup vulnerable to PID reuse
    // and leaving Chromium descendants behind when their leader exits first.
    fn leader_exited(&self) -> io::Result<bool> {
        loop {
            let mut status = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.child.id() as libc::id_t,
                    &mut status,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result == 0 {
                return Ok(unsafe { status.si_pid() } != 0);
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn profile(&self) -> &Path {
        &self.profile
    }
}

impl Drop for BrowserPipe {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        if self.leader_exited().is_ok() {
            // A successful WNOWAIT observation proves we still own this
            // unreaped child, retaining its PID even if it already exited.
            // Terminate the entire private group, including helpers that ignore
            // SIGTERM, before releasing that identity by waiting below.
            unsafe {
                libc::killpg(self.child.id() as libc::pid_t, libc::SIGKILL);
            }
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.profile);
    }
}

pub(crate) fn file_url(path: &Path) -> io::Result<String> {
    use std::os::unix::ffi::OsStrExt;
    let path = fs::canonicalize(path)?;
    let mut result = String::from("file://");
    for byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(byte) {
            result.push(*byte as char);
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Instant;

    struct Fixture(PathBuf);

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn running(pid: libc::pid_t) -> bool {
        match Command::new("/bin/ps")
            .args(["-o", "state=", "-p", &pid.to_string()])
            .output()
        {
            Ok(output) => {
                let state = String::from_utf8_lossy(&output.stdout);
                !state.trim().is_empty() && !state.trim().starts_with('Z')
            }
            Err(_) => unsafe { libc::kill(pid, 0) == 0 },
        }
    }

    #[test]
    fn private_pipe_cleanup_owns_running_and_exited_leader_descendants() {
        let fixture = Fixture(private_directory().unwrap());
        for leader_exits in [false, true] {
            let executable = fixture.0.join(if leader_exits { "exits" } else { "waits" });
            fs::write(
                &executable,
                format!(
                    "#!/bin/sh\nfor arg in \"$@\"; do\ncase \"$arg\" in --user-data-dir=*) profile=${{arg#*=}};; esac\ndone\n/bin/sh -c 'trap \"\" TERM; exec /bin/sleep 60' &\nworker=$!\nprintf '%s' \"$worker\" > \"$profile/worker.pid\"\nprintf '{{\"ready\":true}}\\000' >&4\n{}\n",
                    if leader_exits { "exit 0" } else { "wait \"$worker\"" }
                ),
            )
            .unwrap();
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
            let mut pipe = BrowserPipe::launch(&executable, false).unwrap();
            let profile = pipe.profile().to_owned();
            let leader = pipe.child.id() as libc::pid_t;
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                if let Some(message) = pipe.receive().unwrap() {
                    assert_eq!(message["ready"], true);
                    break;
                }
                assert!(Instant::now() < deadline, "fake browser did not start");
            }
            let worker: libc::pid_t = fs::read_to_string(profile.join("worker.pid"))
                .unwrap()
                .parse()
                .unwrap();
            assert_eq!(unsafe { libc::getpgid(worker) }, leader);
            assert!(running(worker));
            if leader_exits {
                while !pipe.leader_exited().unwrap() {
                    assert!(Instant::now() < deadline, "fake browser did not exit");
                    std::thread::sleep(Duration::from_millis(10));
                }
                // The descendant inherits fd 4, so only the non-reaping process
                // observation detects this disconnected leader on read timeout.
                assert!(pipe.receive().is_err());
                assert!(pipe.leader_exited().unwrap());
            } else {
                assert!(!pipe.leader_exited().unwrap());
            }
            drop(pipe);
            assert!(!profile.exists());
            let deadline = Instant::now() + Duration::from_secs(3);
            while running(worker) {
                assert!(
                    Instant::now() < deadline,
                    "owned browser helper survived cleanup"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(!running(leader));
        }
    }
}
