use crate::devel::{load_devel_info, save_devel_info};
use crate::print_error;
use crate::Config;
use crate::{exec, repo};

use std::collections::HashMap;

use anyhow::Result;

pub fn remove(config: &mut Config) -> Result<i32> {
    let mut devel_info = load_devel_info(config)?.unwrap_or_default();
    let db = config.alpm.localdb();
    let bases = config
        .targets
        .iter()
        .filter_map(|pkg| db.pkg(pkg.as_str()).ok())
        .map(|pkg| pkg.base().unwrap_or(pkg.name()))
        .collect::<Vec<_>>();

    let mut db_map: HashMap<String, Vec<String>> = HashMap::new();
    let dbs = config.alpm.syncdbs();
    let local_repos = repo::configured_local_repos(config);
    let local_dbs = local_repos
        .iter()
        .filter_map(|r| dbs.iter().find(|db| db.name() == *r))
        .collect::<Vec<_>>();

    for pkg in &config.targets {
        for db in &local_dbs {
            if let Ok(pkg) = db.pkg(pkg.as_str()) {
                db_map
                    .entry(db.name().to_string())
                    .or_default()
                    .push(pkg.name().to_string());
            }
        }
    }

    let mut ret = exec::pacman(config, &config.args)?.code();
    if ret != 0 {
        return Ok(ret);
    }

    for target in bases {
        devel_info.info.remove(target);
    }

    if let Err(err) = save_devel_info(config, &devel_info) {
        print_error(config.color.error, err);
        ret = 1;
    }

    if config.local {
        for (db, pkgs) in &db_map {
            let db = local_dbs.iter().find(|d| d.name() == *db).unwrap();
            let name = db.name();
            let path = db.servers().first().unwrap().trim_start_matches("file://");
            let _ = repo::remove(config, path, name, &pkgs);
        }
        repo::refresh(
            config,
            &db_map.keys().map(|s| s.as_str()).collect::<Vec<_>>(),
        )?;
    }

    Ok(ret)
}
