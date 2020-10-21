use crate::config::Config;
use crate::exec;

use anyhow::{ensure, Context, Result};
use reqwest::blocking::get;
use std::io::Write;

pub fn list(config: &Config) -> Result<i32> {
    let mut args = config.pacman_args();
    let dbs = config.alpm.syncdbs();
    let dbs = dbs.collect::<Vec<_>>();

    if args.targets.is_empty() {
        args.targets = dbs.iter().map(|db| db.name()).collect();
        args.target("aur")
    };

    let has_aur = args.targets.contains(&"aur");
    args.targets.retain(|&t| t != "aur");

    if !args.targets.is_empty() {
        exec::pacman(config, &args)?;
    }

    if has_aur {
        list_aur(config)?;
    }

    Ok(0)
}

pub fn list_aur(config: &Config) -> Result<()> {
    let url = config.aur_url.join("packages.gz")?;
    let resp = get(url.clone()).with_context(|| format!("get {}", url))?;
    let success = resp.status().is_success();
    let db = config.alpm.localdb();
    ensure!(success, "get {}: {}", url, resp.status());

    let data = resp.bytes()?;

    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();

    let repo = config.color.sl_repo;
    let pkg = config.color.sl_pkg;
    let version = config.color.sl_version;
    let installed = config.color.sl_installed;

    for line in data
        .split(|&c| c == b'\n')
        .skip(1)
        .filter(|l| !l.is_empty())
    {
        if config.args.has_arg("q", "quiet") {
            let _ = stdout.write_all(&line);
            let _ = stdout.write_all(&[b'\n']);
            continue;
        }
        let _ = repo.paint(&b"aur "[..]).write_to(&mut stdout);
        let _ = pkg.paint(line).write_to(&mut stdout);
        let _ = version.paint(&b" unknown-version"[..]).write_to(&mut stdout);

        if db.pkg(String::from_utf8_lossy(&line)).is_ok() {
            let _ = installed.paint(&b" [installed]"[..]).write_to(&mut stdout);
        }

        let _ = stdout.write_all(&[b'\n']);
    }

    Ok(())
}
