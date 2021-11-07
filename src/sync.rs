use crate::config::{Config, Mode};
use crate::exec;

use std::io::Write;

use anyhow::{ensure, Context, Result};
use raur::Raur;
use tr::tr;

pub async fn filter(config: &Config) -> Result<i32> {
    let mut cache = raur::Cache::new();
    config.raur.cache_info(&mut cache, &config.targets).await?;

    for targ in config.targets.iter().filter(|t| cache.contains(t.as_str())) {
        println!("{}", targ);
    }

    if cache.len() == config.targets.len() {
        Ok(0)
    } else {
        Ok(127)
    }
}

pub async fn list(config: &Config) -> Result<i32> {
    let mut args = config.pacman_args();

    let mut show_aur = args.targets.is_empty() && config.mode != Mode::Repo;
    let dbs = config.alpm.syncdbs();

    if args.targets.is_empty() && config.mode != Mode::Aur {
        args.targets = dbs.iter().map(|db| db.name()).collect();
    };

    show_aur |= args.targets.contains(&config.aur_namespace());
    args.targets.retain(|&t| t != config.aur_namespace());

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

    let mut lines = data
        .split(|&c| c == b'\n')
        .skip(1)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>();

    lines.sort_unstable();

    for line in lines {
        if config.args.has_arg("q", "quiet") {
            let _ = stdout.write_all(line);
            let _ = stdout.write_all(&[b'\n']);
            continue;
        }
        let _ = repo.paint(&b"aur "[..]).write_to(&mut stdout);
        let _ = pkg.paint(line).write_to(&mut stdout);
        let _ = version
            .paint(&b" unknown-version"[..])
            .write_to(&mut stdout);

        if db.pkg(line).is_ok() {
            let _ = installed
                .paint(tr!(" [installed]").as_bytes())
                .write_to(&mut stdout);
        }

        let _ = stdout.write_all(&[b'\n']);
    }

    Ok(())
}
