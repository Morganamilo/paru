use crate::config::Config;
use crate::exec;
use anyhow::{Context, Result};
use nix::unistd::{Uid, User};
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
pub struct Chroot {
    pub sudo: String,
    pub path: PathBuf,
    pub pacman_conf: String,
    pub makepkg_conf: String,
    pub mflags: Vec<String>,
    pub ro: Vec<String>,
    pub rw: Vec<String>,
    pub extra_pkgs: Vec<String>,
}

fn pacman_conf(pacman_conf: &str) -> Result<tempfile::NamedTempFile> {
    let mut tmp = tempfile::NamedTempFile::new()?;
    let conf = pacmanconf::Config::expand_with_opts(None, Some(pacman_conf), Some("/"))?;

    // Bug with dbpath in pacstrap
    let conf = conf
        .lines()
        .filter(|l| !l.starts_with("DBPath"))
        .collect::<Vec<_>>()
        .join("\n");

    tmp.as_file_mut().write_all(conf.as_bytes())?;
    tmp.flush()?;
    Ok(tmp)
}

impl Chroot {
    pub fn exists(&self) -> bool {
        self.path.join("root").exists()
    }

    pub fn create<S: AsRef<OsStr>>(&self, config: &Config, pkgs: &[S]) -> Result<()> {
        let mut cmd = Command::new(&config.sudo_bin);
        cmd.arg("install").arg("-dm755").arg(&self.path);
        exec::command(&mut cmd)?;

        let tmp = pacman_conf(&self.pacman_conf)?;
        let dir = self.path.join("root");

        let mut cmd = Command::new(&self.sudo);
        cmd.arg("mkarchroot")
            .arg("-C")
            .arg(tmp.path())
            .arg("-M")
            .arg(&self.makepkg_conf)
            .arg(dir)
            .args(pkgs);

        exec::command(&mut cmd)?;
        Ok(())
    }

    pub fn run<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<()> {
        self.run_as(true, args)
    }
    pub fn run_usr<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<()> {
        self.run_as(false, args)
    }

    fn run_as<S: AsRef<OsStr>>(&self, root: bool, args: &[S]) -> Result<()> {
        let dir = if root {
            self.path.join("root")
        } else {
            let user = User::from_uid(Uid::current())
                .context("failed to get username")?
                .context("failed to get username")?;
            self.path.join(&user.name)
        };
        let tmp = pacman_conf(&self.pacman_conf)?;

        let mut cmd = Command::new(&self.sudo);
        cmd.arg("arch-nspawn")
            .arg("-C")
            .arg(tmp.path())
            .arg("-M")
            .arg(&self.makepkg_conf)
            .arg(dir);

        if Path::new(&format!("{}.d", self.makepkg_conf)).exists() {
            cmd.arg("--bind-ro");
            cmd.arg(format!("{}.d:/etc/makepkg.conf.d", self.makepkg_conf));
        }

        let xdg_config = std::env::var_os("XDG_CONFIG_HOME")
            .map(|p| PathBuf::from(p).join("pacman/makepkg.conf"));

        if let Some(xdg_config) = xdg_config {
            if xdg_config.exists() {
                cmd.arg("--bind-ro");
                cmd.arg(format!(
                    "{}:/etc/makepkg.conf.d/xdg.conf",
                    xdg_config.display()
                ));
            }
        }

        for file in &self.ro {
            cmd.arg("--bind-ro");
            cmd.arg(file);
        }

        for file in &self.rw {
            cmd.arg("--bind");
            cmd.arg(file);
        }

        cmd.args(args);

        exec::command(&mut cmd)?;
        Ok(())
    }

    pub fn update(&self) -> Result<()> {
        let conf = pacmanconf::Config::with_opts(None, Some(self.pacman_conf.as_str()), Some("/"))?;
        let db = Path::new(&conf.db_path).join("sync");
        let dir = self.path.join("root");
        let mut cmd = Command::new(&self.sudo);
        cmd.arg("cp")
            .arg("-auT")
            .arg(&db)
            .arg(dir.join(db.strip_prefix("/")?));
        let _ = exec::command(&mut cmd);

        self.run(&["pacman", "-Syu", "--noconfirm"])
    }

    pub fn build<S: AsRef<OsStr>>(
        &self,
        pkgbuild: &Path,
        pkgs: &[&str],
        chroot_flags: &[S],
        flags: &[&str],
        env: &[(String, String)],
    ) -> Result<()> {
        let mut cmd = Command::new("makechrootpkg");

        cmd.current_dir(pkgbuild)
            .args(chroot_flags)
            .arg("-r")
            .arg(&self.path);

        for pkg in pkgs {
            cmd.arg("-I").arg(pkg);
        }

        for file in &self.ro {
            cmd.arg("-D").arg(file);
        }

        for file in &self.rw {
            cmd.arg("-d").arg(file);
        }

        cmd.arg("--").args(flags).args(&self.mflags);

        for (key, value) in env {
            if key == "PKGDEST" {
                cmd.env(key, value);
            } else {
                cmd.arg(format!("{}={}", key, value));
            }
        }

        exec::command(&mut cmd)?;
        Ok(())
    }
}
