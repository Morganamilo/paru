#![cfg(feature = "mock")]

use alpm::Alpm;
use anyhow::Result;
use std::env::var;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

pub async fn run(run_args: &[&str]) -> Result<(TempDir, i32)> {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let testdata = Path::new(&var("CARGO_MANIFEST_DIR").unwrap()).join("testdata");

    let status = Command::new("cp")
        .arg("-r")
        .arg(testdata.join("pacman.conf"))
        .arg(dir.join("pacman.conf"))
        .status()?;
    assert!(status.success());

    let status = Command::new("cp")
        .arg("-r")
        .arg(testdata.join("db"))
        .arg(dir.join("db"))
        .status()?;
    assert!(status.success());

    let mut file = fs::OpenOptions::new()
        .write(true)
        .append(true)
        .open(dir.join("pacman.conf"))?;

    writeln!(
        file,
        "[options]\nDBPath = {}",
        dir.join("db").to_str().unwrap()
    )?;

    let pconf = dir.join("pacman.conf");
    let pconf = pconf.to_str();

    let dbpath = dir.join("db");
    let dbpath = dbpath.to_str();

    let clonedir = testdata.join("clone");
    let clonedir = clonedir.to_str();

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
    ];
    args.extend(run_args);

    std::env::set_var("PACMAN", testdata.join("pacman"));
    std::env::set_var("PACMAN_CONF", dir.join("pacman.conf"));
    std::env::set_var("DBPATH", dir.join("db"));
    std::env::set_var("PARU_CONF", testdata.join("paru.conf"));

    let ret = paru::run(&args).await;
    Ok((tmp, ret))
}

pub async fn run_combined(run_args: &[&str]) -> Result<(TempDir, i32)> {
    let mut args = run_args.to_vec();
    args.push("--combinedupgrade");
    run(&args).await
}

pub fn alpm(tmp: &TempDir) -> Result<Alpm> {
    let alpm = Alpm::new("/var/empty", tmp.path().join("db").to_str().unwrap())?;
    Ok(alpm)
}
