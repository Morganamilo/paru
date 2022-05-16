use crate::config::Config;
use crate::download::Bases;
use crate::exec;
use crate::printtr;
use crate::util::ask;

use std::collections::{HashMap, HashSet};
use std::process::Command;

use anyhow::Result;
use aur_depends::Base;
use srcinfo::Srcinfo;
use tr::tr;

pub fn check_pgp_keys(
    config: &Config,
    bases: &Bases,
    srcinfos: &HashMap<String, Srcinfo>,
) -> Result<()> {
    let mut import: HashMap<&str, Vec<&Base>> = HashMap::new();
    let mut seen = HashSet::new();
    let c = config.color;

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
        println!(
            "{} {}",
            c.action.paint("::"),
            c.bold.paint(tr!("keys need to be imported:"))
        );
        for (key, base) in &import {
            let base = base.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            printtr!(
                "     {key} wanted by: {base}",
                key = c.bold.paint(*key),
                base = base.join("  ")
            );
        }
        if ask(config, "import?", true) {
            import_keys(config, &import)?;
        }
    }

    Ok(())
}

fn import_keys(config: &Config, import: &HashMap<&str, Vec<&Base>>) -> Result<()> {
    let mut cmd = Command::new(&config.gpg_bin);
    cmd.args(&config.gpg_flags)
        .arg("--recv-keys")
        .args(import.keys());
    exec::command(&mut cmd)
}
