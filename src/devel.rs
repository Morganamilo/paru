use crate::config::Config;
use crate::download::{self, Bases, cache_info_with_warnings};
use crate::print_error;
use crate::repo;
use crate::util::{pkg_base_or_name, repo_aur_pkgs, split_repo_aur_pkgs};

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{OpenOptions, create_dir_all, read_to_string};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::iter::FromIterator;
use std::time::Duration;

use alpm_utils::Target;
use ansiterm::Style;
use anyhow::{Context, Result, anyhow, bail};
use aur_depends::Base;
use futures::future::{FutureExt, join_all, select_ok};
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

#[derive(Default, Debug)]
pub struct DevelUpgrades {
    pub updates: Vec<String>,
    pub skipped: Vec<String>,
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
    let pkgs = db.pkgs().iter().map(|p| p.name()).collect::<Vec<_>>();
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
        config.raur.as_ref().unwrap(),
        &mut config.cache,
        &aur,
        ignore,
        &config.no_warn,
    )
    .await?;
    warnings.all(config.color, config.cols);

    let bases = Bases::from_iter(warnings.pkgs);
    let mut srcinfos = HashMap::new();
    for base in &bases.bases {
        let path = config.build_dir.join(base.package_base()).join(".SRCINFO");
        if let Ok(srcinfo) = Srcinfo::from_path(path) {
            srcinfos.insert(base.package_base().to_string(), srcinfo);
        }
    }

    download::new_aur_pkgbuilds(config, &bases, &srcinfos).await?;
    srcinfos.clear();

    for base in &bases.bases {
        let path = config.build_dir.join(base.package_base()).join(".SRCINFO");
        if path.exists() {
            let srcinfo = Srcinfo::from_path(path)
                .with_context(|| tr!("failed to parse srcinfo for '{}'", base));

            match srcinfo {
                Ok(srcinfo) => {
                    srcinfos.insert(base.package_base().to_string(), srcinfo);
                }
                Err(err) => print_error(config.color.error, err),
            }
        }
    }

    let bases = bases.bases.into_iter().map(Base::Aur).collect::<Vec<_>>();

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
            "failed to rename '{temp}' to '{devel_toml}'",
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
    #[cfg(all(feature = "mock", not(test)))]
    let _ = git;
    #[cfg(all(feature = "mock", not(test)))]
    let git = "git";

    let mut command = AsyncCommand::new(git);
    command
        .args(flags)
        .env("GIT_TERMINAL_PROMPT", "0")
        .arg("ls-remote")
        .arg("--")
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

pub async fn possible_devel_updates(config: &Config) -> Result<DevelUpgrades> {
    let devel_info = load_devel_info(config)?.unwrap_or_default();
    let mut futures = Vec::new();
    let mut pkgbases: HashMap<&str, Vec<&alpm::Package>> = HashMap::new();
    let mut skipped = Vec::new();

    let (_, pkgs) = repo_aur_pkgs(config);
    for pkg in pkgs {
        let name = pkg_base_or_name(pkg);
        pkgbases.entry(name).or_default().push(pkg);
    }

    for (pkg, repos) in &devel_info.info {
        let custom = config
            .pkgbuild_repos
            .repos
            .iter()
            .any(|repo| repo.base(config, pkg).is_some());
        if (custom && !config.mode.pkgbuild()) || (!custom && !config.mode.aur()) {
            continue;
        }

        let Some(pkgs) = pkgbases.get(pkg.as_str()) else {
            continue;
        };

        if pkgs
            .iter()
            .all(|p| p.should_ignore() || config.ignore_devel.is_match(p.name()))
        {
            skipped.extend(pkgs.iter().map(|p| p.name().to_string()));
            continue;
        }

        futures.push(pkg_has_update(config, pkg, &repos.repos));
    }

    let updates = join_all(futures).await;

    let mut updates = updates
        .into_iter()
        .flatten()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();

    updates.sort_unstable();
    updates.dedup();
    skipped.sort_unstable();
    skipped.dedup();

    Ok(DevelUpgrades { updates, skipped })
}

pub async fn filter_devel_updates(
    config: &Config,
    cache: &mut Cache,
    updates: &[String],
) -> Result<(Vec<Target>, Vec<String>)> {
    let mut pkgbases: HashMap<&str, Vec<&alpm::Package>> = HashMap::new();
    let mut aur = Vec::new();
    let mut targets = Vec::new();
    let mut ignored = Vec::new();
    let (_, pkgs) = repo_aur_pkgs(config);
    for pkg in pkgs {
        let name = pkg_base_or_name(pkg);
        pkgbases.entry(name).or_default().push(pkg);
    }

    for update in updates {
        let custom = config
            .pkgbuild_repos
            .repos
            .iter()
            .find_map(|repo| repo.base(config, update));
        if let Some(base) = custom {
            if !config.mode.pkgbuild() {
                continue;
            }
            for pkg in pkgbases.get(update.as_str()).into_iter().flatten() {
                let name = pkg.name();
                if base.srcinfo.pkg(name).is_none() {
                    continue;
                }
                if pkg.should_ignore() || config.ignore_devel.is_match(name) {
                    ignored.push(name.to_string());
                } else {
                    targets.push(Target::new(Some(base.repo.clone()), name.to_string()));
                }
            }
        } else {
            aur.push(update);
        }
    }

    config
        .raur
        .as_ref()
        .unwrap()
        .cache_info(cache, &aur)
        .await?;
    let aur = aur
        .iter()
        .map(|u| pkgbases.remove(u.as_str()).unwrap())
        .collect::<Vec<_>>();

    if config.mode.aur() {
        for p in aur.iter().flatten() {
            let name = p.name();
            if !cache.contains(name) {
                continue;
            }
            if p.should_ignore() || config.ignore_devel.is_match(name) {
                ignored.push(name.to_string());
            } else {
                targets.push(Target::new(
                    Some(config.aur_namespace().to_string()),
                    name.to_string(),
                ));
            }
        }
    }
    Ok((targets, ignored))
}

pub async fn pkg_has_update<'pkg>(
    config: &'_ Config,
    pkg: &'pkg str,
    info: &'_ HashSet<RepoInfo>,
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

        // TODO dont do this
        let arch = config.alpm.architectures().first().unwrap_or_default();
        for url in srcinfo.base.source.arch(arch) {
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
    let devel_info = DevelInfo::deserialize(toml::Deserializer::parse(&file)?)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LocalRepos, Mode};
    use std::os::unix::fs::PermissionsExt;

    fn ignored_fixture(
        ignore_pkg: &[&str],
        ignore_devel: &[&str],
        remote_commit: &str,
    ) -> (tempfile::TempDir, Config) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("db");
        create_dir_all(db.join("local")).unwrap();
        std::fs::write(db.join("local/ALPM_DB_VERSION"), "9\n").unwrap();

        for name in ["split-a", "split-b"] {
            let pkg_dir = db.join("local").join(format!("{}-1-1", name));
            create_dir_all(&pkg_dir).unwrap();
            std::fs::write(
                pkg_dir.join("desc"),
                format!("%NAME%\n{name}\n\n%VERSION%\n1-1\n\n%BASE%\nsplit\n\n%ARCH%\nany\n\n"),
            )
            .unwrap();
        }

        let mut config = Config::default();
        config.raur = Some(Default::default());
        config.mode = Mode::AUR;
        config.pacman.root_dir = "/".into();
        config.pacman.db_path = db.to_str().unwrap().into();
        config.pacman.gpg_dir = dir.path().to_str().unwrap().into();
        config.pacman.log_file = dir.path().join("pacman.log").to_str().unwrap().into();
        config.ignore = ignore_pkg.iter().map(|s| s.to_string()).collect();
        let mut globs = globset::GlobSetBuilder::new();
        for pattern in ignore_devel {
            globs.add(globset::Glob::new(pattern).unwrap());
        }
        config.ignore_devel = globs.build().unwrap();
        config.init_alpm().unwrap();

        config.state_dir = dir.path().to_path_buf();
        config.devel_path = dir.path().join("devel.toml");
        std::fs::write(
            &config.devel_path,
            "[[split]]\nurl = 'https://example.invalid/repo.git'\ncommit = 'before'\n",
        )
        .unwrap();

        let git = dir.path().join("git");
        std::fs::write(
            &git,
            "#!/bin/sh\nprintf 'called\\n' >> \"$1\"\nprintf '%s\\tHEAD\\n' \"$2\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o755)).unwrap();
        config.git_bin = git.to_str().unwrap().into();
        config.git_flags = vec![
            dir.path().join("git-called").to_str().unwrap().into(),
            remote_commit.into(),
        ];

        (dir, config)
    }

    fn split_cache() -> Cache {
        ["split", "split-a", "split-b"]
            .into_iter()
            .map(|name| {
                raur::Package {
                    name: name.into(),
                    package_base: "split".into(),
                    ..Default::default()
                }
                .into()
            })
            .collect()
    }

    #[tokio::test]
    async fn fully_ignored_devel_checks_are_skipped_without_git() {
        for (ignore_pkg, ignore_devel) in [
            (vec!["split-a", "split-b"], vec![]),
            (vec![], vec!["split-*"]),
            (vec!["split-a"], vec!["split-b"]),
        ] {
            let (dir, config) = ignored_fixture(&ignore_pkg, &ignore_devel, "after");
            let result = possible_devel_updates(&config).await.unwrap();

            assert!(result.updates.is_empty());
            assert_eq!(result.skipped, ["split-a", "split-b"]);
            assert!(!dir.path().join("git-called").exists());
            let (updates, ignored) =
                filter_devel_updates(&config, &mut split_cache(), &result.updates)
                    .await
                    .unwrap();
            assert!(updates.is_empty());
            assert!(ignored.is_empty());
        }
    }

    #[tokio::test]
    async fn partially_ignored_split_update_reports_only_confirmed_updates() {
        for (ignore_pkg, ignore_devel) in [(vec!["split-a"], vec![]), (vec![], vec!["split-a"])] {
            let (dir, config) = ignored_fixture(&ignore_pkg, &ignore_devel, "after");
            let result = possible_devel_updates(&config).await.unwrap();

            assert_eq!(result.updates, ["split"]);
            assert!(result.skipped.is_empty());
            assert!(dir.path().join("git-called").exists());
            let (updates, ignored) =
                filter_devel_updates(&config, &mut split_cache(), &result.updates)
                    .await
                    .unwrap();
            assert_eq!(updates.len(), 1);
            assert_eq!(updates[0].pkg, "split-b");
            assert_eq!(ignored, ["split-a"]);
        }
    }

    #[tokio::test]
    async fn unchanged_split_base_does_not_report_ignored_upgrades() {
        let (dir, config) = ignored_fixture(&[], &["split-a"], "before");
        let result = possible_devel_updates(&config).await.unwrap();

        assert!(result.updates.is_empty());
        assert!(result.skipped.is_empty());
        assert!(dir.path().join("git-called").exists());
        let (updates, ignored) = filter_devel_updates(&config, &mut split_cache(), &result.updates)
            .await
            .unwrap();
        assert!(updates.is_empty());
        assert!(ignored.is_empty());
    }

    #[tokio::test]
    async fn excluded_devel_bases_are_not_reported_as_skipped() {
        let (dir, mut config) = ignored_fixture(&[], &["split-*"], "after");
        config.repos = LocalRepos::Repo(vec!["unrelated".into()]);
        let result = possible_devel_updates(&config).await.unwrap();
        assert!(result.updates.is_empty());
        assert!(result.skipped.is_empty());
        assert!(!dir.path().join("git-called").exists());

        config.repos = LocalRepos::None;
        config.mode = Mode::PKGBUILD;
        let result = possible_devel_updates(&config).await.unwrap();
        assert!(result.updates.is_empty());
        assert!(result.skipped.is_empty());
        assert!(!dir.path().join("git-called").exists());
    }

    #[tokio::test]
    async fn local_repo_split_base_uses_package_members_for_scope() {
        let (dir, mut config) = ignored_fixture(&[], &["split-*"], "after");
        let db = dir.path().join("db");
        create_dir_all(db.join("sync")).unwrap();
        let status = std::process::Command::new("tar")
            .arg("-czf")
            .arg(db.join("sync/devel-repo.db"))
            .arg("-C")
            .arg(db.join("local"))
            .args(["split-a-1-1", "split-b-1-1"])
            .status()
            .unwrap();
        assert!(status.success());
        let mut repo = pacmanconf::Repository::default();
        repo.name = "devel-repo".into();
        repo.servers = vec![format!("file://{}", dir.path().display())];
        config.pacman.repos.push(repo);
        config.repos = LocalRepos::Repo(vec!["devel-repo".into()]);
        config.init_alpm().unwrap();

        let result = possible_devel_updates(&config).await.unwrap();
        assert!(result.updates.is_empty());
        assert_eq!(result.skipped, ["split-a", "split-b"]);
        assert!(!dir.path().join("git-called").exists());

        config.repos = LocalRepos::None;
        let result = possible_devel_updates(&config).await.unwrap();
        assert!(result.updates.is_empty());
        assert!(result.skipped.is_empty());
        assert!(!dir.path().join("git-called").exists());
    }

    #[tokio::test]
    async fn custom_split_base_respects_mode_and_routes_package_members() {
        let (dir, mut config) = ignored_fixture(&["split-a"], &[], "after");
        let path = dir.path().join("custom");
        create_dir_all(&path).unwrap();
        std::fs::write(path.join("PKGBUILD"), "").unwrap();
        std::fs::write(
            path.join(".SRCINFO"),
            "pkgbase = split\n\tpkgver = 1\n\tpkgrel = 1\n\tarch = any\npkgname = split-a\npkgname = split-b\n",
        )
        .unwrap();
        config
            .pkgbuild_repos
            .add_repo("custom".into())
            .source
            .set_path(path);

        let result = possible_devel_updates(&config).await.unwrap();
        assert!(result.updates.is_empty());
        assert!(result.skipped.is_empty());
        assert!(!dir.path().join("git-called").exists());

        config.mode = Mode::PKGBUILD;
        let result = possible_devel_updates(&config).await.unwrap();
        assert_eq!(result.updates, ["split"]);
        assert!(result.skipped.is_empty());
        assert!(dir.path().join("git-called").exists());
        let mut cache = Cache::new();
        let (updates, ignored) = filter_devel_updates(&config, &mut cache, &result.updates)
            .await
            .unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].repo.as_deref(), Some("custom"));
        assert_eq!(updates[0].pkg, "split-b");
        assert_eq!(ignored, ["split-a"]);
        assert!(cache.is_empty());

        config.mode = Mode::AUR;
        let (updates, ignored) = filter_devel_updates(&config, &mut cache, &result.updates)
            .await
            .unwrap();
        assert!(updates.is_empty());
        assert!(ignored.is_empty());
        assert!(cache.is_empty());
    }

    #[tokio::test]
    async fn ls_remote_does_not_parse_remote_as_option() {
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/git-repo");
        let commit = ls_remote_internal("git", &[], repo.to_str().unwrap(), None)
            .await
            .unwrap();
        assert_eq!(commit, "2545915d2a057d068a8585ebfc0592ba543189a7");

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing-repo");

        let result = ls_remote_internal("git", &[], "--get-url", path.to_str()).await;

        assert!(result.is_err(), "remote was interpreted as an option");
    }
}
