use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

pub fn open_under(root: &Path, path: &Path) -> io::Result<File> {
    if !root.is_absolute() || !path.starts_with(root) || path == root {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "file outside transcript root"));
    }
    open(path)
}

pub fn open(path: &Path) -> io::Result<File> {
    if !path.is_absolute() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "absolute path required"));
    }
    let mut directory = File::open("/")?;
    let mut parts = path.components().peekable();
    while let Some(part) = parts.next() {
        let name = match part {
            Component::RootDir => continue,
            Component::Normal(name) => CString::new(name.as_bytes())?,
            _ => return Err(io::Error::new(io::ErrorKind::PermissionDenied, "non-normal path component")),
        };
        let mut flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
        if parts.peek().is_some() {
            flags |= libc::O_DIRECTORY;
        }
        // Each lookup is relative to an owned descriptor, so replacing an
        // ancestor with a symlink cannot redirect a later component.
        let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // openat returned a new descriptor; this File takes sole ownership.
        directory = unsafe { File::from_raw_fd(fd) };
    }
    if !directory.metadata()?.is_file() {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "regular transcript file required"));
    }
    Ok(directory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn rejects_links_traversal_and_nonfiles() {
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!("shepsky-secure-{}", std::process::id()));
        std::fs::create_dir_all(root.join("inside")).unwrap();
        std::fs::write(root.join("inside/chat.jsonl"), b"synthetic").unwrap();
        symlink(root.join("inside"), root.join("linked")).unwrap();
        symlink(root.join("inside/chat.jsonl"), root.join("file.jsonl")).unwrap();
        assert!(open_under(&root, &root.join("inside/chat.jsonl")).is_ok());
        assert!(open_under(&root, &root.join("linked/chat.jsonl")).is_err());
        assert!(open_under(&root, &root.join("file.jsonl")).is_err());
        assert!(open_under(&root, &root.join("inside/../inside/chat.jsonl")).is_err());
        assert!(open_under(&root, &root.join("inside")).is_err());
        assert!(open_under(&root.join("inside"), &root.join("file.jsonl")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
