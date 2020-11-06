use crate::config::Config;

use crate::exec;
use crate::print_error;
use crate::util::ask;

use std::fs::read_dir;
use std::fs::{remove_dir_all, remove_file};

use std::path::Path;
use std::process::Command;

use alpm_utils::DbListExt;
use anyhow::{bail, Context, Error, Result};

use srcinfo::Srcinfo;

pub fn clean(config: &Config) -> Result<()> {
    if config.mode != "aur" {
        exec::pacman(config, &config.args)?;
    }

    if config.mode != "repo" {
        let remove_all = config.clean > 1;
        let clean_method = &config.pacman.clean_method;
        let keep_installed = clean_method.iter().any(|a| a == "KeepInstalled");
        let keep_current = clean_method.iter().any(|a| a == "KeepCurrent");

        let question = if remove_all {
            "Do you want to remove ALL AUR packages from cache?"
        } else {
            "Do you want to remove all other AUR packages from cache?"
        };

        if config.mode == "any" {
            println!();
        }

        println!("Clone Directory: {}", config.fetch.clone_dir.display());

        if ask(config, question, !remove_all) {
            clean_aur(config, keep_installed, keep_current, remove_all)?;
        }

        println!("\nDiff Directory: {}", config.fetch.diff_dir.display());

        let question = "Do you want to remove all saved diffs?";
        if ask(config, question, true) {
            clean_diff(config)?;
        }
    }

    Ok(())
}

fn clean_diff(config: &Config) -> Result<()> {
    if !config.fetch.diff_dir.exists() {
        return Ok(());
    }

    let diffs = read_dir(&config.fetch.diff_dir)
        .with_context(|| format!("can't open diff dir: {}", config.fetch.diff_dir.display()))?;

    for diff in diffs {
        let diff = diff?;

        if !diff.file_type()?.is_dir() && diff.path().extension().map(|s| s == "diff") == Some(true)
        {
            remove_file(diff.path())?
        }
    }

    Ok(())
}

fn clean_aur(
    config: &Config,
    keep_installed: bool,
    keep_current: bool,
    remove_all: bool,
) -> Result<()> {
    if !config.fetch.clone_dir.exists() {
        return Ok(());
    }

    let cached_pkgs = read_dir(&config.fetch.clone_dir)
        .with_context(|| format!("can't open clone dir: {}", config.fetch.clone_dir.display()))?;

    let mut srcinfos = Vec::new();

    for file in cached_pkgs {
        let file = file?;

        if !file.file_type()?.is_dir()
            || !file.path().join(".git").exists()
            || !file.path().join(".SRCINFO").exists()
        {
            continue;
        }

        match Srcinfo::parse_file(file.path().join(".SRCINFO")) {
            Ok(srcinfo) => srcinfos.push(srcinfo),
            Err(err) => print_error(config.color.error, Error::new(err)),
        }
    }

    'outer: for srcinfo in srcinfos {
        if remove_all {
            let path = config.fetch.clone_dir.join(srcinfo.base.pkgbase);
            remove_dir_all(&path)
                .with_context(|| format!("could not remove '{}'", path.display()))?;
            continue;
        }

        if keep_installed {
            let local_db = config.alpm.localdb();
            for pkg in &srcinfo.pkgs {
                if let Ok(pkg) = local_db.pkg(&*pkg.pkgname) {
                    if pkg.version().as_ref() == srcinfo.version() {
                        continue 'outer;
                    }
                }
            }
        }

        if keep_current {
            for pkg in &srcinfo.pkgs {
                let sync_dbs = config.alpm.syncdbs();
                if let Ok(pkg) = sync_dbs.pkg(&*pkg.pkgname) {
                    if pkg.version().as_ref() == srcinfo.version() {
                        continue 'outer;
                    }
                }
            }
        }

        clean_untracked(config, &config.fetch.clone_dir.join(srcinfo.base.pkgbase))?;
    }

    Ok(())
}

pub fn clean_untracked(config: &Config, path: &Path) -> Result<()> {
    let output = Command::new(&config.git_bin)
        .args(&config.git_flags)
        .current_dir(path)
        .args(&["reset", "--hard", "HEAD"])
        .output()
        .with_context(|| {
            format!(
                "{} {} reset --hard HEAD",
                config.git_bin,
                config.git_flags.join(" "),
            )
        })?;

    if !output.status.success() {
        bail!(
            "{} {} reset --hard HEAD: {}",
            config.git_bin,
            config.git_flags.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    let output = Command::new(&config.git_bin)
        .args(&config.git_flags)
        .current_dir(path)
        .arg("clean")
        .arg("-fx")
        .output()
        .with_context(|| {
            format!(
                "{} {} clean -fx",
                config.git_bin,
                config.git_flags.join(" "),
            )
        })?;

    if !output.status.success() {
        bail!(
            "{} {} clean -fx: {}",
            config.git_bin,
            config.git_flags.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    Ok(())
}
