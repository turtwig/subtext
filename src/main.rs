//! Subtext is a resident drawer. With no arguments it stays running and
//! hidden. `toggle`, `show`, and `hide` talk to that process over a socket,
//! starting it first when nothing is listening.

mod app;
mod blur;
mod editor;
mod fade;
mod geom;
mod glibc_compat;
mod ipc;
mod look;
mod message;
mod slide;
mod state;
mod ui;

use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn main() -> ExitCode {
    match dispatch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}

fn dispatch() -> Result<(), ExitCode> {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    if args.next().is_some() {
        usage();
        return Err(ExitCode::from(2));
    }
    match command.as_deref() {
        None | Some("--daemon") => run_daemon(),
        Some("toggle") => client(ipc::Command::Toggle),
        Some("show") => client(ipc::Command::Show),
        Some("hide") => client(ipc::Command::Hide),
        Some("-h") | Some("--help") => {
            usage();
            Ok(())
        }
        Some(other) => {
            eprintln!("subtext: unknown command `{other}`");
            usage();
            Err(ExitCode::from(2))
        }
    }
}

fn run_daemon() -> Result<(), ExitCode> {
    match ipc::bind_default() {
        Ok(listener) => ipc::install(listener),
        Err(ipc::BindError::AlreadyRunning) => {
            eprintln!("subtext is already running");
            return Err(ExitCode::from(1));
        }
        Err(error) => {
            eprintln!("subtext: {error}");
            return Err(ExitCode::from(1));
        }
    }
    if let Err(error) = app::run() {
        eprintln!("subtext: {error}");
        return Err(ExitCode::from(1));
    }
    Ok(())
}

fn client(command: ipc::Command) -> Result<(), ExitCode> {
    if ipc::send(command).is_ok() {
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|error| {
        eprintln!("subtext: {error}");
        ExitCode::from(1)
    })?;
    let _child = Command::new(exe)
        .arg("--daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| {
            eprintln!("subtext: could not start the drawer: {error}");
            ExitCode::from(1)
        })?;
    for _ in 0..150 {
        thread::sleep(Duration::from_millis(20));
        if ipc::send(command).is_ok() {
            return Ok(());
        }
    }
    eprintln!("subtext: the drawer did not open a socket in time");
    Err(ExitCode::from(1))
}

fn usage() {
    let _ = writeln!(
        io::stdout(),
        "\
subtext — a pull-up scratchpad for Wayland

Usage:
  subtext              start the drawer and wait for a hotkey
  subtext toggle       show or hide it
  subtext show
  subtext hide

Hyprland:
  exec-once = subtext
  bind = SUPER, GRAVE, exec, subtext toggle

Sway:
  exec subtext
  bindsym $mod+grave exec subtext toggle

The drawer anchors to the bottom, centered, and sizes in whole characters.
State is kept in $XDG_CONFIG_HOME/subtext."
    );
}

#[derive(Debug)]
struct ExitCode(u8);

impl ExitCode {
    const SUCCESS: Self = Self(0);
    fn from(code: u8) -> Self {
        Self(code)
    }
}

impl std::process::Termination for ExitCode {
    fn report(self) -> std::process::ExitCode {
        std::process::ExitCode::from(self.0)
    }
}
