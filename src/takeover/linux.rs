use super::is_conflicting;
use crate::error::{Error, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(50);

/// Holds the daemon's PID while it runs.
fn lock_path() -> PathBuf {
    crate::config::runtime_dir().join("vulcand.lock")
}

/// Non-blocking `flock`. `Ok(false)` when another process holds a
/// conflicting lock.
fn try_flock(file: &File, op: libc::c_int) -> Result<bool> {
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), op | libc::LOCK_NB) } == 0 {
            return Ok(true);
        }
        let e = std::io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::EWOULDBLOCK) => return Ok(false),
            Some(libc::EINTR) => continue,
            _ => return Err(Error::new(format!("flock: {e}"))),
        }
    }
}

/// Held by the running `vulcand` for its whole lifetime. The kernel drops
/// the `flock` when the process exits, however it exits.
pub struct DaemonLock {
    _file: File,
}

impl DaemonLock {
    /// `None` when another `vulcand` already holds it.
    pub fn acquire() -> Result<Option<Self>> {
        let path = lock_path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)
                .map_err(|e| Error::new(format!("create {}: {e}", dir.display())))?;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(|e| Error::new(format!("open {}: {e}", path.display())))?;
        if !try_flock(&file, libc::LOCK_EX)? {
            return Ok(None);
        }
        // Only the lock holder writes, so `stop_daemon` reads a whole PID or
        // nothing.
        file.set_len(0)?;
        file.write_all(format!("{}\n", std::process::id()).as_bytes())?;
        Ok(Some(Self { _file: file }))
    }
}

/// Ask a running `vulcand` to exit (SIGTERM) and wait until it has released
/// the device. Returns false when no daemon was running.
pub fn stop_daemon(timeout: Duration) -> Result<bool> {
    let path = lock_path();
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(Error::new(format!("open {}: {e}", path.display()))),
    };
    if try_flock(&file, libc::LOCK_SH)? {
        return Ok(false);
    }

    let deadline = Instant::now() + timeout;
    let mut signalled = false;
    loop {
        if !signalled {
            // Empty while a just-started daemon has the lock but has not
            // written its PID yet; retry on the next poll.
            let pid = fs::read_to_string(&path)
                .ok()
                .and_then(|s| s.trim().parse::<libc::pid_t>().ok())
                .filter(|&pid| pid > 0);
            if let Some(pid) = pid {
                if unsafe { libc::kill(pid, libc::SIGTERM) } != 0
                    && std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
                {
                    return Err(Error::last_os(&format!("signal vulcand (pid {pid})")));
                }
                signalled = true;
            }
        }
        sleep(POLL);
        // The lock is released only once the daemon has restored the
        // keyboard and exited.
        if try_flock(&file, libc::LOCK_SH)? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Err(Error::new("vulcand did not exit in time"));
        }
    }
}

/// Close OpenRGB if running. Returns the closed process names.
pub fn close_conflicting_apps() -> Vec<String> {
    let me = std::process::id();
    let mut closed = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return closed;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if pid == me {
            continue;
        }
        let Ok(comm) = fs::read_to_string(entry.path().join("comm")) else {
            continue;
        };
        let name = comm.trim_end_matches('\n');
        if is_conflicting(name) && terminate(pid) {
            closed.push(name.to_owned());
        }
    }
    closed
}

/// SIGTERM, then wait up to 3 s for the process to go away.
fn terminate(pid: u32) -> bool {
    let pid = pid as libc::pid_t;
    if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
        return false;
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if !alive(pid) {
            return true;
        }
        sleep(POLL);
    }
    false
}

/// False once the process is gone or a zombie waiting to be reaped.
fn alive(pid: libc::pid_t) -> bool {
    let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    // `pid (comm) state ...`; comm may contain spaces and parentheses.
    let state = stat
        .rsplit_once(')')
        .and_then(|(_, rest)| rest.trim_start().chars().next());
    state != Some('Z')
}
