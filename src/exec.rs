use crate::args::Args;
use crate::config::Config;

use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::path::Path;
use std::process::{Command, Output};
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};

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
    let ret = Command::new(sudo).args(flags).status().with_context(|| {
        let flags = flags
            .iter()
            .map(|s| s.as_ref().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        format!("failed to run: {} {}", sudo, flags)
    })?;
    Status(ret.code().unwrap_or(1)).success()?;
    Ok(())
}

fn wait_for_lock(config: &Config) {
    let path = Path::new(config.alpm.dbpath()).join("db.lck");
    let c = config.color;
    if path.exists() {
        println!(
            "{} {}",
            c.error.paint("::"),
            c.bold.paint("Pacman is currently in use, please wait...")
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
    let mut command: std::process::Command;
    let error_msg: String;

    if config.need_root {
        wait_for_lock(config);

        command = Command::new(&config.sudo_bin);
        command.args(&config.sudo_flags);
        command.arg(args.bin.as_ref());
        let mut sudo_command = config.sudo_flags.clone();
        sudo_command.insert(0, config.sudo_bin.clone());
        error_msg = format!(
            "failed to run: {} {} {}",
            sudo_command.join(" "),
            args.bin,
            args.args().join(" ")
        );
    } else {
        command = Command::new(args.bin.as_ref());
        error_msg = format!("failed to run: {} {}", args.bin, args.args().join(" "));
    }

    let ret = command
        .args(args.args())
        .status()
        .with_context(|| error_msg)?;
    Ok(Status(ret.code().unwrap_or(1)))
}

pub fn pacman_output<S: AsRef<str> + Display>(config: &Config, args: &Args<S>) -> Result<Output> {
    let mut command = if config.need_root {
        let mut command = Command::new(&config.sudo_bin);
        command.args(&config.sudo_flags);
        command.arg(args.bin.as_ref());
        command
    } else {
        Command::new(args.bin.as_ref())
    };

    let output = command
        .args(args.args())
        .output()
        .with_context(|| format!("failed to run pacman '{}'", args.bin))?;
    Ok(output)
}

pub fn makepkg<S: AsRef<OsStr>>(config: &Config, dir: &Path, args: &[S]) -> Result<Status> {
    let ret = Command::new(&config.makepkg_bin)
        .current_dir(dir)
        .args(&config.mflags)
        .args(args)
        .status()
        .with_context(|| {
            format!(
                "failed to run: {} {} {}",
                config.makepkg_bin,
                config.mflags.join(" "),
                args.iter()
                    .map(|a| a.as_ref().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        })?;

    Ok(Status(ret.code().unwrap_or(1)))
}

pub fn command<C: AsRef<OsStr>, S: AsRef<OsStr>, P: AsRef<Path>>(
    cmd: C,
    dir: P,
    args: &[S],
) -> Result<Status> {
    let ret = Command::new(cmd.as_ref())
        .current_dir(dir)
        .args(args)
        .status()
        .with_context(|| {
            format!(
                "failed to run: {} {}",
                cmd.as_ref().to_string_lossy(),
                args.iter()
                    .map(|a| a.as_ref().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        })?;

    Ok(Status(ret.code().unwrap_or(1)))
}

pub fn makepkg_output<S: AsRef<OsStr>>(config: &Config, dir: &Path, args: &[S]) -> Result<Output> {
    let ret = Command::new(&config.makepkg_bin)
        .current_dir(dir)
        .args(&config.mflags)
        .args(args)
        .output()
        .with_context(|| {
            format!(
                "failed to run: {} {} {}",
                config.makepkg_bin,
                config.mflags.join(" "),
                args.iter()
                    .map(|a| a.as_ref().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        })?;

    if !ret.status.success() {
        bail!(
            "failed to run: {} {} --verifysource -Ccf: {}",
            config.makepkg_bin,
            config.mflags.join(" "),
            String::from_utf8_lossy(&ret.stderr)
        )
    }

    Ok(ret)
}
