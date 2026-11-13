//! Single instance on Linux: the first copy owns a Unix socket; later copies send it one line
//! (`open`, `toggle` or `ask`) and exit.

use bloop::launch::Request;
use std::io::{BufRead, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

const SOCKET: &str = "instance.sock";

/// `<runtime>/bloop`, or `/tmp/bloop-<uid>` when there is no runtime directory.
pub fn socket_dir(runtime_dir: Option<&Path>, uid: u32) -> PathBuf {
    match runtime_dir {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join("bloop"),
        _ => PathBuf::from(format!("/tmp/bloop-{uid}")),
    }
}

/// Creates `dir` private to this user, or checks that an existing one is.
fn prepare_dir(dir: &Path) -> std::io::Result<()> {
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    match std::fs::DirBuilder::new().mode(0o700).create(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.is_dir() || meta.uid() != uid || meta.permissions().mode() & 0o077 != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{} is not private to this user", dir.display()),
        ));
    }
    Ok(())
}

/// Binds the instance socket. `Ok(None)` means another live copy owns it.
pub fn bind(dir: &Path) -> std::io::Result<Option<UnixListener>> {
    prepare_dir(dir)?;
    let path = dir.join(SOCKET);
    match UnixListener::bind(&path) {
        Ok(listener) => Ok(Some(listener)),
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            match UnixStream::connect(&path) {
                Ok(_) => Ok(None),
                // Left behind by a copy that crashed: nobody listens, so take it over.
                Err(_) => {
                    std::fs::remove_file(&path)?;
                    UnixListener::bind(&path).map(Some)
                }
            }
        }
        Err(error) => Err(error),
    }
}

/// Sends one request to the running copy.
pub fn send(dir: &Path, request: Request) -> std::io::Result<()> {
    let mut stream = UnixStream::connect(dir.join(SOCKET))?;
    writeln!(stream, "{}", request.as_str())
}

/// Reads one request from an accepted connection.
pub fn receive(stream: UnixStream) -> Option<Request> {
    // A request is one short word: cap what a peer can make bloop read or wait for.
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    let mut line = String::new();
    std::io::BufReader::new(std::io::Read::take(stream, 64))
        .read_line(&mut line)
        .ok()?;
    Request::parse(&line)
}

/// Removes the socket file when the owning copy exits.
pub fn remove(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(SOCKET));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bloop-inst-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn second_bind_is_refused() {
        let dir = temp("second");
        let first = bind(&dir).unwrap();
        assert!(first.is_some());
        assert!(bind(&dir).unwrap().is_none());
        remove(&dir);
    }

    #[test]
    fn send_reaches_listener() {
        let dir = temp("send");
        let listener = bind(&dir).unwrap().unwrap();
        send(&dir, Request::Toggle).unwrap();
        let (stream, _) = listener.accept().unwrap();
        assert_eq!(receive(stream), Some(Request::Toggle));
        remove(&dir);
    }

    #[test]
    fn stale_socket_is_replaced() {
        let dir = temp("stale");
        drop(bind(&dir).unwrap().unwrap()); // listener gone, file left behind
        assert!(dir.join(SOCKET).exists());
        assert!(bind(&dir).unwrap().is_some());
        remove(&dir);
    }

    #[test]
    fn runtime_dir_fallback_is_private() {
        assert_eq!(socket_dir(None, 1000), PathBuf::from("/tmp/bloop-1000"));
        assert_eq!(
            socket_dir(Some(Path::new("/run/user/1000")), 1000),
            PathBuf::from("/run/user/1000/bloop")
        );
        let dir = temp("private");
        bind(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        remove(&dir);

        let open = temp("open");
        std::fs::create_dir(&open).unwrap();
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert!(bind(&open).is_err());
    }

    #[test]
    fn unknown_requests_are_ignored() {
        assert_eq!(Request::parse("rm -rf"), None);
        assert_eq!(Request::parse("ask\n"), Some(Request::Ask));
    }
}
