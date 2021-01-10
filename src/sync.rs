use crate::config::Config;
use crate::{exec, repo};

use std::io::Write;

use anyhow::{ensure, Context, Result};
use raur::Raur;

pub async fn filter(config: &Config) -> Result<i32> {
    let mut cache = raur::Cache::new();
    config.raur.cache_info(&mut cache, &config.targets).await?;

    for targ in config.targets.iter().filter(|t| cache.contains(t.as_str())) {
        println!("{}", targ);
    }

    Ok(0)
}

pub async fn list(config: &Config) -> Result<i32> {
    let mut args = config.pacman_args();

    if config.local {
        args.targets = repo::configured_local_repos(&config);
    }

    let mut show_aur = args.targets.is_empty() && config.mode != "repo";
    let dbs = config.alpm.syncdbs();

    if args.targets.is_empty() {
        if config.mode != "aur" {
            args.targets = dbs.iter().map(|db| db.name()).collect();
        }
    };

    if config.aur_namespace() {
        show_aur |= args.targets.contains(&"aur");
        args.targets.retain(|&t| t != "aur");
    }

    if !args.targets.is_empty() {
        exec::pacman(config, &args)?;
    }

    if show_aur {
        list_aur(config).await?;
    }

    Ok(0)
}

pub async fn list_aur(config: &Config) -> Result<()> {
    let url = config.aur_url.join("packages.gz")?;
    let client = config.raur.client();
    let resp = client
        .get(url.clone())
        .send()
        .await
        .with_context(|| format!("get {}", url))?;
    let success = resp.status().is_success();
    let db = config.alpm.localdb();
    ensure!(success, "get {}: {}", url, resp.status());

    let data = resp.bytes().await?;

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
        let _ = version
            .paint(&b" unknown-version"[..])
            .write_to(&mut stdout);

        if db.pkg(line).is_ok() {
            let _ = installed.paint(&b" [installed]"[..]).write_to(&mut stdout);
        }

        let _ = stdout.write_all(&[b'\n']);
    }

    Ok(())
}
