use crate::config::Config;
use crate::print_error;

use std::fs::{create_dir_all, metadata, OpenOptions};
use std::io::{stdout, BufRead, BufReader, Write};
use std::path::Path;
use std::time::{Duration, SystemTime};

use anyhow::{ensure, Context, Result};
use reqwest::get;
use tr::tr;
use url::Url;

async fn save_aur_list(aur_url: &Url, cache_dir: &Path) -> Result<()> {
    let url = aur_url.join("packages.gz")?;
    let resp = get(url.clone())
        .await
        .with_context(|| format!("get {}", url))?;
    let success = resp.status().is_success();
    ensure!(success, "get {}: {}", url, resp.status());

    let data = resp.bytes().await?;

    create_dir_all(cache_dir)?;
    let path = cache_dir.join("packages.aur");
    let file = OpenOptions::new().write(true).create(true).open(&path);
    let mut file = file.with_context(|| tr!("failed to open cache file '{}'", path.display()))?;

    for line in data.split(|&c| c == b'\n').skip(1) {
        if !line.is_empty() {
            file.write_all(line)?;
            file.write_all(b"\n")?;
        }
    }

    Ok(())
}

pub async fn update_aur_cache(aur_url: &Url, cache_dir: &Path, timeout: Option<u64>) -> Result<()> {
    let path = cache_dir.join("packages.aur");
    let metadata = metadata(&path);

    let need_refresh = match metadata {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
        Err(err) => return Err(anyhow::Error::new(err)),
        Ok(metadate) => match timeout {
            Some(timeout) => {
                metadate.modified()?
                    < SystemTime::now() - Duration::from_secs(60 * 60 * 24 * timeout)
            }
            None => false,
        },
    };

    if need_refresh {
        save_aur_list(aur_url, cache_dir).await?;
    }

    Ok(())
}

async fn aur_list<W: Write>(config: &Config, w: &mut W, timeout: Option<u64>) -> Result<()> {
    update_aur_cache(&config.aur_url, &config.cache_dir, timeout)
        .await
        .context(tr!("could not update aur cache"))?;
    let path = config.cache_dir.join("packages.aur");
    let file = OpenOptions::new().read(true).open(path)?;
    let file = BufReader::new(file);

    for line in file.split(b'\n') {
        let _ = w.write_all(&line?);
        let _ = w.write_all(b" AUR\n");
    }

    Ok(())
}

fn repo_list<W: Write>(config: &Config, w: &mut W) {
    for db in config.alpm.syncdbs() {
        for pkg in db.pkgs() {
            let _ = w.write_all(pkg.name().as_bytes());
            let _ = w.write_all(b" ");
            let _ = w.write_all(db.name().as_bytes());
            let _ = w.write_all(b"\n");
        }
    }
}

pub async fn print(config: &Config, timeout: Option<u64>) -> i32 {
    let stdout = stdout();
    let mut stdout = stdout.lock();

    repo_list(config, &mut stdout);

    if let Err(err) = aur_list(config, &mut stdout, timeout).await {
        print_error(config.color.error, err);
        return 1;
    }

    0
}
