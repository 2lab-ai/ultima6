//! A run dir's files, reached through the dir's own open file and never by its path: each
//! name is looked up in the dir this process made, so a dir removed, renamed or swapped for a
//! link meanwhile is never followed, and nothing is made again. Unix only; elsewhere every call
//! fails ([`UNSUPPORTED`]).

use std::fs::File;
use std::io;
use std::path::Path;

/// What every call says on a host that is not unix.
pub const UNSUPPORTED: &str = "calibrate needs a unix host (its run dir is written through the \
                               dir's own descriptor: openat, unlinkat)";

#[cfg(unix)]
mod imp {
    use std::ffi::CString;
    use std::fs::File;
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

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

    pub fn open_dir(path: &Path) -> io::Result<File> {
        let p = c(path.as_os_str().as_bytes())?;
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        // SAFETY: a NUL-terminated path that outlives the call.
        owned(unsafe { libc::open(p.as_ptr(), flags) })
    }

    pub fn create(dir: &File, name: &str) -> io::Result<File> {
        let n = c(name.as_bytes())?;
        let flags =
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        let mode: libc::c_uint = 0o644;
        // SAFETY: an open dir and a NUL-terminated name that outlive the call.
        owned(unsafe { libc::openat(dir.as_raw_fd(), n.as_ptr(), flags, mode) })
    }

    pub fn open(dir: &File, name: &str) -> io::Result<File> {
        let n = c(name.as_bytes())?;
        let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
        // SAFETY: an open dir and a NUL-terminated name that outlive the call.
        owned(unsafe { libc::openat(dir.as_raw_fd(), n.as_ptr(), flags) })
    }

    pub fn unlink(dir: &File, name: &str) -> io::Result<()> {
        let n = c(name.as_bytes())?;
        // SAFETY: an open dir and a NUL-terminated name that outlive the call.
        if unsafe { libc::unlinkat(dir.as_raw_fd(), n.as_ptr(), 0) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn id(f: &File) -> io::Result<(u64, u64)> {
        use std::os::unix::fs::MetadataExt;
        let m = f.metadata()?;
        Ok((m.dev(), m.ino()))
    }

    pub fn path_id(path: &Path) -> Option<(u64, u64)> {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::symlink_metadata(path).ok()?;
        m.is_dir().then(|| (m.dev(), m.ino()))
    }
}

#[cfg(not(unix))]
mod imp {
    use std::fs::File;
    use std::io;
    use std::path::Path;

    fn no<T>() -> io::Result<T> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            super::UNSUPPORTED,
        ))
    }

    pub fn open_dir(_: &Path) -> io::Result<File> {
        no()
    }
    pub fn create(_: &File, _: &str) -> io::Result<File> {
        no()
    }
    pub fn open(_: &File, _: &str) -> io::Result<File> {
        no()
    }
    pub fn unlink(_: &File, _: &str) -> io::Result<()> {
        no()
    }
    pub fn id(_: &File) -> io::Result<(u64, u64)> {
        no()
    }
    pub fn path_id(_: &Path) -> Option<(u64, u64)> {
        None
    }
}

/// The dir at `path`, opened: refused when it is a link (`O_NOFOLLOW`) or not a dir.
pub fn open_dir(path: &Path) -> io::Result<File> {
    imp::open_dir(path)
}

/// A new file `name` in the open dir `dir`, made by this call: `O_CREAT | O_EXCL | O_NOFOLLOW`,
/// so it fails when anything is at the name, a link too, and when the dir is gone.
pub fn create(dir: &File, name: &str) -> io::Result<File> {
    imp::create(dir, name)
}

/// The file `name` of the open dir `dir`, to read; not followed when a link.
pub fn open(dir: &File, name: &str) -> io::Result<File> {
    imp::open(dir, name)
}

/// Removes the entry `name` of the open dir `dir` (`unlinkat`): never a dir, never by a path.
pub fn unlink(dir: &File, name: &str) -> io::Result<()> {
    imp::unlink(dir, name)
}

/// The (device, inode) of an open file.
pub fn id(f: &File) -> io::Result<(u64, u64)> {
    imp::id(f)
}

/// The (device, inode) of the dir at `path` (`None`: nothing there, a link, or not a dir).
pub fn path_id(path: &Path) -> Option<(u64, u64)> {
    imp::path_id(path)
}
