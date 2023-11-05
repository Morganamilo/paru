#![cfg(feature = "mock")]

use alpm::Alpm;
use anyhow::{Context, Result};
use std::env::var;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

async fn run(run_args: &[&str], repo: bool) -> Result<(TempDir, i32)> {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let testdata = Path::new(&var("CARGO_MANIFEST_DIR").unwrap()).join("testdata");

    let status = Command::new("cp")
        .arg("-rp")
        .arg(testdata.join("pacman.conf"))
        .arg(dir.join("pacman.conf"))
        .status()?;
    assert!(status.success());

    let status = Command::new("cp")
        .arg("-rp")
        .arg(testdata.join("makepkg.conf"))
        .arg(dir.join("makepkg.conf"))
        .status()?;
    assert!(status.success());

    let status = Command::new("cp")
        .arg("-rp")
        .arg(testdata.join("devel.toml"))
        .arg(dir.join("devel.toml"))
        .status()?;
    assert!(status.success());

    let status = Command::new("cp")
        .arg("-rp")
        .arg(testdata.join("db"))
        .arg(dir.join("db"))
        .status()?;
    assert!(status.success());

    let status = Command::new("cp")
        .arg("-rp")
        .arg(testdata.join("pkgbuild-repo"))
        .arg(dir.join("pkgbuils-repo"))
        .status()?;
    assert!(status.success());

    if repo {
        let status = Command::new("cp")
            .arg("-pa")
            .arg(testdata.join("repo"))
            .arg(dir.join("repo"))
            .status()?;
        assert!(status.success());
    }

    std::fs::create_dir_all(dir.join("cache/pkg"))?;
    let _ = std::fs::create_dir_all(testdata.join("pkg"));

    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(dir.join("makepkg.conf"))?;

    writeln!(
        file,
        "\n PKGDEST={0:}/pkgdest \n SRCDEST={0:}/src \n BUILDDIR={0:}/build",
        dir.join("cache").to_str().unwrap(),
    )?;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .append(true)
        .open(dir.join("pacman.conf"))?;

    writeln!(
        file,
        "[options]
        DBPath = {}
        CacheDir = {}
        CacheDir = {}",
        dir.join("db").to_str().unwrap(),
        dir.join("cache/pkg").to_str().unwrap(),
        testdata.join("pkg").to_str().unwrap()
    )?;

    if repo {
        writeln!(
            file,
            "[repo]
            Server = file://{0:}/repo
            SigLevel = Never",
            dir.display(),
        )?;
        std::fs::write(dir.join("localrepo"), "1")?;
    }

    let mconf = dir.join("makepkg.conf");
    let mconf = mconf.to_str();

    let pconf = dir.join("pacman.conf");
    let pconf = pconf.to_str();

    let dbpath = dir.join("db");
    let dbpath = dbpath.to_str();

    let clonedir = testdata.join("clone");
    let clonedir = clonedir.to_str();

    let develfile = dir.join("devel.toml");
    let develfile = develfile.to_str();

    let mut args = vec![
        "--root=/var/empty",
        "--dbonly",
        "--dbpath",
        dbpath.unwrap(),
        "--aururl=https://test.com",
        "--noconfirm",
        "--clonedir",
        clonedir.unwrap(),
        "--config",
        pconf.unwrap(),
        "--develfile",
        develfile.unwrap(),
        "--makepkgconf",
        mconf.unwrap(),
    ];

    if repo {
        args.push("--localrepo");
    }

    let mut path = std::env::var("PATH").unwrap();
    path.push(':');
    path.push_str(testdata.join("bin").to_str().unwrap());

    std::env::set_var("PACMAN", "true");
    std::env::set_var("PACMAN_CONF", dir.join("pacman.conf"));
    std::env::set_var("DBPATH", dir.join("db"));
    std::env::set_var("PARU_CONF", testdata.join("paru.conf"));
    std::env::set_var("PATH", path);

    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_CHECKDEPENDS_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_VARIABLE_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_PKGVER_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_ARCH_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_MAKEDEPENDS_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_PACKAGE_FUNCTION_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_SOURCE_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_OPTIONS_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_PROVIDES_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_OPTDEPENDS_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_CHANGELOG_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_INSTALL_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_PKGBASE_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_FULLPKGVER_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_PKGREL_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_EPOCH_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_BACKUP_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_PKGNAME_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_PKGLIST_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_UTIL_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_PACKAGE_FUNCTION_VARIABLE_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_DEPENDS_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_CONFLICTS_SH", "1");
    std::env::set_var("LIBMAKEPKG_LINT_PKGBUILD_ARCH_SPECIFIC_SH", "1");

    if repo {
        let mut args = args.clone();
        args.push("-Ly");
        let ret = paru::run(&args).await;
        assert_eq!(ret, 0);
    }

    args.extend(run_args);
    let ret = paru::run(&args).await;

    for pkg in std::fs::read_dir(dir.join("cache/pkg"))? {
        let path = pkg?.path();

        let name = path.file_name().unwrap().to_str().unwrap();
        if name.ends_with(".pkg.tar.zst") {
            let _ = std::fs::rename(&path, testdata.join("pkg").join(name));
        }
    }

    Ok((tmp, ret))
}

pub async fn run_normal(run_args: &[&str]) -> Result<(TempDir, i32)> {
    run(run_args, false).await
}

pub async fn run_combined(run_args: &[&str]) -> Result<(TempDir, i32)> {
    let mut args = run_args.to_vec();
    args.push("--combinedupgrade");
    run(&args, false).await
}

pub async fn run_chroot(run_args: &[&str]) -> Result<(TempDir, i32)> {
    let mut args = run_args.to_vec();
    args.push("--chroot");
    run(&args, false).await
}

pub async fn run_repo(run_args: &[&str]) -> Result<(TempDir, i32)> {
    let args = run_args.to_vec();
    run(&args, true).await
}

pub async fn run_repo_chroot(run_args: &[&str]) -> Result<(TempDir, i32)> {
    let mut args = run_args.to_vec();
    args.push("--chroot");
    run(&args, true).await
}

pub fn alpm(tmp: &TempDir) -> Result<Alpm> {
    let alpm = Alpm::new("/var/empty", tmp.path().join("db").to_str().unwrap())?;
    if tmp.path().join("localrepo").exists() {
        alpm.register_syncdb("repo", alpm::SigLevel::NONE).unwrap();
    }
    Ok(alpm)
}

pub fn assert_in_local_repo(alpm: &Alpm, pkg: &str) {
    if let Some(repo) = alpm.syncdbs().iter().find(|db| db.name() == "repo") {
        repo.pkg(pkg)
            .context(pkg.to_string())
            .expect("pkg not in local repo");
    }
}
