use crate::args::Args;
use crate::config::Config;

use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::path::Path;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use once_cell::sync::Lazy;
use signal_hook::consts::signal::*;
use signal_hook::flag as signal_flag;
use tr::tr;

pub static DEFAULT_SIGNALS: Lazy<Arc<AtomicBool>> = Lazy::new(|| {
    let arc = Arc::new(AtomicBool::new(true));
    signal_flag::register_conditional_default(SIGTERM, Arc::clone(&arc)).unwrap();
    signal_flag::register_conditional_default(SIGINT, Arc::clone(&arc)).unwrap();
    signal_flag::register_conditional_default(SIGQUIT, Arc::clone(&arc)).unwrap();
    arc
});

static CAUGHT_SIGNAL: Lazy<Arc<AtomicUsize>> = Lazy::new(|| {
    let arc = Arc::new(AtomicUsize::new(0));
    signal_flag::register_usize(SIGTERM, Arc::clone(&arc), SIGTERM as usize).unwrap();
    signal_flag::register_usize(SIGINT, Arc::clone(&arc), SIGINT as usize).unwrap();
    signal_flag::register_usize(SIGQUIT, Arc::clone(&arc), SIGQUIT as usize).unwrap();
    arc
});

#[derive(Debug, Clone)]
pub struct PacmanError {
    pub msg: String,
}

impl Display for PacmanError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for PacmanError {}

#[derive(Debug, Clone, Copy)]
pub struct Status(pub i32);

impl Display for Status {
    fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
}

impl std::error::Error for Status {}

impl Status {
    pub fn code(self) -> i32 {
        self.0
    }

    pub fn success(self) -> Result<i32, Status> {
        if self.0 == 0 {
            Ok(0)
        } else {
            Err(self)
        }
    }
}

pub fn command_err<C: AsRef<OsStr>, S: AsRef<OsStr>>(cmd: C, args: &[S]) -> String {
    format!(
        "{} {} {}",
        tr!("failed to run:"),
        cmd.as_ref().to_string_lossy(),
        args.iter()
            .map(|a| a.as_ref().to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    )
}

fn command_status<C: AsRef<OsStr>, S: AsRef<OsStr>, P: AsRef<Path>>(
    cmd: C,
    dir: P,
    args: &[S],
) -> Result<Status> {
    let term = &*CAUGHT_SIGNAL;

    DEFAULT_SIGNALS.store(false, Ordering::Relaxed);

    let ret = Command::new(cmd.as_ref())
        .current_dir(dir)
        .args(args)
        .status()
        .map(|s| Status(s.code().unwrap_or(1)))
        .with_context(|| command_err(cmd.as_ref(), args.as_ref()));

    DEFAULT_SIGNALS.store(true, Ordering::Relaxed);

    match term.swap(0, Ordering::Relaxed) {
        0 => ret,
        n => std::process::exit(128 + n as i32),
    }
}

pub fn command<C: AsRef<OsStr>, S: AsRef<OsStr>, P: AsRef<Path>>(
    cmd: C,
    dir: P,
    args: &[S],
) -> Result<()> {
    command_status(cmd.as_ref(), dir, args)?
        .success()
        .with_context(|| command_err(cmd, args))?;
    Ok(())
}

pub fn command_output<C: AsRef<OsStr>, S: AsRef<OsStr>, P: AsRef<Path>>(
    cmd: C,
    dir: P,
    args: &[S],
) -> Result<Output> {
    let term = &*CAUGHT_SIGNAL;

    DEFAULT_SIGNALS.store(false, Ordering::Relaxed);

    let ret = Command::new(cmd.as_ref())
        .current_dir(dir)
        .args(args)
        .output()
        .with_context(|| command_err(cmd.as_ref(), args.as_ref()));

    DEFAULT_SIGNALS.store(true, Ordering::Relaxed);
    let ret = match term.swap(0, Ordering::Relaxed) {
        0 => ret?,
        n => std::process::exit(128 + n as i32),
    };

    if !ret.status.success() {
        bail!(command_err(cmd, args));
    }

    Ok(ret)
}

pub fn spawn_sudo(sudo: String, flags: Vec<String>) -> Result<()> {
    update_sudo(&sudo, &flags)?;
    thread::spawn(move || sudo_loop(&sudo, &flags));
    Ok(())
}

fn sudo_loop<S: AsRef<OsStr>>(sudo: &str, flags: &[S]) -> Result<()> {
    loop {
        update_sudo(sudo, flags)?;
        thread::sleep(Duration::from_secs(250));
    }
}

fn update_sudo<S: AsRef<OsStr>>(sudo: &str, flags: &[S]) -> Result<()> {
    command_status(sudo, ".", flags)?;
    Ok(())
}

fn wait_for_lock(config: &Config) {
    let path = Path::new(config.alpm.dbpath()).join("db.lck");
    let c = config.color;
    if path.exists() {
        println!(
            "{} {}",
            c.error.paint("::"),
            c.bold
                .paint(tr!("Pacman is currently in use, please wait..."))
        );

        std::thread::sleep(Duration::from_secs(3));
        while path.exists() {
            std::thread::sleep(Duration::from_secs(3));
        }
    }
}

pub fn pacman<S: AsRef<str> + Display + std::fmt::Debug>(
    config: &Config,
    args: &Args<S>,
) -> Result<Status> {
    if config.need_root {
        wait_for_lock(config);
        let mut cmd_args = config
            .sudo_flags
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>();
        cmd_args.push(args.bin.as_ref());
        let args = args.args();
        cmd_args.extend(args.iter().map(|s| s.as_str()));
        command_status(&config.sudo_bin, ".", &cmd_args)
    } else {
        command_status(args.bin.as_ref(), ".", &args.args())
    }
}

pub fn pacman_output<S: AsRef<str> + Display + std::fmt::Debug>(
    config: &Config,
    args: &Args<S>,
) -> Result<Output> {
    if config.need_root {
        wait_for_lock(config);
        let mut cmd_args = config
            .sudo_flags
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>();
        cmd_args.push(args.bin.as_ref());
        let args = args.args();
        cmd_args.extend(args.iter().map(|s| s.as_str()));
        command_output(&config.sudo_bin, ".", &cmd_args)
    } else {
        command_output(args.bin.as_ref(), ".", &args.args())
    }
}

pub fn makepkg<S: AsRef<OsStr>>(config: &Config, dir: &Path, args: &[S]) -> Result<Status> {
    let mut cmd_args = config.mflags.iter().map(|s| s.as_ref()).collect::<Vec<_>>();
    cmd_args.extend(args.iter().map(|s| s.as_ref()));
    command_status(&config.makepkg_bin, dir, &cmd_args)
}

pub fn makepkg_output<S: AsRef<OsStr>>(config: &Config, dir: &Path, args: &[S]) -> Result<Output> {
    let mut cmd_args = config.mflags.iter().map(|s| s.as_ref()).collect::<Vec<_>>();
    cmd_args.extend(args.iter().map(|s| s.as_ref()));
    command_output(&config.makepkg_bin, dir, &cmd_args)
}
