use crate::config::Config;
use crate::download;
use crate::download::{cache_info_with_warnings, Bases};
use crate::util::split_repo_aur_pkgs;
use crate::{print_error, sprintln};

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fs::{create_dir_all, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::iter::FromIterator;

use anyhow::{Context, Result};
use futures::future::{join_all, try_join_all};
use raur_ext::RaurExt;
use serde::{Deserialize, Serialize};
use srcinfo::Srcinfo;
use tokio::process::Command as AsyncCommand;

#[derive(Serialize, Deserialize, SmartDefault, Debug, Eq, Clone)]
pub struct RepoInfo {
    pub url: String,
    pub branch: Option<String>,
    pub commit: String,
}

impl Hash for RepoInfo {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.branch.hash(state);
        self.url.hash(state);
    }
}

impl std::cmp::PartialEq for RepoInfo {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url && self.branch == other.branch
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct PkgInfo {
    pub repos: HashSet<RepoInfo>,
}

impl std::borrow::Borrow<str> for RepoInfo {
    fn borrow(&self) -> &str {
        self.url.as_str()
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct DevelInfo {
    pub info: HashMap<String, PkgInfo>,
}

pub fn gendb(config: &mut Config) -> Result<()> {
    let action = config.color.action;
    let bold = config.color.bold;

    let db = config.alpm.localdb();
    let pkgs = db.pkgs().iter().map(|p| p.name()).collect::<Vec<_>>();
    let ignore = &config.ignore;

    let aur = split_repo_aur_pkgs(config, &pkgs).1;

    sprintln!("{} {}", action.paint("::"), bold.paint("Querying AUR..."));
    let warnings = cache_info_with_warnings(&config.raur, &mut config.cache, &aur, ignore)?;
    warnings.all(config.color, config.cols);

    let bases = Bases::from_iter(warnings.pkgs);
    let mut srcinfos = HashMap::new();
    let mut failed = HashSet::new();

    for base in &bases.bases {
        let path = config.build_dir.join(base.package_base()).join(".SRCINFO");
        if path.exists() {
            let srcinfo = Srcinfo::parse_file(path)
                .with_context(|| format!("failed to parse srcinfo for '{}'", base));

            match srcinfo {
                Ok(srcinfo) => {
                    srcinfos.insert(srcinfo.base.pkgbase.to_string(), srcinfo);
                }
                Err(err) => {
                    print_error(config.color.error, err);
                    failed.insert(base.package_base());
                }
            };
        }
    }

    download::new_aur_pkgbuilds(config, &bases, &srcinfos)?;

    for base in &bases.bases {
        if failed.contains(base.package_base()) || srcinfos.contains_key(base.package_base()) {
            continue;
        }
        let path = config.build_dir.join(base.package_base()).join(".SRCINFO");
        if path.exists() {
            if let Entry::Vacant(vacant) = srcinfos.entry(base.package_base().to_string()) {
                let srcinfo = Srcinfo::parse_file(path)
                    .with_context(|| format!("failed to parse srcinfo for '{}'", base));

                match srcinfo {
                    Ok(srcinfo) => {
                        vacant.insert(srcinfo);
                    }
                    Err(err) => {
                        print_error(config.color.error, err);
                        continue;
                    }
                }
            }
        }
    }

    sprintln!(
        "{} {}",
        action.paint("::"),
        bold.paint("Looking for devel repos...")
    );

    let devel_info = fetch_devel_info(config, &bases, srcinfos)?;
    save_devel_info(config, &devel_info).context("failed to save devel info")?;
    Ok(())
}

pub fn save_devel_info(config: &Config, devel_info: &DevelInfo) -> Result<()> {
    create_dir_all(&config.cache_dir)
        .with_context(|| format!("mkdir: {}", config.cache_dir.display()))?;
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&config.devel_path);
    let mut file = file.with_context(|| format!("open: {}", config.devel_path.display()))?;
    let json = serde_json::to_string_pretty(&devel_info).unwrap();
    file.write_all(json.as_bytes())?;

    Ok(())
}

async fn ls_remote(config: &Config, remote: String, branch: Option<&str>) -> Result<String> {
    let mut command = AsyncCommand::new(&config.git_bin);
    command
        .args(&config.git_flags)
        .arg("ls-remote")
        .arg(&remote)
        .arg(branch.unwrap_or("HEAD"));

    let output = command.output().await?;

    let sha = String::from_utf8_lossy(&output.stdout)
        .split('\t')
        .next()
        .unwrap()
        .to_string();

    let _action = config.color.action;
    //sprintln!(" found git repo: {}",  remote);
    Ok(sha)
}

fn parse_url(source: &str) -> Option<(String, &'_ str, Option<&'_ str>)> {
    let url = source.splitn(2, "::").last().unwrap();

    if !url.starts_with("git") || !url.contains("://") {
        return None;
    }

    let mut split = url.splitn(2, "://");
    let protocol = split.next().unwrap();
    let protocol = protocol.rsplit('+').next().unwrap();
    let rest = split.next().unwrap();

    let mut split = rest.splitn(2, '#');
    let remote = format!("{}://{}", protocol, split.next().unwrap());

    let branch = if let Some(fragment) = split.next() {
        let mut split = fragment.splitn(2, '=');
        let frag_type = split.next().unwrap();

        match frag_type {
            "commit" | "tag" => return None,
            "branch" => split.next(),
            _ => None,
        }
    } else {
        None
    };

    Some((remote, protocol, branch))
}

pub fn devel_updates(config: &Config) -> Result<Vec<String>> {
    let mut rt = tokio::runtime::Runtime::new()?;
    let mut devel_info = load_devel_info(config)?.unwrap_or_default();
    let db = config.alpm.localdb();
    devel_info.info.retain(|pkg, _| db.pkg(pkg).map(|p| !p.should_ignore()).unwrap_or(false));
    save_devel_info(config, &devel_info)?;

    let updates = rt.block_on(async {
        let mut futures = Vec::new();

        for (pkg, repos) in &devel_info.info {
            for repo in &repos.repos {
                futures.push(has_update(config, pkg, repo));
            }
        }

        let updates = join_all(futures).await;
        updates.into_iter().flatten().collect::<Vec<_>>()
    });

    let info = config.raur.info_ext(&updates)?;

    for update in &updates {
        if !info.iter().any(|i| &i.name == update) {
            devel_info.info.remove(update);
        }
    }

    //save_devel_info(config, &devel_info)?;

    Ok(updates)
}

async fn has_update(config: &Config, pkg: &str, url: &RepoInfo) -> Option<String> {
    if let Ok(sha) = ls_remote(config, url.url.clone(), url.branch.as_deref()).await {
        if sha != *url.commit {
            return Some(pkg.to_string());
        }
    }

    None
}

pub fn fetch_devel_info(
    config: &Config,
    bases: &Bases,
    srcinfos: HashMap<String, Srcinfo>,
) -> Result<DevelInfo> {
    let mut devel_info = DevelInfo::default();

    let mut rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let mut parsed = Vec::new();
        let mut futures = Vec::new();

        for base in &bases.bases {
            let srcinfo = srcinfos.get(base.package_base());

            let srcinfo = match srcinfo {
                Some(v) => v,
                None => continue,
            };

            for url in srcinfo.base.source.iter().flat_map(|v| &v.vec) {
                if let Some((remote, _, branch)) = parse_url(&url) {
                    let future = ls_remote(config, remote.clone(), branch);
                    futures.push(future);
                    parsed.push((remote, base.package_base().to_string(), branch));
                }
            }
        }

        let commits = try_join_all(futures).await?;
        for ((remote, pkgbase, branch), commit) in parsed.into_iter().zip(commits) {
            let url_info = RepoInfo {
                url: remote,
                branch: branch.map(|s| s.to_string()),
                commit,
            };

            devel_info
                .info
                .entry(pkgbase)
                .or_default()
                .repos
                .insert(url_info);
        }

        Ok(devel_info)
    })
}

pub fn load_devel_info(config: &Config) -> Result<Option<DevelInfo>> {
    if let Ok(file) = OpenOptions::new().read(true).open(&config.devel_path) {
        let devel_info = serde_json::from_reader(file)
            .with_context(|| format!("invalid json: {}", config.devel_path.display()))?;

        let mut devel_info: DevelInfo = devel_info;
        devel_info
            .info
            .retain(|pkg, _| config.alpm.localdb().pkg(pkg).is_ok());

        Ok(Some(devel_info))
    } else {
        Ok(None)
    }
}
