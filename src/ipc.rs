//! One-line commands on a Unix socket.
//!
//! The daemon binds `$XDG_RUNTIME_DIR/subtext.sock`. A client writes
//! `toggle`, `show`, or `hide` and exits. A stale socket file is removed
//! when nothing is listening. Without `XDG_RUNTIME_DIR` the socket lives in
//! a `0700` directory under `/tmp`.

use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use iced::Subscription;
use iced::futures::Stream;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Toggle,
    Show,
    Hide,
}

impl Command {
    pub fn parse(line: &str) -> Option<Self> {
        match line.trim() {
            "toggle" => Some(Self::Toggle),
            "show" => Some(Self::Show),
            "hide" => Some(Self::Hide),
            _ => None,
        }
    }
}

impl std::fmt::Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Toggle => "toggle",
            Self::Show => "show",
            Self::Hide => "hide",
        })
    }
}

#[derive(Debug)]
pub enum BindError {
    AlreadyRunning,
    Io(io::Error),
}

impl std::fmt::Display for BindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRunning => f.write_str("subtext is already running"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for BindError {}

impl From<io::Error> for BindError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

static LISTENER: OnceLock<Mutex<Option<UnixListener>>> = OnceLock::new();

pub fn socket_path() -> PathBuf {
    runtime_dir().join("subtext.sock")
}

pub fn bind_default() -> Result<UnixListener, BindError> {
    bind_at(&socket_path())
}

pub fn install(listener: UnixListener) {
    let _ = LISTENER.set(Mutex::new(Some(listener)));
}

pub fn send(command: Command) -> io::Result<()> {
    send_to(&socket_path(), command)
}

pub fn subscription() -> Subscription<Command> {
    Subscription::run(listen)
}

fn listen() -> impl Stream<Item = Command> + Send + 'static {
    iced::stream::channel(32, async |sender| {
        let Some(listener) = take_listener() else {
            std::future::pending::<()>().await;
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || serve(listener, tx));
        thread::spawn(move || forward(sender, rx));
        std::future::pending::<()>().await;
    })
}

fn take_listener() -> Option<UnixListener> {
    LISTENER.get()?.lock().ok()?.take()
}

fn serve(listener: UnixListener, tx: std::sync::mpsc::Sender<Command>) {
    for incoming in listener.incoming() {
        let Ok(stream) = incoming else {
            continue;
        };
        let mut line = String::new();
        if BufReader::new(stream).read_line(&mut line).is_err() {
            continue;
        }
        let Some(command) = Command::parse(&line) else {
            continue;
        };
        if tx.send(command).is_err() {
            break;
        }
    }
}

fn forward(
    mut sender: iced::futures::channel::mpsc::Sender<Command>,
    rx: std::sync::mpsc::Receiver<Command>,
) {
    while let Ok(mut command) = rx.recv() {
        loop {
            match sender.try_send(command) {
                Ok(()) => break,
                Err(error) if error.is_full() => {
                    command = error.into_inner();
                    thread::sleep(Duration::from_millis(5));
                }
                Err(_) => return,
            }
        }
    }
}

fn bind_at(path: &Path) -> Result<UnixListener, BindError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if path.exists() {
        match UnixStream::connect(path) {
            Ok(stream) => {
                drop(stream);
                return Err(BindError::AlreadyRunning);
            }
            Err(_) => {
                let _ = fs::remove_file(path);
            }
        }
    }
    let listener = UnixListener::bind(path)?;
    if let Ok(meta) = fs::metadata(path) {
        let mut permissions = meta.permissions();
        permissions.set_mode(0o600);
        let _ = fs::set_permissions(path, permissions);
    }
    Ok(listener)
}

fn send_to(path: &Path, command: Command) -> io::Result<()> {
    let mut stream = UnixStream::connect(path)?;
    writeln!(stream, "{command}")?;
    stream.flush()?;
    Ok(())
}

fn runtime_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(dir);
    }
    let dir = std::env::temp_dir().join(format!("subtext-{}", uid()));
    if let Err(error) = prepare_private_dir(&dir) {
        eprintln!("subtext: runtime dir {}: {error}", dir.display());
    }
    dir
}

/// The fallback under `/tmp` is not created by the session, so it does not
/// inherit the privacy of `XDG_RUNTIME_DIR`. Keep it owner-only.
fn prepare_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let meta = fs::symlink_metadata(dir)?;
    if meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "runtime directory is a symlink",
        ));
    }
    let mut permissions = meta.permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(dir, permissions)
}

fn uid() -> u32 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let rest = line.strip_prefix("Uid:")?;
                rest.split_whitespace().next()?.parse().ok()
            })
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_three_commands() {
        assert_eq!(Command::parse("toggle\n"), Some(Command::Toggle));
        assert_eq!(Command::parse("  show"), Some(Command::Show));
        assert_eq!(Command::parse("hide"), Some(Command::Hide));
        assert_eq!(Command::parse("quit"), None);
    }

    #[test]
    fn command_round_trips_over_a_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subtext.sock");
        let listener = bind_at(&path).unwrap();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(stream).read_line(&mut line).unwrap();
            Command::parse(&line).unwrap()
        });
        send_to(&path, Command::Show).unwrap();
        assert_eq!(server.join().unwrap(), Command::Show);
    }

    #[test]
    fn the_fallback_runtime_dir_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subtext-user");
        fs::create_dir(&path).unwrap();
        let mut loose = fs::metadata(&path).unwrap().permissions();
        loose.set_mode(0o755);
        fs::set_permissions(&path, loose).unwrap();
        prepare_private_dir(&path).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn second_bind_fails_while_the_socket_is_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subtext.sock");
        let _listener = bind_at(&path).unwrap();
        assert!(matches!(bind_at(&path), Err(BindError::AlreadyRunning)));
    }
}
