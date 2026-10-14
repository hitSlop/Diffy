//! Per-user, bounded local requests. The lock outlives the listener so stale
//! Unix sockets can only be removed by the next instance that owns the lock.
use fs2::FileExt as _;
use interprocess::local_socket::{ListenerNonblockingMode, ListenerOptions, Stream, prelude::*};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const LIMIT: usize = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    version: u8,
    paths: Vec<PathBuf>,
}

pub enum Start {
    Forwarded,
    Primary(Instance),
}
pub struct Instance {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Drop for Instance {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn endpoint(directory: &Path) -> io::Result<interprocess::local_socket::Name<'static>> {
    #[cfg(windows)]
    {
        let hash = Sha256::digest(directory.as_os_str().to_string_lossy().as_bytes());
        format!("diffy-{:x}", hash)
            .to_ns_name::<interprocess::local_socket::GenericNamespaced>()
            .map(|name| name.into_owned())
    }
    #[cfg(not(windows))]
    {
        directory
            .join("open.sock")
            .to_fs_name::<interprocess::local_socket::GenericFilePath>()
            .map(|name| name.into_owned())
    }
}
fn connect(directory: &Path, deadline: Instant) -> io::Result<Stream> {
    #[cfg(windows)]
    {
        interprocess::local_socket::ConnectOptions::new()
            .name(endpoint(directory)?)
            .wait_mode(interprocess::ConnectWaitMode::Timeout(
                deadline.saturating_duration_since(Instant::now()),
            ))
            .connect_sync()
    }
    #[cfg(not(windows))]
    {
        let _ = deadline;
        Stream::connect(endpoint(directory)?)
    }
}

fn runtime_directory(state: &Path) -> io::Result<PathBuf> {
    let parent = state.parent().ok_or(io::ErrorKind::InvalidInput)?;
    #[cfg(windows)]
    {
        Ok(parent.join("runtime"))
    }
    #[cfg(not(windows))]
    {
        // Unix socket names have a small fixed limit. Hash the full state path,
        // preserving isolation for overrides without putting it in the socket name.
        let hash = Sha256::digest(state.as_os_str().to_string_lossy().as_bytes());
        let base = directories::ProjectDirs::from("com", "hitSlop", "Diffy")
            .and_then(|dirs| dirs.runtime_dir().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let _ = parent;
        Ok(base.join(format!("diffy-{}", &format!("{hash:x}")[..24])))
    }
}

pub fn start(
    state: &Path,
    paths: Vec<PathBuf>,
    sender: async_channel::Sender<Vec<String>>,
) -> Result<Start, String> {
    start_in(
        &runtime_directory(state).map_err(|e| e.to_string())?,
        paths,
        sender,
    )
    .map_err(|e| format!("Couldn't contact Diffy: {e}"))
}

fn start_in(
    directory: &Path,
    paths: Vec<PathBuf>,
    sender: async_channel::Sender<Vec<String>>,
) -> io::Result<Start> {
    if paths.iter().any(|path| !path.is_absolute()) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    fs::create_dir_all(directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = fs::symlink_metadata(directory)?;
        let home = directories::BaseDirs::new().ok_or(io::ErrorKind::NotFound)?;
        if metadata.file_type().is_symlink()
            || metadata.uid() != fs::metadata(home.home_dir())?.uid()
        {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join("instance.lock"))?;
    match lock.try_lock_exclusive() {
        Ok(()) => {}
        Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
            let deadline = Instant::now() + TIMEOUT;
            loop {
                match connect(directory, deadline) {
                    Ok(mut stream) => {
                        #[cfg(windows)]
                        if let Ok(peer) = stream.peer_creds()
                            && let Some(pid) = peer.pid()
                        {
                            // Transfer foreground activation permission only to
                            // the app process at the connected per-user endpoint.
                            unsafe {
                                windows_sys::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid);
                            }
                        }
                        stream.set_nonblocking(true)?;
                        let mut request = serde_json::to_vec(&Request { version: 1, paths })
                            .map_err(io::Error::other)?;
                        request.push(b'\n');
                        if request.len() > LIMIT {
                            return Err(io::ErrorKind::InvalidInput.into());
                        }
                        send(&mut stream, &request, deadline)?;
                        let reply = receive(&mut stream, deadline)?;
                        send(&mut stream, b"ack\n", deadline)?;
                        return if reply == b"ok" {
                            Ok(Start::Forwarded)
                        } else {
                            Err(io::Error::other("the running app rejected the request"))
                        };
                    }
                    Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                    Err(error) => return Err(error),
                }
            }
        }
        Err(error) => return Err(error),
    }
    #[cfg(not(windows))]
    match fs::remove_file(directory.join("open.sock")) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let listener = ListenerOptions::new()
        .name(endpoint(directory)?)
        .nonblocking(ListenerNonblockingMode::Both)
        .create_sync()?;
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let worker = thread::spawn(move || {
        let _lock: File = lock;
        while !stopped.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok(mut stream) => {
                    let deadline = Instant::now() + TIMEOUT;
                    let result = receive(&mut stream, deadline).and_then(|bytes| {
                        let request: Request =
                            serde_json::from_slice(&bytes).map_err(io::Error::other)?;
                        if request.version != 1 || request.paths.iter().any(|p| !p.is_absolute()) {
                            return Err(io::ErrorKind::InvalidInput.into());
                        }
                        let url = crate::cli::open_url(&request.paths).map_err(io::Error::other)?;
                        sender.try_send(vec![url]).map_err(io::Error::other)
                    });
                    if send(
                        &mut stream,
                        if result.is_ok() { b"ok\n" } else { b"error\n" },
                        deadline,
                    )
                    .is_ok()
                    {
                        // Keep the Windows pipe alive until the client consumed
                        // the reply. Disconnecting first can discard buffered bytes.
                        let _ = receive(&mut stream, deadline);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(_) => break,
            }
        }
        drop(listener);
    });
    Ok(Start::Primary(Instance {
        stop,
        worker: Some(worker),
    }))
}

fn send(stream: &mut Stream, mut bytes: &[u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        match stream.write(bytes) {
            Ok(0) if cfg!(windows) => thread::sleep(Duration::from_millis(5)),
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(count) => bytes = &bytes[count..],
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5))
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
fn receive(stream: &mut Stream, deadline: Instant) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        if Instant::now() >= deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let mut chunk = [0; 4096];
        match stream.read(&mut chunk) {
            // Windows PIPE_NOWAIT can report an empty pipe as a zero-byte read
            // with this interprocess snapshot. Treat it as pending until the deadline.
            Ok(0) if cfg!(windows) => thread::sleep(Duration::from_millis(5)),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(count) => {
                bytes.extend_from_slice(&chunk[..count]);
                if bytes.len() > LIMIT {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                if let Some(end) = bytes.iter().position(|b| *b == b'\n') {
                    bytes.truncate(end);
                    return Ok(bytes);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5))
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requests_forward_and_released_instances_can_restart() {
        let temp = tempfile::tempdir().unwrap();
        let (sender, receiver) = async_channel::unbounded();
        let primary = match start_in(temp.path(), vec![], sender.clone()).unwrap() {
            Start::Primary(primary) => primary,
            _ => panic!("first instance"),
        };
        let path = temp.path().join("日本語 space #?%&");
        assert!(matches!(
            start_in(temp.path(), vec![path.clone()], sender.clone()).unwrap(),
            Start::Forwarded
        ));
        assert_eq!(
            crate::cli::paths_from_url(&receiver.recv_blocking().unwrap()[0]).unwrap(),
            vec![path]
        );
        drop(primary);
        assert!(matches!(
            start_in(temp.path(), vec![], sender).unwrap(),
            Start::Primary(_)
        ));
    }
    #[test]
    fn invalid_requests_do_not_open_projects() {
        let temp = tempfile::tempdir().unwrap();
        let (sender, receiver) = async_channel::unbounded();
        let _primary = start_in(temp.path(), vec![], sender).unwrap();
        let mut stream = Stream::connect(endpoint(temp.path()).unwrap()).unwrap();
        stream.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + TIMEOUT;
        send(&mut stream, b"{\"version\":2,\"paths\":[]}\n", deadline).unwrap();
        assert_eq!(receive(&mut stream, deadline).unwrap(), b"error");
        send(&mut stream, b"ack\n", deadline).unwrap();
        assert!(receiver.try_recv().is_err());
    }
}
