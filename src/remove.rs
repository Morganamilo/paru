use crate::devel::{load_devel_info, save_devel_info};
use crate::print_error;
use crate::util::pkg_base_or_name;
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
        .map(|pkg| pkg_base_or_name(&pkg))
        .collect::<Vec<_>>();

    let mut db_map: HashMap<String, Vec<String>> = HashMap::new();
    let (_, local_repos) = repo::repo_aur_dbs(config);
    for pkg in &config.targets {
        for db in &local_repos {
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

    let (_, dbs) = repo::repo_aur_dbs(config);

    for target in bases {
        devel_info.info.remove(target);
    }

    drop(dbs);

    if let Err(err) = save_devel_info(config, &devel_info) {
        print_error(config.color.error, err);
        ret = 1;
    }

    Ok(ret)
}
