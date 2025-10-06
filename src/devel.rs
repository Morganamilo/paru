use crate::config::{Config, LocalRepos};
use crate::download::{self, cache_info_with_warnings, Bases};
use crate::print_error;
use crate::repo;
use crate::util::{pkg_base_or_name, split_repo_aur_pkgs};

use std::cmp::Ordering;
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{create_dir_all, read_to_string, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::iter::FromIterator;
use std::time::Duration;

use alpm_utils::{DbListExt, Target};
use ansiterm::Style;
use anyhow::{anyhow, bail, Context, Result};
use aur_depends::Base;
use futures::future::{join_all, select_ok, FutureExt};
use log::debug;
use raur::{Cache, Raur};
use serde::{Deserialize, Serialize, Serializer};
use srcinfo::Srcinfo;
use tokio::process::Command as AsyncCommand;
use tokio::time::timeout;
use tr::tr;

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct _PkgInfo {
    pub repos: HashSet<RepoInfo>,
}

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

impl PartialOrd for RepoInfo {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(std::cmp::Ord::cmp(self, other))
    }
}

impl Ord for RepoInfo {
    fn cmp(&self, other: &Self) -> Ordering {
        self.url
            .cmp(&other.url)
            .then(self.branch.cmp(&other.branch))
    }
}

impl std::cmp::PartialEq for RepoInfo {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url && self.branch == other.branch
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(transparent)]
pub struct PkgInfo {
    #[serde(serialize_with = "ordered_set")]
    pub repos: HashSet<RepoInfo>,
}

impl std::borrow::Borrow<str> for RepoInfo {
    fn borrow(&self) -> &str {
        self.url.as_str()
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct DevelInfo {
    #[serde(rename = "info")]
    #[serde(default)]
    #[serde(skip_serializing)]
    _info: HashMap<String, _PkgInfo>,
    #[serde(flatten)]
    #[serde(serialize_with = "ordered_map")]
    pub info: HashMap<String, PkgInfo>,
}

fn ordered_map<S, T>(value: &HashMap<String, T>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize,
{
    let ordered: BTreeMap<_, _> = value.iter().collect();
    ordered.serialize(serializer)
}

fn ordered_set<S, T>(value: &HashSet<T>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize + Ord,
{
    let ordered: BTreeSet<_> = value.iter().collect();
    ordered.serialize(serializer)
}

pub async fn gendb(config: &mut Config) -> Result<()> {
    let action = config.color.action;
    let bold = config.color.bold;

    let db = config.alpm.localdb();
    let pkgs: Vec<_> = db.pkgs().iter().map(|p| p.name()).collect();
    let ignore = &config.ignore;

    let (_, mut aur) = split_repo_aur_pkgs(config, &pkgs);
    let mut devel_info = load_devel_info(config)?.unwrap_or_default();

    aur.retain(|pkg| {
        let pkg = db.pkg(*pkg).unwrap();
        let pkg = pkg.base().unwrap_or_else(|| pkg.name());

        !devel_info.info.contains_key(pkg)
    });

    let (_pkgbuilds, aur): (Vec<_>, Vec<_>) = aur
        .into_iter()
        .partition(|aur| config.pkgbuild_repos.pkg(config, aur).is_some());

    if !aur.is_empty() {
        println!(
            "{} {}",
            action.paint("::"),
            bold.paint(tr!("Querying AUR..."))
        );
    }
    let warnings = cache_info_with_warnings(
        &config.raur,
        &mut config.cache,
        &aur,
        ignore,
        &config.no_warn,
    )
    .await?;
    warnings.all(config.color, config.cols);

    let bases = Bases::from_iter(warnings.pkgs);
    let mut srcinfos = HashMap::new();
    let mut failed = HashSet::new();

    for base in &bases.bases {
        let path = config.build_dir.join(base.package_base()).join(".SRCINFO");
        if path.exists() {
            let srcinfo = Srcinfo::parse_file(path)
                .with_context(|| tr!("failed to parse srcinfo for '{}'", base));

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

    download::new_aur_pkgbuilds(config, &bases, &srcinfos).await?;

    for base in &bases.bases {
        if failed.contains(base.package_base()) || srcinfos.contains_key(base.package_base()) {
            continue;
        }
        let path = config.build_dir.join(base.package_base()).join(".SRCINFO");
        if path.exists() {
            if let Entry::Vacant(vacant) = srcinfos.entry(base.package_base().to_string()) {
                let srcinfo = Srcinfo::parse_file(path)
                    .with_context(|| tr!("failed to parse srcinfo for '{}'", base));

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

    let bases: Vec<_> = bases.bases.into_iter().map(Base::Aur).collect();

    println!(
        "{} {}",
        action.paint("::"),
        bold.paint(tr!("Looking for devel repos..."))
    );

    let new_devel_info = fetch_devel_info(config, &bases, &srcinfos).await?;

    for (k, v) in new_devel_info.info {
        devel_info.info.entry(k).or_insert(v);
    }

    save_devel_info(config, &devel_info).context(tr!("failed to save devel info"))?;

    Ok(())
}

pub fn save_devel_info(config: &Config, devel_info: &DevelInfo) -> Result<()> {
    create_dir_all(&config.state_dir).with_context(|| {
        tr!(
            "failed to create state directory: {}",
            config.state_dir.display()
        )
    })?;

    let mut temp = config.devel_path.to_owned();
    temp.set_extension("toml.tmp");

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&temp);

    let mut file =
        file.with_context(|| tr!("failed to create temporary file: {}", temp.display()))?;

    let toml = toml::to_string(&devel_info).unwrap();

    file.write_all(toml.as_bytes())
        .with_context(|| tr!("failed to write to temporary file: {}", temp.display()))?;

    drop(file);

    std::fs::rename(&temp, &config.devel_path).with_context(|| {
        tr!(
            "failed to rename '{temp}' to '{devel_toml}",
            temp = temp.display(),
            devel_toml = config.devel_path.display()
        )
    })?;

    Ok(())
}

async fn ls_remote_internal(
    git: &str,
    flags: &[String],
    remote: &str,
    branch: Option<&str>,
) -> Result<String> {
    #[cfg(feature = "mock")]
    let _ = git;
    #[cfg(feature = "mock")]
    let git = "git";

    let mut command = AsyncCommand::new(git);
    command
        .args(flags)
        .env("GIT_TERMINAL_PROMPT", "0")
        .arg("ls-remote")
        .arg(remote)
        .arg(branch.unwrap_or("HEAD"));

    debug!("git ls-remote {} {}", remote, branch.unwrap_or("HEAD"));
    let output = command.output().await?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr));
    }

    let sha = String::from_utf8_lossy(&output.stdout)
        .split('\t')
        .next()
        .unwrap()
        .to_string();

    Ok(sha)
}

async fn ls_remote(
    style: Style,
    git: &str,
    flags: &[String],
    remote: String,
    branch: Option<&str>,
) -> Result<String> {
    let remote = &remote;
    let time = Duration::from_secs(15);
    let future = ls_remote_internal(git, flags, remote, branch);
    let future = timeout(time, future);

    if let Ok(v) = future.await {
        v
    } else {
        print_error(
            style,
            anyhow!("timed out looking for devel update: {}", remote),
        );
        bail!("")
    }
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
    let remote = split.next().unwrap();
    let remote = remote.split_once('?').map_or(remote, |(x, _)| x);
    let remote = format!("{}://{}", protocol, remote);

    let branch = if let Some(fragment) = split.next() {
        let fragment = fragment.split_once('?').map_or(fragment, |(x, _)| x);
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

pub async fn possible_devel_updates(config: &Config) -> Result<Vec<String>> {
    let devel_info = load_devel_info(config)?.unwrap_or_default();
    let db = config.alpm.localdb();
    let mut futures = Vec::new();
    let mut pkgbases: HashMap<&str, Vec<&alpm::Package>> = HashMap::new();

    for pkg in db.pkgs().iter() {
        let name = pkg_base_or_name(pkg);
        pkgbases.entry(name).or_default().push(pkg);
    }

    'outer: for (pkg, repos) in &devel_info.info {
        if let Some(pkgs) = pkgbases.get(pkg.as_str()) {
            if pkgs.iter().all(|p| p.should_ignore()) {
                continue;
            }

            if pkgs.iter().all(|p| config.ignore_devel.is_match(p.name())) {
                continue;
            }
        }

        if config.repos != LocalRepos::None {
            let (_, dbs) = repo::repo_aur_dbs(config);
            for db in dbs {
                if db.pkg(pkg.as_str()).is_ok() {
                    futures.push(pkg_has_update(config, pkg, &repos.repos));
                    continue 'outer;
                }
            }
        } else if config.alpm.syncdbs().pkg(pkg.as_str()).is_err() {
            futures.push(pkg_has_update(config, pkg, &repos.repos));
        }
    }

    let updates = join_all(futures).await;

    let updates = updates
        .into_iter()
        .flatten()
        .map(|s| s.to_string())
        .collect::<std::collections::BTreeSet<_>>();

    Ok(updates.into_iter().collect())
}

pub async fn filter_devel_updates(
    config: &Config,
    cache: &mut Cache,
    updates: &[String],
) -> Result<Vec<Target>> {
    let mut pkgbases: HashMap<&str, Vec<&alpm::Package>> = HashMap::new();
    let mut aur = Vec::new();
    let mut custom = Vec::new();
    let db = config.alpm.localdb();

    'pkg: for update in updates {
        if let Some((base, pkg)) = config.pkgbuild_repos.pkg(config, update) {
            custom.push(Target::new(Some(base.repo.clone()), pkg.pkgname.clone()));
            continue 'pkg;
        }

        aur.push(update);
    }

    let (_, dbs) = repo::repo_aur_dbs(config);
    for pkg in dbs.iter().flat_map(|d| d.pkgs()) {
        let name = pkg_base_or_name(pkg);
        pkgbases.entry(name).or_default().push(pkg);
    }

    for pkg in db.pkgs().iter() {
        let name = pkg_base_or_name(pkg);
        pkgbases.entry(name).or_default().push(pkg);
    }

    config.raur.cache_info(cache, &aur).await?;
    let aur: Vec<_> = aur
        .iter()
        .map(|u| pkgbases.remove(u.as_str()).unwrap())
        .collect();

    let mut updates = Vec::new();

    if config.mode.aur() {
        let aur = aur
            .iter()
            .flatten()
            .filter(|p| !p.should_ignore())
            .filter(|p| !config.ignore_devel.is_match(p.name()))
            .map(|p| p.name().to_string())
            .filter(|p| cache.contains(p.as_str()))
            .map(|p| Target::new(Some(config.aur_namespace().to_string()), p));

        updates.extend(aur);
    }
    if config.mode.pkgbuild() {
        updates.extend(custom);
    }

    Ok(updates)
}

pub async fn pkg_has_update<'pkg>(
    config: &Config,
    pkg: &'pkg str,
    info: &HashSet<RepoInfo>,
) -> Option<&'pkg str> {
    if info.is_empty() {
        return None;
    }

    let mut futures = Vec::with_capacity(info.len());

    for info in info {
        if config.ignore_devel_source.iter().any(|u| u == &info.url) {
            continue;
        }

        futures
            .push(has_update(config.color.error, &config.git_bin, &config.git_flags, info).boxed());
    }

    if !futures.is_empty() && select_ok(futures).await.is_ok() {
        Some(pkg)
    } else {
        None
    }
}

async fn has_update(style: Style, git: &str, flags: &[String], url: &RepoInfo) -> Result<()> {
    let sha = ls_remote(style, git, flags, url.url.clone(), url.branch.as_deref()).await?;
    debug!(
        "devel check {}: '{}' == '{}' different: {}",
        url.url,
        url.commit,
        sha,
        url.commit != sha
    );
    if sha != *url.commit {
        return Ok(());
    }

    bail!(tr!("package does not have an update"))
}

pub async fn fetch_devel_info(
    config: &Config,
    bases: &[Base],
    srcinfos: &HashMap<String, Srcinfo>,
) -> Result<DevelInfo> {
    let mut devel_info = DevelInfo::default();

    let mut parsed = Vec::new();
    let mut futures = Vec::new();

    for base in bases {
        let srcinfo = match base {
            Base::Aur(_) => srcinfos.get(base.package_base()),
            Base::Pkgbuild(c) => Some(c.srcinfo.as_ref()),
        };

        let srcinfo = match srcinfo {
            Some(v) => v,
            None => continue,
        };

        for url in srcinfo.base.source.iter().flat_map(|v| &v.vec) {
            if let Some((remote, _, branch)) = parse_url(url) {
                let future = ls_remote(
                    config.color.error,
                    &config.git_bin,
                    &config.git_flags,
                    remote.clone(),
                    branch,
                );
                futures.push(future);
                parsed.push((remote, base.package_base().to_string(), branch));
            }
        }
    }

    let commits = join_all(futures).await;
    for ((remote, pkgbase, branch), commit) in parsed.into_iter().zip(commits) {
        match commit {
            Err(e) => print_error(
                config.color.error,
                e.context(tr!("failed to lookup: {}", pkgbase)),
            ),
            Ok(commit) => {
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
        }
    }

    Ok(devel_info)
}

pub fn load_devel_info(config: &Config) -> Result<Option<DevelInfo>> {
    let file = match read_to_string(&config.devel_path) {
        Ok(file) => file,
        _ => return Ok(None),
    };
    let devel_info = DevelInfo::deserialize(toml::Deserializer::new(&file))
        .with_context(|| tr!("invalid toml: {}", config.devel_path.display()))?;

    let mut pkgbases: HashMap<&str, Vec<&alpm::Package>> = HashMap::new();
    let mut devel_info: DevelInfo = devel_info;

    if !devel_info._info.is_empty() {
        for (pkg, info) in devel_info._info.drain() {
            devel_info.info.insert(pkg, PkgInfo { repos: info.repos });
        }
    }

    for pkg in config.alpm.localdb().pkgs().iter() {
        let name = pkg_base_or_name(pkg);
        pkgbases.entry(name).or_default().push(pkg);
    }

    let (_, dbs) = repo::repo_aur_dbs(config);
    for pkg in dbs.iter().flat_map(|d| d.pkgs()) {
        let name = pkg_base_or_name(pkg);
        pkgbases.entry(name).or_default().push(pkg);
    }

    devel_info
        .info
        .retain(|pkg, _| pkgbases.contains_key(pkg.as_str()));

    save_devel_info(config, &devel_info)?;

    Ok(Some(devel_info))
}
