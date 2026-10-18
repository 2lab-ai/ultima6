//! A run dir and its files, reached through open dirs and never by a path: `--out` is opened
//! once (a link at `--out` itself followed), the run dir made and opened in it, and each of the
//! run's files made, read, checked and deleted by its name in the run dir it holds open. A dir
//! removed, renamed or swapped for a link meanwhile is never followed, and nothing is made
//! again. Unix only (the crate is).

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

fn c(bytes: &[u8]) -> io::Result<CString> {
    CString::new(bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a NUL in it"))
}

/// `fd`, owned, or the error of the call that gave it.
fn owned(fd: libc::c_int) -> io::Result<File> {
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `fd` was just returned open by the call, and nothing else owns it.
    Ok(File::from(unsafe { OwnedFd::from_raw_fd(fd) }))
}

fn done(r: libc::c_int) -> io::Result<()> {
    if r == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The dir at `path`, opened, a link at it followed (`--out`).
pub fn open_dir(path: &Path) -> io::Result<File> {
    let p = c(path.as_os_str().as_bytes())?;
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC;
    // SAFETY: a NUL-terminated path that outlives the call.
    owned(unsafe { libc::open(p.as_ptr(), flags) })
}

/// Makes the dir `name` in the open dir `dir` (`mkdirat`): a failure when anything is there.
pub fn mkdir(dir: &File, name: &str) -> io::Result<()> {
    let n = c(name.as_bytes())?;
    let mode: libc::mode_t = 0o755;
    // SAFETY: an open dir and a NUL-terminated name that outlive the call.
    done(unsafe { libc::mkdirat(dir.as_raw_fd(), n.as_ptr(), mode) })
}

/// The dir `name` of the open dir `dir`, opened: refused when it is a link (`O_NOFOLLOW`) or
/// not a dir.
pub fn open_dir_at(dir: &File, name: &str) -> io::Result<File> {
    let n = c(name.as_bytes())?;
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: an open dir and a NUL-terminated name that outlive the call.
    owned(unsafe { libc::openat(dir.as_raw_fd(), n.as_ptr(), flags) })
}

/// A new file `name` in the open dir `dir`, made by this call: `O_CREAT | O_EXCL | O_NOFOLLOW`,
/// so it fails when anything is at the name, a link too, and when the dir is gone.
pub fn create(dir: &File, name: &str) -> io::Result<File> {
    let n = c(name.as_bytes())?;
    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let mode: libc::c_uint = 0o644;
    // SAFETY: an open dir and a NUL-terminated name that outlive the call.
    owned(unsafe { libc::openat(dir.as_raw_fd(), n.as_ptr(), flags, mode) })
}

/// The file `name` of the open dir `dir`, to read; not followed when a link.
pub fn open(dir: &File, name: &str) -> io::Result<File> {
    let n = c(name.as_bytes())?;
    let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    // SAFETY: an open dir and a NUL-terminated name that outlive the call.
    owned(unsafe { libc::openat(dir.as_raw_fd(), n.as_ptr(), flags) })
}

/// The (device, inode) of the entry `name` of the open dir `dir`, itself (a link is not
/// followed: its own identity), with no file opened (`fstatat`, `AT_SYMLINK_NOFOLLOW`).
pub fn id_at(dir: &File, name: &str) -> io::Result<(u64, u64)> {
    let n = c(name.as_bytes())?;
    // SAFETY: an all-zero `stat` is a valid value for the call to fill.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: an open dir, a NUL-terminated name and a `stat` that outlive the call.
    done(unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            n.as_ptr(),
            &mut st,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    })?;
    // `dev_t` is i32 on macOS, u64 on Linux: the same widening `MetadataExt::dev` makes.
    #[allow(clippy::unnecessary_cast)]
    Ok((st.st_dev as u64, st.st_ino as u64))
}

/// Removes the entry `name` of the open dir `dir` (`unlinkat`): never a dir, never by a path.
pub fn unlink(dir: &File, name: &str) -> io::Result<()> {
    let n = c(name.as_bytes())?;
    // SAFETY: an open dir and a NUL-terminated name that outlive the call.
    done(unsafe { libc::unlinkat(dir.as_raw_fd(), n.as_ptr(), 0) })
}

/// The (device, inode) of an open file.
pub fn id(f: &File) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = f.metadata()?;
    Ok((m.dev(), m.ino()))
}

/// The (device, inode) of the dir at `path` (`None`: nothing there, a link, or not a dir).
pub fn path_id(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::symlink_metadata(path).ok()?;
    m.is_dir().then(|| (m.dev(), m.ino()))
}

/// Where the open dir `f` is now, when the system says (macOS `F_GETPATH`, Linux
/// `/proc/self/fd`), and it is the same dir there.
pub fn where_now(f: &File) -> Option<PathBuf> {
    let path = path_of(f)?;
    (path_id(&path) == id(f).ok()).then_some(path)
}

#[cfg(target_os = "macos")]
fn path_of(f: &File) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let mut buf = vec![0u8; libc::PATH_MAX as usize];
    // SAFETY: an open fd and a buffer of PATH_MAX bytes, as F_GETPATH asks, that outlive the call.
    let r = unsafe { libc::fcntl(f.as_raw_fd(), libc::F_GETPATH, buf.as_mut_ptr()) };
    if r < 0 {
        return None;
    }
    let len = buf.iter().position(|&b| b == 0)?;
    buf.truncate(len);
    Some(PathBuf::from(std::ffi::OsString::from_vec(buf)))
}

#[cfg(target_os = "linux")]
fn path_of(f: &File) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/self/fd/{}", f.as_raw_fd())).ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn path_of(_: &File) -> Option<PathBuf> {
    None
}

/// The system's cap on a process's open files where it has one below the hard limit (macOS:
/// `OPEN_MAX` of `<sys/syslimits.h>` and `kern.maxfilesperproc`).
#[cfg(target_os = "macos")]
fn cap() -> libc::rlim_t {
    const OPEN_MAX: libc::rlim_t = 10240;
    let mut per: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>();
    let Ok(name) = c(b"kern.maxfilesperproc") else {
        return OPEN_MAX;
    };
    // SAFETY: a NUL-terminated name, an int and its size, as sysctlbyname asks.
    let r = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut per as *mut libc::c_int).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    match libc::rlim_t::try_from(per) {
        Ok(p) if r == 0 && p > 0 => p.min(OPEN_MAX),
        _ => OPEN_MAX,
    }
}

/// No cap below the hard limit but a large one (a hard limit of "infinity" is not taken).
#[cfg(not(target_os = "macos"))]
fn cap() -> libc::rlim_t {
    1 << 20
}

/// Raises the soft limit on open files toward the hard one, as far as the system caps it: a run
/// holds each file it makes open until it ends. Best effort: the limit as it was when it cannot.
pub fn raise_open_files() {
    // SAFETY: an all-zero `rlimit` is a valid value for the call to fill.
    let mut r: libc::rlimit = unsafe { std::mem::zeroed() };
    // SAFETY: a `rlimit` that outlives the call.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut r) } != 0 {
        return;
    }
    let want = r.rlim_max.min(cap());
    if want > r.rlim_cur {
        let up = libc::rlimit {
            rlim_cur: want,
            rlim_max: r.rlim_max,
        };
        // SAFETY: a `rlimit` that outlives the call.
        let _ = unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &up) };
    }
}
