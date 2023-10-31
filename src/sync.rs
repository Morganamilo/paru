use crate::config::Config;
use crate::pkgbuild::PkgbuildRepos;
use crate::{exec, print_error};

use std::io::Write;

use anyhow::{anyhow, ensure, Context, Result};

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
    let c = config.color;
    let args = config.pacman_args();
    let mut ret = 0;

    if args.targets.is_empty() {
        if config.mode.repo() {
            if let Err(e) = exec::pacman(config, &args) {
                print_error(c.error, e);
                ret = 1
            }
        }
        if config.mode.pkgbuild() {
            for repo in &config.pkgbuild_repos.repos {
                list_pkgbuilds(config, &config.pkgbuild_repos, &repo.name);
            }
        }
        if config.mode.aur() {
            if let Err(e) = list_aur(config).await {
                print_error(c.error, e);
                ret = 1
            }
        }
    } else {
        for &target in &args.targets {
            if config.alpm.syncdbs().iter().any(|r| r.name() == target) && config.mode.repo() {
                let mut args = args.clone();
                args.targets.clear();
                args.target(target);
                if let Err(e) = exec::pacman(config, &args) {
                    print_error(c.error, e);
                    ret = 1;
                }
            } else if config.pkgbuild_repos.repo(target).is_some() && config.mode.pkgbuild() {
                list_pkgbuilds(config, &config.pkgbuild_repos, target);
            } else if target == config.aur_namespace() && config.mode.aur() {
                if let Err(e) = list_aur(config).await {
                    print_error(c.error, e);
                    ret = 1;
                }
            } else {
                print_error(c.error, anyhow!("repository \"{}\" was not found", target));
                ret = 1;
            }
        }
    }

    Ok(ret)
}

pub fn list_pkgbuilds(config: &Config, repos: &PkgbuildRepos, repo: &str) {
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();

    if let Some(repo) = repos.repo(repo) {
        for pkg in repo.pkgs(config) {
            for name in pkg.srcinfo.names() {
                print_pkg(
                    config,
                    &mut stdout,
                    name.as_bytes(),
                    &repo.name,
                    &pkg.srcinfo.version(),
                )
            }
        }
    }
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
    ensure!(success, "get {}: {}", url, resp.status());

    let data = resp.bytes().await?;

    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();

    let mut lines = data
        .split(|&c| c == b'\n')
        .skip(1)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>();

    lines.sort_unstable();

    for line in lines {
        print_pkg(config, &mut stdout, line, "aur", "unknown-version");
    }

    Ok(())
}

fn print_pkg(config: &Config, mut stdout: impl Write, line: &[u8], repo: &str, version: &str) {
    let cpkg = config.color.sl_pkg;
    let crepo = config.color.sl_repo;
    let cversion = config.color.sl_version;
    let cinstalled = config.color.sl_installed;

    if config.args.has_arg("q", "quiet") {
        let _ = stdout.write_all(line);
        let _ = stdout.write_all(&[b'\n']);
        return;
    }
    let _ = crepo.paint(repo.as_bytes()).write_to(&mut stdout);
    let _ = stdout.write_all(b" ");
    let _ = cpkg.paint(line).write_to(&mut stdout);
    let _ = stdout.write_all(b" ");
    let _ = cversion.paint(version.as_bytes()).write_to(&mut stdout);

    if config.alpm.localdb().pkg(line).is_ok() {
        let _ = cinstalled
            .paint(tr!(" [installed]").as_bytes())
            .write_to(&mut stdout);
    }

    let _ = stdout.write_all(&[b'\n']);
}
