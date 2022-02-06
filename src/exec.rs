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

pub static RAISE_SIGPIPE: Lazy<Arc<AtomicBool>> = Lazy::new(|| {
    let arc = Arc::new(AtomicBool::new(true));
    signal_flag::register_conditional_default(SIGPIPE, Arc::clone(&arc)).unwrap();
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

fn command_err(cmd: &Command) -> String {
    format!(
        "{} {} {}",
        tr!("failed to run:"),
        cmd.get_program().to_string_lossy(),
        cmd.get_args()
            .map(|a| a.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    )
}

fn command_status(cmd: &mut Command) -> Result<Status> {
    let term = &*CAUGHT_SIGNAL;

    DEFAULT_SIGNALS.store(false, Ordering::Relaxed);

    let ret = cmd
        .status()
        .map(|s| Status(s.code().unwrap_or(1)))
        .with_context(|| command_err(cmd));

    DEFAULT_SIGNALS.store(true, Ordering::Relaxed);

    match term.swap(0, Ordering::Relaxed) {
        0 => ret,
        n => std::process::exit(128 + n as i32),
    }
}

pub fn command(cmd: &mut Command) -> Result<()> {
    command_status(cmd)?
        .success()
        .with_context(|| command_err(cmd))?;
    Ok(())
}

pub fn command_output(cmd: &mut Command) -> Result<Output> {
    let term = &*CAUGHT_SIGNAL;

    DEFAULT_SIGNALS.store(false, Ordering::Relaxed);

    let ret = cmd.output().with_context(|| command_err(cmd));

    DEFAULT_SIGNALS.store(true, Ordering::Relaxed);
    let ret = match term.swap(0, Ordering::Relaxed) {
        0 => ret?,
        n => std::process::exit(128 + n as i32),
    };

    if !ret.status.success() {
        bail!(command_err(cmd));
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
    let mut cmd = Command::new(sudo);
    cmd.args(flags);
    command_status(&mut cmd)?;
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
        let mut cmd = Command::new(&config.sudo_bin);
        cmd.args(&config.sudo_flags)
            .arg(args.bin.as_ref())
            .args(args.args());
        command_status(&mut cmd)
    } else {
        let mut cmd = Command::new(args.bin.as_ref());
        cmd.args(args.args());
        command_status(&mut cmd)
    }
}

pub fn pacman_output<S: AsRef<str> + Display + std::fmt::Debug>(
    config: &Config,
    args: &Args<S>,
) -> Result<Output> {
    if config.need_root {
        wait_for_lock(config);
        let mut cmd = Command::new(&config.sudo_bin);
        cmd.args(&config.sudo_flags)
            .arg(args.bin.as_ref())
            .args(args.args());
        command_output(&mut cmd)
    } else {
        let mut cmd = Command::new(args.bin.as_ref());
        cmd.args(args.args());
        command_output(&mut cmd)
    }
}

pub fn makepkg<S: AsRef<OsStr>>(config: &Config, dir: &Path, args: &[S]) -> Result<Status> {
    let mut cmd = Command::new(&config.makepkg_bin);
    cmd.args(&config.mflags).args(args).current_dir(dir);
    command_status(&mut cmd)
}

pub fn makepkg_output<S: AsRef<OsStr>>(config: &Config, dir: &Path, args: &[S]) -> Result<Output> {
    let mut cmd = Command::new(&config.makepkg_bin);
    cmd.args(&config.mflags).args(args).current_dir(dir);
    command_output(&mut cmd)
}
