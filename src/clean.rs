use crate::config::{Config, Mode};

use crate::exec;
use crate::print_error;
use crate::printtr;
use crate::util::ask;

use std::fs::{read_dir, remove_dir_all, remove_file, set_permissions, DirEntry};

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use alpm_utils::DbListExt;
use anyhow::{bail, Context, Result};
use srcinfo::Srcinfo;
use tr::tr;

pub fn clean(config: &Config) -> Result<()> {
    if config.mode != Mode::Aur {
        exec::pacman(config, &config.args)?;
    }

    if config.mode != Mode::Repo {
        let rm = config.delete >= 1;
        let remove_all = config.clean >= 2;
        let clean_method = &config.pacman.clean_method;
        let keep_installed = clean_method.iter().any(|a| a == "KeepInstalled");
        let keep_current = clean_method.iter().any(|a| a == "KeepCurrent");

        let question = if remove_all {
            tr!("Do you want to clean ALL AUR packages from cache?")
        } else {
            tr!("Do you want to clean all other AUR packages from cache?")
        };

        if config.mode == Mode::Any {
            println!();
        }

        printtr!("Clone Directory: {}", config.fetch.clone_dir.display());

        if ask(config, &question, !remove_all) {
            clean_aur(config, keep_installed, keep_current, remove_all, rm)?;
        }

        printtr!("\nDiff Directory: {}", config.fetch.diff_dir.display());

        let question = tr!("Do you want to remove all saved diffs?");
        if ask(config, &question, true) {
            clean_diff(config)?;
        }
    }

    Ok(())
}

fn clean_diff(config: &Config) -> Result<()> {
    if !config.fetch.diff_dir.exists() {
        return Ok(());
    }

    let diffs = read_dir(&config.fetch.diff_dir).with_context(|| {
        tr!(
            "can't open diff dir: {}",
            config.fetch.diff_dir.display().to_string()
        )
    })?;

    for diff in diffs {
        let diff = diff?;

        if !diff.file_type()?.is_dir() && diff.path().extension().map(|s| s == "diff") == Some(true)
        {
            remove_file(diff.path())
                .with_context(|| tr!("could not remove '{}'", diff.path().display().to_string()))?;
        }
    }

    Ok(())
}

fn clean_aur(
    config: &Config,
    keep_installed: bool,
    keep_current: bool,
    remove_all: bool,
    rm: bool,
) -> Result<()> {
    if !config.fetch.clone_dir.exists() {
        return Ok(());
    }

    let cached_pkgs = read_dir(&config.fetch.clone_dir)
        .with_context(|| tr!("can't open clone dir: {}", config.fetch.clone_dir.display()))?;

    for file in cached_pkgs {
        if let Err(err) = clean_aur_pkg(config, file, remove_all, keep_installed, keep_current, rm)
        {
            print_error(config.color.error, err);
            continue;
        }
    }

    Ok(())
}

fn fix_perms(file: &Path) -> Result<()> {
    let pkg = file.join("pkg");
    let mut perms = pkg.metadata()?.permissions();
    perms.set_mode(0o755);
    set_permissions(pkg, perms)?;
    Ok(())
}

fn clean_aur_pkg(
    config: &Config,
    file: std::io::Result<DirEntry>,
    remove_all: bool,
    keep_installed: bool,
    keep_current: bool,
    rm: bool,
) -> Result<()> {
    let file = file?;

    if !file.file_type()?.is_dir()
        || !file.path().join(".git").exists()
        || !file.path().join(".SRCINFO").exists()
    {
        return Ok(());
    }

    let _ = fix_perms(&file.path());

    if remove_all {
        return do_remove(config, &file.path(), rm);
    }

    let srcinfo = Srcinfo::parse_file(file.path().join(".SRCINFO"))?;

    if config.clean == 1 {
        if keep_installed {
            let local_db = config.alpm.localdb();
            for pkg in &srcinfo.pkgs {
                if let Ok(pkg) = local_db.pkg(&*pkg.pkgname) {
                    if pkg.version().as_str() == srcinfo.version() {
                        return Ok(());
                    }
                }
            }
        }

        if keep_current {
            for pkg in &srcinfo.pkgs {
                let sync_dbs = config.alpm.syncdbs();
                if let Ok(pkg) = sync_dbs.pkg(&*pkg.pkgname) {
                    if pkg.version().as_str() == srcinfo.version() {
                        return Ok(());
                    }
                }
            }
        }
    }

    do_remove(
        config,
        &config.fetch.clone_dir.join(srcinfo.base.pkgbase),
        rm,
    )
}

fn do_remove(config: &Config, path: &Path, rm: bool) -> Result<()> {
    if rm {
        remove_dir_all(path)
            .with_context(|| tr!("could not remove '{}'", path.display().to_string()))
    } else {
        clean_untracked(config, path)
    }
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
