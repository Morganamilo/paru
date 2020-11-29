use crate::config::Config;
use crate::exec;
use anyhow::Result;
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Chroot {
    pub path: PathBuf,
    pub pacman_conf: String,
    pub makepkg_conf: String,
    pub ro: Vec<String>,
    pub rw: Vec<String>,
}

fn pacman_conf(pacman_conf: &str) -> Result<tempfile::NamedTempFile> {
    let mut tmp = tempfile::NamedTempFile::new()?;
    let conf = pacmanconf::Config::expand_from_file(pacman_conf)?;

    // Bug with dbpath in pacstrap
    let conf = conf
        .lines()
        .filter(|l| !l.starts_with("DbPath"))
        .collect::<String>();

    tmp.as_file_mut().write_all(conf.as_bytes())?;
    tmp.flush()?;
    Ok(tmp)
}

impl Chroot {
    pub fn exists(&self) -> bool {
        self.path.join("root").exists()
    }

    pub fn create<S: AsRef<OsStr>>(&self, config: &Config, pkgs: &[S]) -> Result<()> {
        let args = &[
            OsStr::new("install"),
            OsStr::new("-dm755"),
            self.path.as_os_str(),
        ];
        exec::command(&config.sudo_bin, ".", args)?.success()?;

        let tmp = pacman_conf(&self.pacman_conf)?;
        let dir = self.path.join("root");

        let mut args = vec![
            OsStr::new("-C"),
            tmp.path().as_os_str(),
            OsStr::new("-M"),
            OsStr::new(&self.makepkg_conf),
            dir.as_os_str(),
        ];
        args.extend(pkgs.iter().map(|p| p.as_ref()));
        exec::command("mkarchroot", ".", &args)?.success()?;
        Ok(())
    }

    pub fn run<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<()> {
        let dir = self.path.join("root");
        let tmp = pacman_conf(&self.pacman_conf)?;

        let mut a = vec![
            OsStr::new("-C"),
            tmp.path().as_os_str(),
            OsStr::new("-M"),
            OsStr::new(&self.makepkg_conf),
            dir.as_os_str(),
        ];

        for file in &self.ro {
            a.push(OsStr::new("--bind-ro"));
            a.push(OsStr::new(file));
        }

        for file in &self.rw {
            a.push(OsStr::new("--bind"));
            a.push(OsStr::new(file));
        }

        a.extend(args.iter().map(|p| p.as_ref()));
        exec::command("arch-nspawn", ".", &a)?.success()?;
        Ok(())
    }

    pub fn update(&self) -> Result<()> {
        self.run(&["pacman", "-Syu", "--noconfirm"])
    }

    pub fn build(&self, pkgbuild: &Path, chroot_flags: &[&str], flags: &[&str]) -> Result<()> {
        let mut args = chroot_flags
            .iter()
            .map(|f| OsStr::new(f))
            .collect::<Vec<_>>();
        args.push(OsStr::new("-r"));
        args.push(OsStr::new(self.path.as_os_str()));

        for file in &self.ro {
            args.push(OsStr::new("-D"));
            args.push(OsStr::new(file));
        }

        for file in &self.rw {
            args.push(OsStr::new("-d"));
            args.push(OsStr::new(file));
        }

        args.push(OsStr::new("--"));

        for flag in flags {
            args.push(OsStr::new(flag));
        }

        exec::command("makechrootpkg", pkgbuild, &args)?.success()?;
        Ok(())
    }
}

/*pub fn chroot(globals: clap::Globals, ch: clap::Chroot) -> Result<()> {
    let pacman = pacmanconf::Config::from_file(&globals.pacman_conf)?;

    let chroot = Chroot {
        path: ch.chroot,
        pacman_conf: globals.pacman_conf.clone(),
        makepkg_conf: globals.makepkg_conf.clone(),
        ro: repo::all_files(&pacman)
            .iter()
            .map(|s| s.to_string())
            .collect(),
        rw: pacman.cache_dir.clone(),
    };

    println!("{:?}", chroot);

    if !chroot.path.exists() && !ch.create {
        bail!("chroot does not exist: use 'chroot -c' to create it");
    }

    if ch.create {
        if ch.targets.is_empty() {
            chroot.create(&["base-devel"])?;
        } else {
            chroot.create(&ch.targets)?;
        }
    } else if ch.upgrade {
        chroot.update()?;
    } else if ch.interactive {
        let targs: &[&str] = &[];
        chroot.run(targs)?;
    } else if ch.sync {
        let mut cmd = vec!["pacman", "-S", "--ask=255"];
        cmd.extend(ch.targets.iter().map(|s| s.as_str()));
        chroot.run(&cmd)?;
    } else {
        chroot.run(&ch.targets)?;
    }

    Ok(())
}*/
