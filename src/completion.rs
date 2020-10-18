use crate::config::Config;
use crate::print_error;

use std::fs::{metadata, OpenOptions};
use std::io::{stdout, BufRead, BufReader, Write};
use std::time::{Duration, SystemTime};

use anyhow::{ensure, Context, Result};
use libflate::gzip::Decoder;
use reqwest::blocking::get;

fn save_aur_list(config: &Config) -> Result<()> {
    let url = config.aur_url.join("packages.gz")?;
    let resp = get(url.clone()).with_context(|| format!("get {}", url))?;
    let success = resp.status().is_success();
    ensure!(success, "get {}: {}", url, resp.status());

    let data = resp.bytes()?;
    let decoder = Decoder::new(&data[..]).context("invalid gzip data")?;
    let decoder = BufReader::new(decoder);

    let path = config.cache_dir.join("packages.aur");
    let file = OpenOptions::new().write(true).create(true).open(&path);
    let mut file =
        file.with_context(|| format!("failed to open cache file '{}'", path.display()))?;

    for line in decoder.split(b'\n').skip(1) {
        let line = line?;
        if !line.is_empty() {
            file.write_all(&line)?;
            file.write_all(b"\n")?;
        }
    }

    Ok(())
}

fn update_aur_cache(config: &Config, timeout: Option<u64>) -> Result<()> {
    let path = config.cache_dir.join("packages.aur");
    let metadata = metadata(&path);

    let need_refresh = match metadata {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
        Err(err) => return Err(anyhow::Error::new(err)),
        Ok(metadate) => match timeout {
            Some(timeout) => {
                metadate.modified()?
                    > SystemTime::now() + Duration::from_secs(60 * 60 * 24 * timeout)
            }
            None => false,
        },
    };

    if need_refresh {
        save_aur_list(config)?;
    }

    Ok(())
}

fn aur_list<W: Write>(config: &Config, w: &mut W, timeout: Option<u64>) -> Result<()> {
    update_aur_cache(config, timeout).context("could not update aur cache")?;
    let path = config.cache_dir.join("packages.aur");
    let file = OpenOptions::new().read(true).open(path)?;
    let file = BufReader::new(file);

    for line in file.split(b'\n') {
        let _ = w.write_all(&line?);
        let _ = w.write_all(b" AUR\n");
    }

    Ok(())
}

fn repo_list<W: Write>(config: &Config, w: &mut W) -> Result<()> {
    for db in config.alpm.syncdbs() {
        for pkg in db.pkgs()? {
            let _ = w.write_all(pkg.name().as_bytes());
            let _ = w.write_all(b" ");
            let _ = w.write_all(db.name().as_bytes());
            let _ = w.write_all(b"\n");
        }
    }

    Ok(())
}

pub fn print(config: &Config, timeout: Option<u64>) -> i32 {
    let stdout = stdout();
    let mut stdout = stdout.lock();
    let mut ret = 0;

    if let Err(err) = repo_list(config, &mut stdout) {
        ret = 1;
        print_error(config.color.error, err);
    }

    if let Err(err) = aur_list(config, &mut stdout, timeout) {
        ret = 1;
        print_error(config.color.error, err);
    }

    ret
}
