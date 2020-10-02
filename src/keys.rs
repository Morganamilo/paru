use crate::config::Config;
use crate::download::{Base, Bases};
use crate::sprintln;
use crate::util::ask;

use std::collections::{HashMap, HashSet};
use std::process::Command;

use anyhow::{Context, Result};
use srcinfo::Srcinfo;

pub fn check_pgp_keys(
    config: &Config,
    bases: &Bases,
    srcinfos: &HashMap<String, Srcinfo>,
) -> Result<()> {
    let mut import: HashMap<&str, Vec<&Base>> = HashMap::new();
    let mut seen = HashSet::new();

    for base in &bases.bases {
        let pkg = base.package_base();
        let srcinfo = srcinfos.get(pkg).unwrap();
        for key in &srcinfo.base.valid_pgp_keys {
            if !seen.insert(key) {
                continue;
            }

            let ret = Command::new(&config.gpg_bin)
                .args(&config.gpg_flags)
                .arg("--list-keys")
                .arg(key)
                .output()?;

            if !ret.status.success() {
                import.entry(key).or_default().push(base);
            }
        }
    }

    if !import.is_empty() {
        // TODO format
        sprintln!("keys need to be imported:");
        for (base, keys) in &import {
            let keys = keys.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            sprintln!("{} wanted by: {}", base, keys.join("  "));
        }
        if ask(config, "import?", true) {
            import_keys(config, &import)?;
        }
    }

    Ok(())
}

fn import_keys(config: &Config, import: &HashMap<&str, Vec<&Base>>) -> Result<()> {
    Command::new(&config.gpg_bin)
        .args(&config.gpg_flags)
        .arg("--recv-keys")
        .args(import.keys())
        .spawn()
        .with_context(|| {
            format!(
                "failed to run {} {} --resv-keys {}",
                config.gpg_bin,
                config.gpg_flags.join(" "),
                import.keys().cloned().collect::<Vec<_>>().join(" ")
            )
        })?
        .wait()
        .context("failed to import keys")?;

    Ok(())
}
