use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::env::var;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs::{read_dir, read_link, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;

use crate::args::{Arg, Args};
use crate::chroot::Chroot;
use crate::clean::clean_untracked;
use crate::completion::update_aur_cache;
use crate::config::{Config, LocalRepos, Mode, Op, Sign, YesNoAllTree, YesNoAsk};
use crate::devel::{fetch_devel_info, load_devel_info, save_devel_info, DevelInfo};
use crate::download::{self, Bases};
use crate::fmt::{print_indent, print_install, print_install_verbose};
use crate::keys::check_pgp_keys;
use crate::pkgbuild::PkgbuildRepo;
use crate::resolver::{flags, resolver};
use crate::upgrade::{get_upgrades, Upgrades};
use crate::util::{ask, repo_aur_pkgs, split_repo_aur_targets};
use crate::{args, exec, news, print_error, printtr, repo};

use alpm::{Alpm, Depend, Version};
use alpm_utils::depends::{satisfies, satisfies_nover, satisfies_provide, satisfies_provide_nover};
use alpm_utils::{DbListExt, Targ};
use ansi_term::Style;
use anyhow::{bail, ensure, Context, Result};
use aur_depends::{Actions, Base, Conflict, DepMissing, RepoPackage};
use log::debug;
use raur::Cache;
use srcinfo::{ArchVec, Srcinfo};
use tr::tr;

#[derive(Copy, Clone, Debug)]
pub struct Status(pub i32);

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Status: {}", self.0)
    }
}

impl std::error::Error for Status {}

impl Status {
    pub fn err(n: i32) -> Result<()> {
        bail!(Status(n))
    }
}

struct Installer {
    install_targets: bool,
    done_something: bool,
    ran_pacman: bool,
    upgrades: Upgrades,
    srcinfos: HashMap<String, Srcinfo>,
    remove_make: Vec<String>,
    conflicts: HashSet<String>,
    failed: Vec<Base>,
    chroot: Chroot,
    deps: Vec<String>,
    exp: Vec<String>,
    install_queue: Vec<String>,
    conflict: bool,
    devel_info: DevelInfo,
    new_devel_info: DevelInfo,
    built: Vec<String>,
}

pub async fn install(config: &mut Config, targets_str: &[String]) -> Result<()> {
    let mut installer = Installer::new(config);
    installer.install_targets = !config.no_install;
    installer.install(config, targets_str).await
}

pub async fn build_dirs(config: &mut Config, dirs: Vec<PathBuf>) -> Result<()> {
    let mut installer = Installer::new(config);
    let repo = PkgbuildRepo::from_pkgbuilds(config, &dirs)?;

    let targets = repo
        .pkgs(config)
        .iter()
        .flat_map(|s| s.srcinfo.names())
        .map(|name| format!("./{}", name))
        .collect::<Vec<_>>();

    config.pkgbuild_repos.repos.push(repo);
    installer.install_targets = config.install;
    installer.install(config, &targets).await
}

impl Installer {
    fn new(config: &Config) -> Self {
        let mut fetch = config.fetch.clone();
        fetch.clone_dir = fetch.clone_dir.join("repo");
        fetch.diff_dir = fetch.diff_dir.join("repo");

        Self {
            install_targets: true,
            done_something: false,
            ran_pacman: false,
            upgrades: Upgrades::default(),
            srcinfos: HashMap::new(),
            remove_make: Vec::new(),
            conflicts: HashSet::new(),
            failed: Vec::new(),
            chroot: chroot(config),
            deps: Vec::new(),
            exp: Vec::new(),
            install_queue: Vec::new(),
            conflict: false,
            devel_info: DevelInfo::default(),
            new_devel_info: DevelInfo::default(),
            built: Vec::new(),
        }
    }

    fn early_refresh(&self, config: &Config) -> Result<()> {
        let mut args = config.pacman_globals();
        for _ in 0..config.args.count("y", "refresh") {
            args.arg("y");
        }
        args.targets.clear();
        exec::pacman(config, &args)?.success()?;
        Ok(())
    }

    fn early_pacman(&mut self, config: &Config, targets: Vec<String>) -> Result<()> {
        let mut args = config.pacman_args();
        args.targets.clear();
        args.targets(targets.iter().map(|i| i.as_str()));
        exec::pacman(config, &args)?.success()?;
        Ok(())
    }

    fn sudo_loop(&self, config: &Config) -> Result<()> {
        if !config.sudo_loop.is_empty() {
            let mut flags = config.sudo_flags.clone();
            flags.extend(config.sudo_loop.clone());
            exec::spawn_sudo(config.sudo_bin.clone(), flags)?;
        }
        Ok(())
    }

    async fn news(&self, config: &Config) -> Result<()> {
        let c = config.color;

        if config.news_on_upgrade && config.args.has_arg("u", "sysupgrade") {
            let mut ret = 0;
            match news::news(config).await {
                Ok(v) => ret = v,
                Err(err) => eprintln!(
                    "{} {}: {}",
                    c.error.paint(tr!("error:")),
                    tr!("could not get news",),
                    err
                ),
            }

            if ret != 1 && !ask(config, &tr!("Proceed with installation?"), true) {
                return Status::err(1);
            }
        }

        Ok(())
    }

    async fn download_pkgbuilds(&mut self, config: &Config, bases: &Bases) -> Result<()> {
        for base in &bases.bases {
            let path = config.build_dir.join(base.package_base()).join(".SRCINFO");
            if path.exists() {
                let srcinfo = Srcinfo::parse_file(path);
                if let Ok(srcinfo) = srcinfo {
                    self.srcinfos
                        .insert(srcinfo.base.pkgbase.to_string(), srcinfo);
                }
            }
        }

        download::new_aur_pkgbuilds(config, bases, &self.srcinfos).await?;

        for base in &bases.bases {
            if self.srcinfos.contains_key(base.package_base()) {
                continue;
            }
            let path = config.build_dir.join(base.package_base()).join(".SRCINFO");
            if path.exists() {
                if let Entry::Vacant(vacant) = self.srcinfos.entry(base.package_base().to_string())
                {
                    let srcinfo = Srcinfo::parse_file(path)
                        .with_context(|| tr!("failed to parse srcinfo for '{}'", base))?;
                    vacant.insert(srcinfo);
                }
            } else {
                bail!(tr!("could not find .SRCINFO for '{}'", base.package_base()));
            }
        }
        Ok(())
    }

    fn chroot_install(&self, config: &Config, build: &[Base], repo_targs: &[String]) -> Result<()> {
        if !config.chroot {
            return Ok(());
        }

        if !config.args.has_arg("w", "downloadonly") {
            let mut targets = Vec::new();

            let iter = build.iter().filter(|b| {
                !self
                    .failed
                    .iter()
                    .any(|f| b.package_base() == f.package_base())
            });

            for base in iter {
                match base {
                    Base::Aur(base) => {
                        for pkg in &base.pkgs {
                            if pkg.target && self.install_targets {
                                targets.push(pkg.pkg.name.as_str())
                            }
                        }
                    }
                    Base::Pkgbuild(base) => {
                        for pkg in &base.pkgs {
                            if pkg.target && self.install_targets {
                                targets.push(pkg.pkg.pkgname.as_str())
                            }
                        }
                    }
                }
            }

            if config.args.has_arg("u", "sysupgrade") {
                targets.retain(|&p| config.alpm.localdb().pkg(p).is_ok());
            }

            targets.extend(repo_targs.iter().map(|s| s.as_str()));

            let mut args = config.pacman_globals();
            args.op("sync");
            copy_sync_args(config, &mut args);
            if config.args.has_arg("asexplicit", "asexp") {
                args.arg("asexplicit");
            } else if config.args.has_arg("asdeps", "asdep") {
                args.arg("asdeps");
            }

            if config.mode.repo() {
                for _ in 0..config.args.count("y", "refresh") {
                    args.arg("y");
                }
                for _ in 0..config.args.count("u", "sysupgrade") {
                    args.arg("u");
                }
            }

            args.targets = targets;

            if !self.conflict
                && !self.built.is_empty()
                && (!config.args.has_arg("u", "sysupgrade")
                    || config.combined_upgrade
                    || !config.mode.repo())
            {
                args.arg("noconfirm");
            }

            if !args.targets.is_empty()
                || args.has_arg("u", "sysupgrade")
                || args.has_arg("y", "refresh")
            {
                exec::pacman(config, &args)?.success()?;
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn dep_or_exp(
        &mut self,
        config: &Config,
        base: &Base,
        version: &str,
        pkg: &str,
        target: bool,
        make: bool,
        pkgdest: &mut HashMap<String, String>,
    ) -> Result<()> {
        if !needs_install(config, base, version, pkg) {
            return Ok(());
        }

        let assume = if config.chroot {
            // TODO handle nover
            config
                .assume_installed
                .iter()
                .map(|a| Depend::new(a.as_str()))
                .any(|assume| satisfies_provide(Depend::new(pkg), assume))
        } else {
            false
        };

        if config.chroot && (make || assume) {
            return Ok(());
        }

        if config.args.has_arg("asexplicit", "asexp") {
            self.exp.push(pkg.to_string());
        } else if config.args.has_arg("asdeps", "asdeps") {
            self.deps.push(pkg.to_string());
        } else if config.alpm.localdb().pkg(pkg).is_err() {
            if target {
                self.exp.push(pkg.to_string())
            } else {
                self.deps.push(pkg.to_string())
            }
        }

        let path = pkgdest.remove(pkg).with_context(|| {
            tr!(
                "could not find package '{pkg}' in package list for '{base}'",
                pkg = pkg,
                base = base
            )
        })?;

        self.conflict |= self.conflicts.contains(pkg);
        self.install_queue.push(path);
        Ok(())
    }

    fn do_install(&mut self, config: &Config) -> Result<()> {
        if !self.install_queue.is_empty() {
            let mut args = config.pacman_globals();
            let ask;
            args.op("upgrade");
            copy_sync_args(config, &mut args);

            for _ in 0..args.count("d", "nodeps") {
                args.arg("d");
            }

            if self.conflict {
                if config.use_ask {
                    if let Some(arg) = args.args.iter_mut().find(|a| a.key == "ask") {
                        let num = arg.value.unwrap_or_default();
                        let mut num = num.parse::<i32>().unwrap_or_default();
                        num |= alpm::QuestionType::ConflictPkg as i32;
                        ask = num.to_string();
                        arg.value = Some(ask.as_str());
                    } else {
                        let value = alpm::QuestionType::ConflictPkg as i32;
                        ask = value.to_string();
                        args.push_value("ask", ask.as_str());
                    }
                }
            } else {
                args.arg("noconfirm");
            }

            debug!("flushing install queue");
            args.targets = self.install_queue.iter().map(|s| s.as_str()).collect();
            exec::pacman(config, &args)?.success()?;

            if config.devel {
                save_devel_info(config, &self.devel_info)?;
            }

            asdeps(config, &self.deps)?;
            asexp(config, &self.exp)?;
            self.deps.clear();
            self.exp.clear();
            self.install_queue.clear();
        }
        Ok(())
    }

    fn build_cleanup(&self, config: &Config, build: &[Base]) -> Result<()> {
        let mut ret = 0;

        if !self.remove_make.is_empty() {
            let mut args = config.pacman_globals();
            args.op("remove").arg("noconfirm");
            args.targets = self.remove_make.iter().map(|s| s.as_str()).collect();

            if let Err(err) = exec::pacman(config, &args) {
                print_error(config.color.error, err);
                ret = 1;
            }
        }

        if config.clean_after {
            for base in build {
                let path = config.build_dir.join(base.package_base());
                if let Err(err) = clean_untracked(config, &path) {
                    print_error(config.color.error, err);
                    ret = 1;
                }
            }
        }

        if !self.failed.is_empty() {
            let failed = self
                .failed
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>();
            bail!(tr!("packages failed to build: {}", failed.join("  ")));
        }

        if ret != 0 {
            Status::err(ret)
        } else {
            Ok(())
        }
    }

    fn debug_paths(
        &mut self,
        config: &Config,
        base: &mut Base,
        pkgdest: &HashMap<String, String>,
    ) -> Result<HashMap<String, String>> {
        let mut debug_paths = HashMap::new();

        if config.install_debug {
            for dest in pkgdest.values() {
                let file = dest.rsplit('/').next().unwrap();

                match base {
                    Base::Aur(base) => {
                        let mut debug = Vec::new();
                        for pkg in &base.pkgs {
                            if pkg.make {
                                continue;
                            }
                            let debug_pkg = format!("{}-debug-", pkg.pkg.name);

                            if file.starts_with(&debug_pkg) {
                                let debug_pkg = format!("{}-debug", pkg.pkg.name);
                                let mut pkg = pkg.clone();
                                let mut raur_pkg = (*pkg.pkg).clone();
                                raur_pkg.name = debug_pkg;
                                pkg.pkg = raur_pkg.into();
                                debug_paths.insert(pkg.pkg.name.clone(), dest.clone());
                                debug.push(pkg);
                            }
                        }
                        base.pkgs.extend(debug);
                    }
                    Base::Pkgbuild(base) => {
                        let mut debug = Vec::new();
                        for pkg in &base.pkgs {
                            if pkg.make {
                                continue;
                            }
                            let debug_pkg = format!("{}-debug-", pkg.pkg.pkgname);

                            if file.starts_with(&debug_pkg) {
                                let debug_pkg = format!("{}-debug", pkg.pkg.pkgname);
                                let mut pkg = pkg.clone();
                                pkg.pkg.pkgname = debug_pkg;
                                debug_paths.insert(pkg.pkg.pkgname.clone(), dest.clone());
                                debug.push(pkg);
                            }
                        }
                        base.pkgs.extend(debug);
                    }
                }
            }
        }

        for (pkg, path) in &debug_paths {
            if !Path::new(path).exists() {
                match base {
                    Base::Aur(base) => base.pkgs.retain(|p| p.pkg.name != *pkg),
                    Base::Pkgbuild(base) => base.pkgs.retain(|p| p.pkg.pkgname != *pkg),
                }
            } else {
                printtr!("adding {} to the install list", pkg);
            }
        }

        Ok(debug_paths)
    }

    // TODO: sort out args
    fn build_pkgbuild(
        &mut self,
        config: &mut Config,
        base: &mut Base,
        repo: Option<(&str, &str)>,
        dir: &Path,
    ) -> Result<(HashMap<String, String>, String)> {
        let c = config.color;
        let pkgdest = repo.map(|r| r.1);
        let mut env = config.env.clone();
        env.extend(pkgdest.map(|p| ("PKGDEST".to_string(), p.to_string())));

        if config.chroot {
            let mut extra = Vec::new();
            if config.repos == LocalRepos::None {
                extra.extend(self.built.iter().map(|s| s.as_str()));
            }
            self.chroot
                .build(dir, &extra, &["-cu"], &["-ofA"], &config.env)
                .with_context(|| tr!("failed to download sources for '{}'"))?;
        } else {
            // download sources
            let mut args = vec!["--verifysource", "-Af"];
            if !config.keep_src {
                args.push("-Cc");
            }
            exec::makepkg(config, dir, &args)?
                .success()
                .with_context(|| tr!("failed to download sources for '{}'", base))?;

            // pkgver bump
            let mut args = vec!["-ofA"];
            if !config.keep_src {
                args.push("-C");
            }
            exec::makepkg(config, dir, &args)?
                .success()
                .with_context(|| tr!("failed to build '{}'", base))?;
        }

        printtr!("{}: parsing pkg list...", base);
        let (pkgdests, version) = parse_package_list(config, dir, pkgdest)?;

        if !base.packages().all(|p| pkgdests.contains_key(p)) {
            bail!(tr!("package list does not match srcinfo"));
        }

        let debug_paths = self.debug_paths(config, base, &pkgdests)?;

        let needs_build = needs_build(config, base, &pkgdests, &version);
        if needs_build {
            // actual build
            if config.chroot {
                let mut extra = Vec::new();
                if config.repos == LocalRepos::None {
                    extra.extend(self.built.iter().map(|s| s.as_str()));
                }
                self.chroot
                    .build(
                        dir,
                        &extra,
                        &[],
                        &["-feA", "--noconfirm", "--noprepare", "--holdver"],
                        &env,
                    )
                    .with_context(|| tr!("failed to build '{}'", base))?;
            } else {
                let mut args = vec!["-feA", "--noconfirm", "--noprepare", "--holdver"];
                if !config.keep_src {
                    args.push("-c");
                }
                exec::makepkg_dest(config, dir, &args, pkgdest)?
                    .success()
                    .with_context(|| tr!("failed to build '{}'", base))?;
            }
        } else {
            println!(
                "{} {}",
                c.warning.paint("::"),
                tr!(
                    "{}-{} is up to date -- skipping build",
                    base.package_base(),
                    base.version()
                )
            )
        }

        self.add_pkg(config, base, repo, &pkgdests, &debug_paths)?;
        self.queue_install(base, &pkgdests, &debug_paths);
        Ok((pkgdests, version))
    }

    fn queue_install(
        &mut self,
        base: &mut Base,
        pkgdest: &HashMap<String, String>,
        debug_paths: &HashMap<String, String>,
    ) {
        let to_install: Vec<_> = match base {
            Base::Aur(a) => a
                .pkgs
                .iter()
                .filter(|a| !a.make)
                .map(|a| a.pkg.name.as_str())
                .collect(),
            Base::Pkgbuild(c) => c
                .pkgs
                .iter()
                .filter(|c| !c.make)
                .map(|a| a.pkg.pkgname.as_str())
                .collect(),
        };

        let to_install = to_install
            .iter()
            .filter_map(|p| pkgdest.get(*p))
            .chain(debug_paths.values())
            .cloned();

        self.built.extend(to_install);
    }

    fn add_pkg(
        &mut self,
        config: &mut Config,
        base: &Base,
        repo: Option<(&str, &str)>,
        pkgdest: &HashMap<String, String>,
        debug_paths: &HashMap<String, String>,
    ) -> Result<()> {
        let paths = base
            .packages()
            .filter_map(|p| pkgdest.get(p))
            .chain(debug_paths.values())
            .map(|s| s.as_str())
            .collect::<Vec<_>>();
        sign_pkg(config, &paths, false)?;

        if let Some(ref repo) = repo {
            if let Some(repo) = self.upgrades.aur_repos.get(base.package_base()) {
                let repo = config
                    .alpm
                    .syncdbs()
                    .iter()
                    .find(|db| db.name() == *repo)
                    .unwrap();
                let path = repo::file(&repo).unwrap();
                let name = repo.name().to_string();
                repo::add(config, path, &name, &paths)?;
                repo::refresh(config, &[name])?;
            } else {
                let path = repo.1;
                repo::add(config, path, repo.0, &paths)?;
                repo::refresh(config, &[repo.0])?;
            }
            if let Some(info) = self.new_devel_info.info.remove(base.package_base()) {
                self.devel_info
                    .info
                    .insert(base.package_base().to_string(), info);
            } else {
                self.devel_info.info.remove(base.package_base());
            }
            if config.devel {
                save_devel_info(config, &self.devel_info)?;
            }
        }

        Ok(())
    }

    fn build_install_pkgbuild(
        &mut self,
        config: &mut Config,
        base: &mut Base,
        repo: Option<(&str, &str)>,
    ) -> Result<()> {
        let dir = match base {
            Base::Aur(_) => config.build_dir.join(base.package_base()),
            Base::Pkgbuild(c) => config
                .pkgbuild_repos
                .repo(&c.repo)
                .unwrap()
                .base(config, &c.srcinfo.base.pkgbase)
                .unwrap()
                .path
                .clone(),
        };

        let pkgdest = repo.map(|r| r.1);
        let build = base.build();

        if !config.chroot
            && (!config.batch_install || !deps_not_satisfied(config, base)?.is_empty())
        {
            self.do_install(config)?;
            self.conflict = false;
        }

        let mut missing = if config.args.count("d", "nodeps") > 1 {
            Vec::new()
        } else if config.chroot {
            if config.repos == LocalRepos::None {
                // todo
                Vec::new()
            } else {
                deps_not_satisfied_by_repo(config, base)?
            }
        } else {
            deps_not_satisfied(config, base)?
        };

        let ver = config.args.count("d", "nodeps") == 0;
        let arch = config.alpm.architectures().first().unwrap_or_default();

        match &*base {
            Base::Aur(a) => {
                for pkg in &a.pkgs {
                    missing.retain(|mis| {
                        let provides = pkg.pkg.provides.iter().map(|p| Depend::new(p.as_str()));
                        let v = Version::new(pkg.pkg.version.as_str());
                        if ver {
                            !satisfies(Depend::new(*mis), &pkg.pkg.name, v, provides)
                        } else {
                            !satisfies_nover(Depend::new(*mis), &pkg.pkg.name, provides)
                        }
                    })
                }
            }
            Base::Pkgbuild(c) => {
                for pkg in &c.pkgs {
                    missing.retain(|mis| {
                        let provides = ArchVec::supported(&pkg.pkg.provides, arch).map(Depend::new);
                        let v = Version::new(c.version().as_str());
                        if ver {
                            !satisfies(Depend::new(*mis), &pkg.pkg.pkgname, v, provides)
                        } else {
                            !satisfies_nover(Depend::new(*mis), &pkg.pkg.pkgname, provides)
                        }
                    })
                }
            }
        }

        if !missing.is_empty() {
            bail!(tr!(
                "can't build {base}, deps not satisfied: {deps}",
                base = base,
                deps = missing.join("  ")
            ));
        }

        let (mut pkgdest, version) = if build {
            self.build_pkgbuild(config, base, repo, &dir)?
        } else {
            printtr!("{}: parsing pkg list...", base);
            let (pkgdests, version) = parse_package_list(config, &dir, pkgdest)?;
            let debug_paths = self.debug_paths(config, base, &pkgdests)?;
            self.add_pkg(config, base, repo, &pkgdests, &debug_paths)?;
            self.queue_install(base, &pkgdests, &debug_paths);
            (pkgdests, version)
        };

        match &*base {
            Base::Aur(b) => {
                for pkg in &b.pkgs {
                    if pkg.target && !self.install_targets {
                        continue;
                    }
                    self.dep_or_exp(
                        config,
                        base,
                        &version,
                        &pkg.pkg.name,
                        pkg.target,
                        pkg.make,
                        &mut pkgdest,
                    )?
                }
            }
            Base::Pkgbuild(b) => {
                for pkg in &b.pkgs {
                    if pkg.target && !self.install_targets {
                        continue;
                    }
                    self.dep_or_exp(
                        config,
                        base,
                        &version,
                        &pkg.pkg.pkgname,
                        pkg.target,
                        pkg.make,
                        &mut pkgdest,
                    )?
                }
            }
        }

        if repo.is_none() {
            if let Some(info) = self.new_devel_info.info.remove(base.package_base()) {
                self.devel_info
                    .info
                    .insert(base.package_base().to_string(), info);
            } else {
                self.devel_info.info.remove(base.package_base());
            }
        }

        Ok(())
    }

    async fn build_install_pkgbuilds(
        &mut self,
        config: &mut Config,
        build: &mut [Base],
    ) -> Result<()> {
        if config.devel {
            printtr!("fetching devel info...");
            self.devel_info = load_devel_info(config)?.unwrap_or_default();
            self.new_devel_info = fetch_devel_info(config, build, &self.srcinfos).await?;
        }

        let (_, repo) = repo::repo_aur_dbs(config);
        let default_repo = repo.first();
        if let Some(repo) = default_repo {
            let file = repo::file(&repo).unwrap();
            repo::init(config, file, repo.name())?;
        }

        if config.chroot {
            if !self.chroot.exists() {
                self.chroot.create(config, &["base-devel"])?;
            } else {
                self.chroot.update()?;
            }
        }

        let repo_server =
            default_repo.map(|r| (r.name().to_string(), repo::file(&r).unwrap().to_string()));
        drop(repo);

        for base in build {
            self.failed.push(base.clone());
            let repo_server = repo_server
                .as_ref()
                .map(|rs| (rs.0.as_str(), rs.1.as_str()));

            let err = self.build_install_pkgbuild(config, base, repo_server);

            match err {
                Ok(_) => {
                    self.failed.pop().unwrap();
                }
                Err(e) => {
                    if config.fail_fast {
                        self.failed.pop().unwrap();
                        return Err(e);
                    }
                    print_error(config.color.error, e);
                }
            }
        }

        if !config.chroot {
            self.do_install(config)?;
        }

        Ok(())
    }

    pub async fn install(&mut self, config: &mut Config, targets_str: &[String]) -> Result<()> {
        self.sudo_loop(config)?;
        self.news(config).await?;

        config.set_op_args_globals(Op::Sync);
        config.targets = targets_str.to_vec();
        config.args.targets = config.targets.clone();

        let targets = args::parse_targets(targets_str);
        let (mut repo_targets, aur_targets) = split_repo_aur_targets(config, &targets)?;

        if targets_str.is_empty()
            && !config.args.has_arg("u", "sysupgrade")
            && !config.args.has_arg("y", "refresh")
        {
            bail!(tr!("no targets specified (use -h for help)"));
        }

        if config.mode.repo() {
            if config.combined_upgrade {
                if config.args.has_arg("y", "refresh") {
                    self.early_refresh(config)?;
                }
            } else if !config.chroot
                && (config.args.has_arg("y", "refresh")
                    || config.args.has_arg("u", "sysupgrade")
                    || !repo_targets.is_empty()
                    || config.mode == Mode::REPO)
            {
                let targets = repo_targets.iter().map(|t| t.to_string()).collect();
                repo_targets.clear();
                self.done_something = true;
                self.ran_pacman = true;
                self.early_pacman(config, targets)?;
            }
        }

        if targets_str.is_empty()
            && !config.args.has_arg("u", "sysupgrade")
            && !config.args.has_arg("y", "refresh")
        {
            return Ok(());
        }

        config.init_alpm()?;

        if config.args.has_arg("y", "refresh") {
            config.pkgbuild_repos.refresh(config)?;
            self.done_something = true;
        }
        self.resolve_targets(config, &repo_targets, &aur_targets)
            .await
    }

    async fn resolve_targets<'a>(
        &mut self,
        config: &mut Config,
        repo_targets: &[Targ<'a>],
        aur_targets: &[Targ<'a>],
    ) -> Result<()> {
        let mut cache = Cache::new();
        let flags = flags(config);
        let c = config.color;

        let repos = config.pkgbuild_repos.clone();
        let repos = repos.aur_depends_repo(config);
        let mut resolver = resolver(config, &config.alpm, &config.raur, &mut cache, repos, flags);

        if config.args.has_arg("u", "sysupgrade") {
            // TODO?
            let upgrades = get_upgrades(config, &mut resolver).await?;
            for pkg in &upgrades.repo_skip {
                let arg = Arg {
                    key: "ignore".to_string(),
                    value: Some(pkg.to_string()),
                };

                config.args.args.push(arg);
            }
            self.upgrades = upgrades;
        }

        let mut targets = repo_targets.to_vec();
        targets.extend(aur_targets);
        targets.extend(self.upgrades.aur_keep.iter().map(|p| Targ {
            repo: Some(config.aur_namespace()),
            pkg: p,
        }));
        targets.extend(self.upgrades.pkgbuild_keep.iter().map(|p| Targ {
            repo: Some(&p.0),
            pkg: &p.1,
        }));

        targets.extend(self.upgrades.repo_keep.iter().map(Targ::from));

        if Self::shoud_just_pacman(
            config.mode,
            &config.args,
            aur_targets,
            &self.upgrades,
            self.ran_pacman,
        ) {
            print_warnings(config, &cache, None);
            let mut args = config.pacman_args();
            let targets = targets.iter().map(|t| t.to_string()).collect::<Vec<_>>();
            args.targets = targets.iter().map(|s| s.as_str()).collect();

            if config.combined_upgrade {
                args.remove("y").remove("refresh");
            }
            if !args.targets.is_empty()
                || args.has_arg("u", "sysupgrade")
                || args.has_arg("y", "refresh")
            {
                let code = exec::pacman(config, &args)?.code();
                return Status::err(code);
            }

            return Ok(());
        }

        if targets.is_empty() && !upgrade_later(config) {
            print_warnings(config, &cache, None);
            if !self.done_something || config.args.has_arg("u", "sysupgrade") {
                printtr!(" there is nothing to do");
            }
            return Ok(());
        }

        println!(
            "{} {}",
            c.action.paint("::"),
            c.bold.paint(tr!("Resolving dependencies..."))
        );

        let mut actions = resolver.resolve_targets(&targets).await?;
        debug!("{:#?}", actions);
        let repo_targs = actions
            .install
            .iter()
            .filter(|p| p.target)
            .map(|p| p.pkg.name().to_string())
            .collect::<Vec<_>>();

        self.prepare_build(config, &cache, &mut actions).await?;

        let mut build = actions.build;

        let mut err = Ok(());

        if !build.is_empty() {
            err = self.build_install_pkgbuilds(config, &mut build).await;
        }

        if err.is_ok() && config.chroot {
            if config.repos == LocalRepos::None {
                err = self.chroot_install(config, &[], &repo_targs);
                self.do_install(config)?;
            } else {
                err = self.chroot_install(config, &build, &repo_targs);
            }
        }

        self.build_cleanup(config, &build)?;
        err
    }

    fn shoud_just_pacman(
        mode: Mode,
        args: &Args<String>,
        aur_targets: &[Targ<'_>],
        upgrades: &Upgrades,
        ran_pacman: bool,
    ) -> bool {
        if !mode.aur() && !mode.pkgbuild() {
            return true;
        }
        if args.has_arg("u", "sysupgrade") || args.has_arg("y", "refresh") {
            return false;
        }
        if ran_pacman {
            return false;
        }
        aur_targets.is_empty() && upgrades.aur_keep.is_empty() && upgrades.pkgbuild_keep.is_empty()
    }

    async fn prepare_build(
        &mut self,
        config: &Config,
        cache: &Cache,
        actions: &mut Actions<'_>,
    ) -> Result<()> {
        if !actions.build.is_empty() && nix::unistd::getuid().is_root() {
            bail!(tr!("can't install AUR package as root"));
        }
        if !actions.build.is_empty() && config.args.has_arg("w", "downloadonly") {
            bail!(tr!("--downloadonly can't be used for AUR packages"));
        }

        let conflicts = check_actions(config, actions)?;
        let c = config.color;

        print_warnings(config, cache, Some(actions));

        if actions.build.is_empty() && actions.install.is_empty() {
            printtr!(" there is nothing to do");
            return Ok(());
        }

        if config.pacman.verbose_pkg_lists {
            print_install_verbose(config, actions, &self.upgrades.devel);
        } else {
            print_install(config, actions, &self.upgrades.devel);
        }

        let has_make = if !config.chroot
            && (actions.build.iter().any(|p| p.make()) || actions.install.iter().any(|p| p.make))
        {
            if config.remove_make == YesNoAsk::Ask {
                ask(
                    config,
                    &tr!("Remove make dependencies after install?"),
                    false,
                )
            } else {
                config.remove_make == YesNoAsk::Yes
            }
        } else {
            false
        };

        if !config.skip_review && actions.iter_aur_pkgs().next().is_some() {
            if !ask(config, &tr!("Proceed to review?"), true) {
                return Status::err(1);
            }
        } else if !ask(config, &tr!("Proceed with installation?"), true) {
            return Status::err(1);
        }

        if actions.build.is_empty() {
            if !config.chroot {
                repo_install(config, &actions.install)?;
            }
            return Ok(());
        }

        let bases = actions.iter_aur_pkgs().cloned().collect();
        self.download_pkgbuilds(config, &bases).await?;

        for pkg in &actions.build {
            match pkg {
                Base::Aur(base) => {
                    let dir = config.fetch.clone_dir.join(base.package_base());
                    pre_build_command(config, &dir, base.package_base(), &base.version())?;
                }
                Base::Pkgbuild(c) => {
                    let dir = &config
                        .pkgbuild_repos
                        .repo(&c.repo)
                        .unwrap()
                        .base(config, c.package_base())
                        .unwrap()
                        .path;
                    pre_build_command(config, dir, c.package_base(), &c.version())?;
                }
            }
        }

        if !config.skip_review {
            let pkgs = actions
                .build
                .iter()
                .filter(|b| b.build())
                .filter_map(|b| match b {
                    Base::Aur(pkg) => Some(pkg.package_base()),
                    Base::Pkgbuild(_) => None,
                })
                .collect::<Vec<_>>();
            review(config, &config.fetch, &pkgs)?;
        }

        let arch = config
            .alpm
            .architectures()
            .first()
            .context(tr!("no architecture"))?;

        let incompatible = self
            .srcinfos
            .values()
            .flat_map(|s| &s.pkgs)
            .filter(|p| !p.arch.iter().any(|a| a == "any") && !p.arch.iter().any(|a| a == arch))
            .collect::<Vec<_>>();

        if !incompatible.is_empty() {
            println!(
                "{} {}",
                c.error.paint("::"),
                c.bold.paint(tr!(
                    "The following packages are not compatible with your architecture:"
                ))
            );
            print!("    ");
            print_indent(
                Style::new(),
                0,
                4,
                config.cols,
                "  ",
                incompatible.iter().map(|i| i.pkgname.as_str()),
            );
            if !ask(
                config,
                &tr!("Would you like to try build them anyway?"),
                true,
            ) {
                return Status::err(1);
            }
        }

        if config.pgp_fetch {
            check_pgp_keys(config, actions, &self.srcinfos)?;
        }

        if !config.chroot {
            repo_install(config, &actions.install)?;
        } else {
            return Ok(());
        }

        update_aur_list(config);

        self.conflicts = conflicts
            .0
            .iter()
            .map(|c| c.pkg.clone())
            .chain(conflicts.1.iter().map(|c| c.pkg.clone()))
            .collect::<HashSet<_>>();

        if has_make {
            self.remove_make.extend(
                actions
                    .install
                    .iter()
                    .filter(|p| p.make)
                    .map(|p| p.pkg.name().to_string())
                    .collect::<Vec<_>>(),
            );

            self.remove_make.extend(
                actions
                    .iter_aur_pkgs()
                    .filter(|p| p.make)
                    .map(|p| p.pkg.name.clone()),
            );
            self.remove_make.extend(
                actions
                    .iter_pkgbuilds()
                    .filter(|p| p.1.make)
                    .map(|p| p.1.pkg.pkgname.clone()),
            );
        }

        Ok(())
    }
}

fn is_debug(pkg: alpm::Package) -> bool {
    if let Some(base) = pkg.base() {
        if pkg.name().ends_with("-debug") && pkg.name().trim_end_matches("-debug") == base {
            return true;
        }
    }

    false
}

fn print_warnings(config: &Config, cache: &Cache, actions: Option<&Actions>) {
    let mut warnings = crate::download::Warnings::default();

    if !config.mode.aur() && !config.mode.pkgbuild() {
        return;
    }

    if config.args.has_arg("u", "sysupgrade") && config.mode.aur() {
        let (_, pkgs) = repo_aur_pkgs(config);

        warnings.missing = pkgs
            .iter()
            .filter(|pkg| !cache.contains(pkg.name()))
            .filter(|pkg| !is_debug(**pkg))
            .map(|pkg| pkg.name())
            .filter(|pkg| !config.no_warn.is_match(pkg))
            .collect::<Vec<_>>();

        warnings.ood = pkgs
            .iter()
            .filter(|pkg| !is_debug(**pkg))
            .filter_map(|pkg| cache.get(pkg.name()))
            .filter(|pkg| pkg.out_of_date.is_some())
            .map(|pkg| pkg.name.as_str())
            .filter(|pkg| !config.no_warn.is_match(pkg))
            .collect::<Vec<_>>();

        warnings.orphans = pkgs
            .iter()
            .filter(|pkg| !is_debug(**pkg))
            .filter_map(|pkg| cache.get(pkg.name()))
            .filter(|pkg| pkg.maintainer.is_none())
            .map(|pkg| pkg.name.as_str())
            .filter(|pkg| !config.no_warn.is_match(pkg))
            .collect::<Vec<_>>();
    }

    if let Some(actions) = actions {
        warnings.ood.extend(
            actions
                .iter_aur_pkgs()
                .map(|pkg| &pkg.pkg)
                .filter(|pkg| pkg.out_of_date.is_some())
                .filter(|pkg| !config.no_warn.is_match(&pkg.name))
                .map(|pkg| pkg.name.as_str()),
        );

        warnings.orphans.extend(
            actions
                .iter_aur_pkgs()
                .map(|pkg| &pkg.pkg)
                .filter(|pkg| pkg.maintainer.is_none())
                .filter(|pkg| !config.no_warn.is_match(&pkg.name))
                .map(|pkg| pkg.name.as_str()),
        );
    }

    warnings.missing.sort_unstable();
    warnings.ood.sort_unstable();
    warnings.ood.dedup();
    warnings.orphans.sort_unstable();
    warnings.orphans.dedup();

    warnings.all(config.color, config.cols);
}

fn upgrade_later(config: &Config) -> bool {
    config.mode.repo()
        && config.chroot
        && (config.args.has_arg("u", "sysupgrade") || config.args.has_arg("y", "refresh"))
}

fn fmt_stack(want: &DepMissing) -> String {
    match &want.dep {
        Some(dep) => format!("{} ({})", want.pkg, dep),
        None => want.pkg.to_string(),
    }
}

fn check_actions(config: &Config, actions: &mut Actions) -> Result<(Vec<Conflict>, Vec<Conflict>)> {
    let c = config.color;
    let dups = actions.duplicate_targets();
    ensure!(
        dups.is_empty(),
        tr!("duplicate packages: {}", dups.join(" "))
    );

    if !actions.missing.is_empty() {
        let mut err = tr!("could not find all required packages:");
        for missing in &actions.missing {
            if missing.stack.is_empty() {
                write!(err, "\n    {} (target)", c.error.paint(&missing.dep))?;
            } else {
                let stack = missing
                    .stack
                    .iter()
                    .map(fmt_stack)
                    .collect::<Vec<_>>()
                    .join(" -> ");
                err.push_str(&tr!(
                    "\n    {missing} (wanted by: {stack})",
                    missing = c.error.paint(&missing.dep),
                    stack = stack
                ));
            };
        }

        bail!("{}", err);
    }

    for pkg in &actions.unneeded {
        eprintln!(
            "{} {}",
            c.warning.paint("::"),
            tr!("{}-{} is up to date -- skipping", pkg.name, pkg.version)
        );
    }

    if actions.build.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }

    if config.chroot && config.args.has_arg("w", "downloadonly") {
        return Ok((Vec::new(), Vec::new()));
    }

    println!(
        "{} {}",
        c.action.paint("::"),
        c.bold.paint(tr!("Calculating conflicts..."))
    );
    let conflicts = actions.calculate_conflicts(!config.chroot);
    println!(
        "{} {}",
        c.action.paint("::"),
        c.bold.paint(tr!("Calculating inner conflicts..."))
    );
    let inner_conflicts = actions.calculate_inner_conflicts(!config.chroot);

    if !conflicts.is_empty() || !inner_conflicts.is_empty() {
        eprintln!();
    }

    if !inner_conflicts.is_empty() {
        eprintln!(
            "{} {}",
            c.error.paint("::"),
            c.bold.paint(tr!("Inner conflicts found:"))
        );

        for conflict in &inner_conflicts {
            eprint!("    {}: ", conflict.pkg);

            for conflict in &conflict.conflicting {
                eprint!("{}", conflict.pkg);
                if let Some(conflict) = &conflict.conflict {
                    eprint!(" ({})", conflict);
                }
                eprint!("  ");
            }
            eprintln!();
        }
        eprintln!();
    }

    if !conflicts.is_empty() {
        eprintln!(
            "{} {}",
            c.error.paint("::"),
            c.bold.paint(tr!("Conflicts found:"))
        );

        for conflict in &conflicts {
            eprint!("    {}: ", conflict.pkg);

            for conflict in &conflict.conflicting {
                eprint!("{}", conflict.pkg);
                if let Some(conflict) = &conflict.conflict {
                    eprint!(" ({})", conflict);
                }
                eprint!("  ");
            }
            eprintln!();
        }
        eprintln!();
    }

    if (!conflicts.is_empty() || !inner_conflicts.is_empty()) && !config.use_ask {
        eprintln!(
            "{} {}",
            c.warning.paint("::"),
            c.bold.paint(tr!(
                "Conflicting packages will have to be confirmed manually"
            ))
        );
        if config.no_confirm {
            bail!(tr!("can not install conflicting packages with --noconfirm"));
        }
    }

    Ok((conflicts, inner_conflicts))
}

fn repo_install(config: &Config, install: &[RepoPackage]) -> Result<i32> {
    if install.is_empty() {
        return Ok(0);
    }

    let mut deps = Vec::new();
    let mut exp = Vec::new();

    let targets = install
        .iter()
        .map(|p| format!("{}/{}", p.pkg.db().unwrap().name(), p.pkg.name()))
        .collect::<Vec<_>>();

    let mut args = config.pacman_args();
    args.remove("asdeps")
        .remove("asdep")
        .remove("asexplicit")
        .remove("asexp")
        .remove("y")
        .remove("i")
        .remove("refresh")
        .arg("noconfirm");
    args.targets = targets.iter().map(|s| s.as_str()).collect();

    if !config.combined_upgrade || !config.mode.repo() {
        args.remove("u").remove("sysupgrade");
    }

    if config.globals.has_arg("asexplicit", "asexp") {
        exp.extend(install.iter().map(|p| p.pkg.name()));
    } else if config.globals.has_arg("asdeps", "asdep") {
        deps.extend(install.iter().map(|p| p.pkg.name()));
    } else {
        for pkg in install {
            if config.alpm.localdb().pkg(pkg.pkg.name()).is_err() {
                if pkg.target {
                    exp.push(pkg.pkg.name())
                } else {
                    deps.push(pkg.pkg.name())
                }
            }
        }
    }

    exec::pacman(config, &args)?.success()?;
    asdeps(config, &deps)?;
    asexp(config, &exp)?;

    Ok(0)
}

fn asdeps<S: AsRef<str>>(config: &Config, pkgs: &[S]) -> Result<()> {
    set_install_reason(config, "asdeps", pkgs)
}

fn asexp<S: AsRef<str>>(config: &Config, pkgs: &[S]) -> Result<()> {
    set_install_reason(config, "asexplicit", pkgs)
}

fn set_install_reason<S: AsRef<str>>(config: &Config, reason: &str, pkgs: &[S]) -> Result<()> {
    let alpm = config.new_alpm()?;
    let db = alpm.localdb();

    let pkgs = pkgs
        .iter()
        .map(|s| s.as_ref())
        .filter(|p| db.pkg(*p).is_ok());

    let mut args = config.pacman_globals();
    args.op("database").arg(reason).targets(pkgs);
    if args.targets.is_empty() {
        return Ok(());
    }

    let output = exec::pacman_output(config, &args)?;
    ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn pre_build_command(config: &Config, dir: &Path, base: &str, version: &str) -> Result<()> {
    if let Some(ref pb_cmd) = config.pre_build_command {
        let mut cmd = Command::new("sh");
        cmd.env("PKGBASE", base)
            .env("VERSION", version)
            .current_dir(dir)
            .arg("-c")
            .arg(pb_cmd);
        exec::command(&mut cmd)?;
    }
    Ok(())
}

fn file_manager(
    config: &Config,
    fetch: &aur_fetch::Fetch,
    fm: &str,
    pkgs: &[&str],
) -> Result<tempfile::TempDir> {
    let has_diff = fetch.has_diff(pkgs)?;
    fetch.save_diffs(&has_diff)?;
    let view = tempfile::Builder::new().prefix("aur").tempdir()?;
    fetch.make_view(view.path(), pkgs, &has_diff)?;
    run_file_manager(config, fm, view.path())?;
    Ok(view)
}

fn run_file_manager(config: &Config, fm: &str, dir: &Path) -> Result<()> {
    let ret = Command::new(fm)
        .args(&config.fm_flags)
        .arg(dir)
        .current_dir(dir)
        .status()
        .with_context(|| tr!("failed to execute file manager: {}", fm))?;
    ensure!(
        ret.success(),
        tr!("file manager did not execute successfully")
    );
    Ok(())
}

fn print_dir(
    config: &Config,
    path: &Path,
    stdin: &mut impl Write,
    buf: &mut Vec<u8>,
    bat: bool,
    recurse: u32,
) -> Result<()> {
    {
        let c = config.color;
        let has_pkgbuild = path.join("PKGBUILD").exists();

        for file in read_dir(path).with_context(|| tr!("failed to read dir: {}", path.display()))? {
            let file = file?;

            if file.file_type()?.is_dir() && file.path().file_name() == Some(OsStr::new(".git")) {
                continue;
            }
            if file.file_type()?.is_file()
                && file.path().file_name() == Some(OsStr::new(".SRCINFO"))
            {
                continue;
            }
            if file.file_type()?.is_dir() {
                if recurse == 0 {
                    continue;
                }
                print_dir(config, &file.path(), stdin, buf, bat, recurse - 1)?;
            }
            if !has_pkgbuild {
                continue;
            }
            if file.file_type()?.is_symlink() {
                let s = format!(
                    "{} -> {}\n\n\n",
                    file.path().display(),
                    read_link(file.path())?.display()
                );
                let _ = write!(stdin, "{}", c.bold.paint(s));
                continue;
            }

            let _ = writeln!(stdin, "{}", c.bold.paint(file.path().display().to_string()));
            if bat {
                let output = Command::new(&config.bat_bin)
                    .arg("-pp")
                    .arg("--color=always")
                    .arg(file.path())
                    .args(&config.bat_flags)
                    .output()
                    .with_context(|| {
                        format!(
                            "{} {} {}",
                            tr!("failed to run:"),
                            config.bat_bin,
                            file.path().display()
                        )
                    })?;
                let _ = stdin.write_all(&output.stdout);
            } else {
                let mut pkgbbuild = OpenOptions::new()
                    .read(true)
                    .open(file.path())
                    .with_context(|| {
                        tr!("failed to open: {}", file.path().display().to_string())
                    })?;
                buf.clear();
                pkgbbuild.read_to_end(buf)?;

                let _ = match std::str::from_utf8(buf) {
                    Ok(_) => stdin.write_all(buf),
                    Err(_) => {
                        write!(
                            stdin,
                            "{}",
                            tr!("binary file: {}", file.path().display().to_string())
                        )
                    }
                };
            }
            let _ = stdin.write_all(b"\n\n");
        }
    }

    Ok(())
}

pub fn review(config: &Config, fetch: &aur_fetch::Fetch, pkgs: &[&str]) -> Result<()> {
    if pkgs.is_empty() {
        return Ok(());
    }
    if !config.no_confirm {
        if let Some(ref fm) = config.fm {
            let _view = file_manager(config, fetch, fm, pkgs)?;

            if !ask(config, &tr!("Accept changes?"), true) {
                return Status::err(1);
            }

            if config.save_changes {
                fetch.commit(pkgs, "paru save changes")?;
            }
        } else {
            let unseen = fetch.unseen(pkgs)?;
            let has_diff = fetch.has_diff(&unseen)?;
            let printed = !has_diff.is_empty() || unseen.iter().any(|p| !has_diff.contains(p));
            let diffs = fetch.diff(&has_diff, config.color.enabled)?;

            if printed {
                let pager = if Command::new("less").output().is_ok() {
                    "less"
                } else {
                    "cat"
                };

                let pager = config
                    .pager_cmd
                    .clone()
                    .or_else(|| var("PARU_PAGER").ok())
                    .or_else(|| var("PAGER").ok())
                    .unwrap_or_else(|| pager.to_string());

                exec::RAISE_SIGPIPE.store(false, Ordering::Relaxed);
                let mut command = Command::new("sh");

                if std::env::var("LESS").is_err() {
                    command.env("LESS", "SRXF");
                }
                let mut command = command
                    .arg("-c")
                    .arg(&pager)
                    .stdin(Stdio::piped())
                    .spawn()
                    .with_context(|| format!("{} {}", tr!("failed to run:"), pager))?;

                let mut stdin = command.stdin.take().unwrap();

                for diff in diffs {
                    let _ = stdin.write_all(diff.as_bytes());
                    let _ = stdin.write_all(b"\n\n\n");
                }

                let bat = config.color.enabled
                    && Command::new(&config.bat_bin).arg("-V").output().is_ok();

                let mut buf = Vec::new();
                for pkg in &unseen {
                    if !has_diff.contains(pkg) {
                        let dir = fetch.clone_dir.join(pkg);
                        print_dir(config, &dir, &mut stdin, &mut buf, bat, 1)?;
                    }
                }

                drop(stdin);
                command
                    .wait()
                    .with_context(|| format!("{} {}", tr!("failed to run:"), pager))?;
                exec::RAISE_SIGPIPE.store(true, Ordering::Relaxed);

                if !ask(config, &tr!("Accept changes?"), true) {
                    return Status::err(1);
                }
            } else {
                printtr!(" nothing new to review");
            }
        }
    }

    fetch.mark_seen(pkgs)?;
    Ok(())
}

fn update_aur_list(config: &Config) {
    let url = config.aur_url.clone();
    let dir = config.cache_dir.clone();
    let interval = config.completion_interval;

    tokio::spawn(async move {
        let _ = update_aur_cache(&url, &dir, Some(interval)).await;
    });
}

fn chroot(config: &Config) -> Chroot {
    let mut chroot = Chroot {
        #[cfg(not(feature = "mock_chroot"))]
        sudo: config.sudo_bin.clone(),
        #[cfg(feature = "mock_chroot")]
        sudo: "sudo".to_string(),
        path: config.chroot_dir.clone(),
        pacman_conf: config
            .pacman_conf
            .as_deref()
            .unwrap_or("/etc/pacman.conf")
            .to_string(),
        makepkg_conf: config
            .makepkg_conf
            .as_deref()
            .unwrap_or("/etc/makepkg.conf")
            .to_string(),
        mflags: config.mflags.clone(),

        ro: repo::all_files(config),
        rw: config.pacman.cache_dir.clone(),
    };

    if config.args.count("d", "nodeps") > 1 {
        chroot.mflags.push("-d".to_string());
    }

    chroot
}

fn trim_dep_ver(dep: &str, trim: bool) -> &str {
    if trim {
        dep.split_once(is_ver_char).map_or(dep, |x| x.0)
    } else {
        dep
    }
}

fn check_deps_local<'a>(
    alpm: &Alpm,
    missing: &mut Vec<&'a str>,
    deps: impl Iterator<Item = &'a str>,
    nover: bool,
) {
    let db = alpm.localdb().pkgs();

    for dep in deps {
        let not_found = db.find_satisfier(trim_dep_ver(dep, nover)).is_none();

        if not_found {
            missing.push(dep)
        }
    }
}

fn check_deps_sync<'a>(
    alpm: &Alpm,
    missing: &mut Vec<&'a str>,
    deps: impl Iterator<Item = &'a str>,
    nover: bool,
) {
    let db = alpm.syncdbs();

    for dep in deps {
        let not_found = db.find_satisfier(trim_dep_ver(dep, nover)).is_none();

        if not_found {
            missing.push(dep)
        }
    }
}

fn supported_deps<'a>(config: &'a Config, deps: &'a [ArchVec]) -> impl Iterator<Item = &'a str> {
    let arch = config.alpm.architectures().first().unwrap_or_default();
    ArchVec::supported(deps, arch)
}

fn deps_not_satisfied<'a>(config: &'a Config, base: &'a Base) -> Result<Vec<&'a str>> {
    let nover = config.args.count("d", "nodeps") > 0;

    let alpm = config.new_alpm()?;
    let mut missing = Vec::new();

    match base {
        Base::Aur(base) => {
            for pkg in &base.pkgs {
                check_deps_local(
                    &alpm,
                    &mut missing,
                    pkg.pkg.depends.iter().map(|s| s.as_str()),
                    nover,
                );
                check_deps_local(
                    &alpm,
                    &mut missing,
                    pkg.pkg.make_depends.iter().map(|s| s.as_str()),
                    nover,
                );
                if !config.no_check {
                    check_deps_local(
                        &alpm,
                        &mut missing,
                        pkg.pkg.check_depends.iter().map(|s| s.as_str()),
                        nover,
                    );
                }
            }
        }
        Base::Pkgbuild(base) => {
            check_deps_local(
                &alpm,
                &mut missing,
                supported_deps(config, &base.srcinfo.base.makedepends),
                nover,
            );
            if !config.no_check {
                check_deps_local(
                    &alpm,
                    &mut missing,
                    supported_deps(config, &base.srcinfo.base.checkdepends),
                    nover,
                );
            }

            for pkg in &base.pkgs {
                check_deps_local(
                    &alpm,
                    &mut missing,
                    supported_deps(config, &pkg.pkg.depends),
                    nover,
                );
            }
        }
    }

    if nover {
        missing.retain(|dep| {
            !config
                .alpm
                .assume_installed()
                .iter()
                .any(|provide| satisfies_provide_nover(Depend::new(*dep), provide))
        });
    } else {
        missing.retain(|dep| {
            !config
                .alpm
                .assume_installed()
                .iter()
                .any(|provide| satisfies_provide(Depend::new(*dep), provide))
        });
    }

    Ok(missing)
}

fn deps_not_satisfied_by_repo<'a>(config: &'a Config, base: &'a Base) -> Result<Vec<&'a str>> {
    let nover = config.args.count("d", "nodeps") > 0;
    let alpm = config.new_alpm()?;
    let mut missing = Vec::new();

    match base {
        Base::Aur(base) => {
            for pkg in &base.pkgs {
                check_deps_sync(
                    &alpm,
                    &mut missing,
                    pkg.pkg.depends.iter().map(|s| s.as_str()),
                    nover,
                );
                check_deps_sync(
                    &alpm,
                    &mut missing,
                    pkg.pkg.make_depends.iter().map(|s| s.as_str()),
                    nover,
                );
                if !config.no_check {
                    check_deps_sync(
                        &alpm,
                        &mut missing,
                        pkg.pkg.check_depends.iter().map(|s| s.as_str()),
                        nover,
                    );
                }
            }
        }
        Base::Pkgbuild(base) => {
            check_deps_sync(
                &alpm,
                &mut missing,
                supported_deps(config, &base.srcinfo.base.makedepends),
                nover,
            );
            if !config.no_check {
                check_deps_sync(
                    &alpm,
                    &mut missing,
                    supported_deps(config, &base.srcinfo.base.checkdepends),
                    nover,
                );
            }

            for pkg in &base.pkgs {
                check_deps_sync(
                    &alpm,
                    &mut missing,
                    supported_deps(config, &pkg.pkg.depends),
                    nover,
                );
            }
        }
    }

    Ok(missing)
}

pub fn copy_sync_args<'a>(config: &'a Config, args: &mut Args<&'a str>) {
    config
        .args
        .args
        .iter()
        .filter(|a| matches!(&*a.key, "overwrite" | "ignore"))
        .for_each(|a| args.push(&a.key, a.value.as_deref()));

    config
        .assume_installed
        .iter()
        .for_each(|a| args.push("assume-installed", Some(a.as_str())));

    if config.args.has_arg("dbonly", "dbonly") {
        args.arg("dbonly");
    }

    for _ in 0..config.args.count("d", "nodeps") {
        args.arg("d");
    }
}

fn parse_package_list(
    config: &Config,
    dir: &Path,
    pkgdest: Option<&str>,
) -> Result<(HashMap<String, String>, String)> {
    let output = exec::makepkg_output_dest(config, dir, &["--packagelist"], pkgdest)?;
    let output = String::from_utf8(output.stdout).context("pkgdest is not utf8")?;
    let mut pkgdests = HashMap::new();
    let mut version = String::new();

    for line in output.trim().lines() {
        let file = line.rsplit('/').next().unwrap();

        let split = file.split('-').collect::<Vec<_>>();
        ensure!(
            split.len() >= 4,
            "{}",
            tr!("can't find package name in packagelist: {}", line)
        );

        // pkgname-pkgver-pkgrel-arch.pkgext
        // This assumes 3 dashes after the pkgname, Will cause an error
        // if the PKGEXT contains a dash. Please no one do that.
        let pkgname = split[..split.len() - 3].join("-");
        version = split[split.len() - 3..split.len() - 1].join("-");
        pkgdests.insert(pkgname, line.to_string());
    }

    Ok((pkgdests, version))
}

fn needs_build(
    config: &Config,
    base: &Base,
    pkgdest: &HashMap<String, String>,
    version: &str,
) -> bool {
    if (config.rebuild == YesNoAllTree::Yes && base.target()) || config.rebuild != YesNoAllTree::No
    {
        return true;
    }

    if config.args.has_arg("needed", "needed") {
        let mut all_installed = true;
        let c = config.color;

        if config.repos != LocalRepos::None {
            let (_, repos) = repo::repo_aur_dbs(config);

            if !base
                .packages()
                .filter_map(|p| repos.pkg(p).ok())
                .any(|_| base.version() == version)
            {
                all_installed = false
            }
        } else if !base
            .packages()
            .filter_map(|p| config.alpm.localdb().pkg(p).ok())
            .any(|_| base.version() == version)
        {
            all_installed = false
        }

        if all_installed {
            println!(
                "{} {}",
                c.warning.paint("::"),
                tr!(
                    "{}-{} is up to date -- skipping",
                    base.package_base(),
                    base.version()
                )
            );
            return false;
        }
    }

    !base
        .packages()
        .all(|p| Path::new(pkgdest.get(p).unwrap()).exists())
}

fn sign_pkg(config: &Config, paths: &[&str], delete_sig: bool) -> Result<()> {
    if config.sign != Sign::No {
        let c = config.color;
        println!(
            "{} {}",
            c.action.paint("::"),
            c.bold.paint(tr!("Signing packages..."))
        );

        for path in paths {
            let mut cmd = Command::new("gpg");
            cmd.args(["--detach-sign", "--no-armor", "--batch"]);

            if let Sign::Key(ref k) = config.sign {
                cmd.arg("-u").arg(k);
            }

            let sig = format!("{}.sig", path);
            if Path::new(&sig).exists() {
                if delete_sig {
                    std::fs::remove_file(&sig)?;
                } else {
                    continue;
                }
            }

            cmd.arg("--output").arg(&sig).arg(path);

            exec::command(&mut cmd)?;
        }
    }

    Ok(())
}

fn needs_install(config: &Config, base: &Base, version: &str, pkg: &str) -> bool {
    if config.args.has_arg("needed", "needed") {
        if let Ok(pkg) = config.alpm.localdb().pkg(pkg) {
            if pkg.version().as_str() == version {
                let c = config.color;
                println!(
                    "{} {}",
                    c.warning.paint("::"),
                    tr!(
                        "{}-{} is up to date -- skipping install",
                        base.package_base(),
                        base.version()
                    )
                );
                return false;
            }
        }
    }

    true
}

fn is_ver_char(c: char) -> bool {
    matches!(c, '<' | '=' | '>')
}
