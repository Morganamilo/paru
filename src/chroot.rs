use crate::config::Config;
use crate::exec;
use anyhow::Result;
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
pub struct Chroot {
    pub path: PathBuf,
    pub pacman_conf: String,
    pub makepkg_conf: String,
    pub mflags: Vec<String>,
    pub ro: Vec<String>,
    pub rw: Vec<String>,
}

fn pacman_conf(pacman_conf: &str) -> Result<tempfile::NamedTempFile> {
    let mut tmp = tempfile::NamedTempFile::new()?;
    let conf = pacmanconf::Config::expand_from_file(pacman_conf)?;

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

        let mut cmd = Command::new("mkarchroot");
        cmd.arg("-C")
            .arg(tmp.path())
            .arg("-M")
            .arg(&self.makepkg_conf)
            .arg(dir)
            .args(pkgs);

        exec::command(&mut cmd)?;
        Ok(())
    }

    pub fn run<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<()> {
        let dir = self.path.join("root");
        let tmp = pacman_conf(&self.pacman_conf)?;

        let mut cmd = Command::new("arch-nspawn");
        cmd.arg("-C")
            .arg(tmp.path())
            .arg("-M")
            .arg(&self.makepkg_conf)
            .arg(dir);

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
        self.run(&["pacman", "-Syu", "--noconfirm"])
    }

    pub fn build(&self, pkgbuild: &Path, chroot_flags: &[&str], flags: &[&str]) -> Result<()> {
        let mut cmd = Command::new("makechrootpkg");

        cmd.current_dir(pkgbuild)
            .args(chroot_flags)
            .arg("-r")
            .arg(&self.path);

        for file in &self.ro {
            cmd.arg("-D").arg(file);
        }

        for file in &self.rw {
            cmd.arg("-d").arg(file);
        }

        cmd.arg("--").args(flags).args(&self.mflags);

        exec::command(&mut cmd)?;
        Ok(())
    }
}
