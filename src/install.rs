use crate::args::Arg;
use crate::chroot::Chroot;
use crate::clean::clean_untracked;
use crate::completion::update_aur_cache;
use crate::config::{Config, LocalRepos};
use crate::devel::{fetch_devel_info, load_devel_info, save_devel_info, DevelInfo};
use crate::download::{self, Bases};
use crate::fmt::{color_repo, print_indent};
use crate::keys::check_pgp_keys;
use crate::print_error;
use crate::repo;
use crate::upgrade::get_upgrades;
use crate::util::{ask, get_provider, split_repo_aur_targets, NumberMenu};
use crate::{args, exec, news};

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::io::{stdin, stdout, BufRead, Write};
use std::iter::FromIterator;
use std::path::Path;
use std::process::{Command, Stdio};

use alpm::Alpm;
use alpm_utils::{DbListExt, Targ};
use ansi_term::Style;
use anyhow::{bail, ensure, Context, Result};
use aur_depends::{Actions, AurPackage, Base, Conflict, Flags, RepoPackage, Resolver};
use pacmanconf::Repository;
use raur::Cache;
use srcinfo::Srcinfo;

fn early_refresh(config: &Config) -> Result<()> {
    let mut args = config.pacman_globals();
    for _ in 0..config.args.count("y", "refresh") {
        args.arg("y");
    }
    args.targets.clear();
    exec::pacman(config, &args)?.success()?;
    Ok(())
}

fn early_pacman(config: &Config, targets: Vec<String>) -> Result<()> {
    let mut args = config.pacman_args();
    args.targets.clear();
    args.targets(targets.iter().map(|i| i.as_str()));
    exec::pacman(config, &args)?.success()?;
    Ok(())
}

pub async fn install(config: &mut Config, targets_str: &[String]) -> Result<i32> {
    let mut cache = Cache::new();
    let flags = flags(config);
    let c = config.color;

    if config.sudo_loop {
        exec::spawn_sudo(config.sudo_bin.clone(), config.sudo_flags.clone())?;
    }

    if config.news_on_upgrade && config.args.has_arg("u", "sysupgrade") {
        let mut ret = 0;
        match news::news(config).await {
            Ok(v) => ret = v,
            Err(err) => eprintln!("{} could not get news: {}", c.error.paint("error:"), err),
        }

        if ret != 1 {
            ask(config, "Continue with install?", true);
        }
    }

    config.op = "sync".to_string();
    config.args.op = config.op.clone();
    config.globals.op = config.op.clone();
    config.targets = targets_str.to_vec();
    config.args.targets = config.targets.clone();

    let targets = args::parse_targets(&targets_str);
    let (mut repo_targets, aur_targets) = split_repo_aur_targets(config, &targets);
    let mut done_something = false;

    if targets_str.is_empty()
        && !config.args.has_arg("u", "sysupgrade")
        && !config.args.has_arg("y", "refresh")
    {
        bail!("no targets specified (use -h for help)");
    }

    if config.mode != "aur" {
        if config.combined_upgrade {
            if config.args.has_arg("y", "refresh") {
                early_refresh(config)?;
            }
        } else if config.args.has_arg("y", "refresh")
            || config.args.has_arg("u", "sysupgrade")
            || !repo_targets.is_empty()
        {
            let targets = repo_targets.iter().map(|t| t.to_string()).collect();
            repo_targets.clear();
            done_something = true;
            early_pacman(config, targets)?;
        }
    }

    if targets_str.is_empty() && !config.args.has_arg("u", "sysupgrade") {
        return Ok(0);
    }

    config.init_alpm()?;

    let mut resolver = resolver(&config, &config.alpm, &config.raur, &mut cache, flags);

    let upgrades = if config.args.has_arg("u", "sysupgrade") {
        let upgrades = get_upgrades(config, &mut resolver).await?;
        for pkg in &upgrades.repo_skip {
            let arg = Arg {
                key: "ignore".to_string(),
                value: Some(pkg.to_string()),
            };

            config.globals.args.push(arg.clone());
            config.args.args.push(arg);
        }
        upgrades
    } else {
        Default::default()
    };

    let mut targets = repo_targets;
    targets.extend(&aur_targets);
    targets.extend(upgrades.aur_keep.iter().map(|p| Targ {
        repo: Some(config.aur_namespace()),
        pkg: p,
    }));
    targets.extend(upgrades.repo_keep.iter().map(Targ::from));

    // No aur stuff, let's just use pacman
    if aur_targets.is_empty() && upgrades.aur_keep.is_empty() && config.combined_upgrade {
        print_warnings(config, &cache, None);
        let mut args = config.pacman_args();
        let targets = targets.iter().map(|t| t.to_string()).collect::<Vec<_>>();
        args.targets = targets.iter().map(|s| s.as_str()).collect();
        args.remove("y").remove("refresh");

        let code = exec::pacman(config, &args)?.code();
        return Ok(code);
    }

    if targets.is_empty() {
        print_warnings(config, &cache, None);
        if !done_something {
            println!(" there is nothing to do");
        }
        return Ok(0);
    }

    println!(
        "{} {}",
        c.action.paint("::"),
        c.bold.paint("Resolving dependencies...")
    );

    let mut actions = resolver.resolve_targets(&targets).await?;

    if !actions.build.is_empty() && nix::unistd::getuid().is_root() {
        bail!("can't install AUR package as root");
    }

    let conflicts = check_actions(config, &actions)?;

    print_warnings(config, &cache, Some(&actions));

    if actions.build.is_empty() && actions.install.is_empty() {
        if config.args.has_arg("u", "sysupgrade") || !aur_targets.is_empty() {
            print_warnings(config, &cache, None);
            println!(" there is nothing to do");
        }
        return Ok(0);
    }

    print_install(config, &actions);

    let remove_make = if !config.chroot
        && (actions.iter_build_pkgs().any(|p| p.make) || actions.install.iter().any(|p| p.make))
    {
        if config.remove_make == "ask" {
            ask(config, "Remove make dependencies after install?", false)
        } else {
            config.remove_make == "yes"
        }
    } else {
        false
    };

    if !ask(config, "Proceed to review?", true) {
        return Ok(1);
    }

    let bases = Bases::from_iter(actions.iter_build_pkgs().map(|p| p.pkg.clone()));
    let srcinfos = download_pkgbuilds(config, &bases).await?;

    let ret = review(config, &actions, &srcinfos, &bases)?;
    if ret != 0 {
        return Ok(ret);
    }

    let mut err = if !config.chroot {
        repo_install(config, &mut actions.install)
    } else {
        Ok(0)
    };

    update_aur_list(config);

    let conflicts = conflicts
        .0
        .iter()
        .map(|c| c.pkg.as_str())
        .chain(conflicts.1.iter().map(|c| c.pkg.as_str()))
        .collect::<HashSet<_>>();

    let mut build = actions.build;
    let install_targets = actions
        .install
        .iter()
        .filter(|p| p.make)
        .map(|p| p.pkg.name().to_string())
        .collect::<Vec<_>>();

    if err.is_ok() {
        //download_pkgbuild_sources(config, &actions.build)?;
        err = build_install_pkgbuilds(
            config,
            &mut build,
            &srcinfos,
            &upgrades.aur_repos,
            &bases,
            &conflicts,
        )
        .await;
    }

    if remove_make {
        let mut args = config.pacman_globals();
        args.op("remove").arg("noconfirm");
        args.targets = build
            .iter()
            .flat_map(|b| &b.pkgs)
            .filter(|p| p.make)
            .map(|p| p.pkg.name.as_str())
            .collect();

        args.targets
            .extend(install_targets.iter().map(|s| s.as_str()));

        if let Err(err) = exec::pacman(config, &args) {
            print_error(config.color.error, err);
        }
    }

    if config.clean_after {
        for base in &build {
            let path = config.build_dir.join(base.package_base());
            if let Err(err) = clean_untracked(config, &path) {
                print_error(config.color.error, err);
            }
        }
    }

    err
}

async fn download_pkgbuilds<'a>(
    config: &Config,
    bases: &Bases,
) -> Result<HashMap<String, Srcinfo>> {
    let mut srcinfos = HashMap::new();

    for base in &bases.bases {
        let path = config.build_dir.join(base.package_base()).join(".SRCINFO");
        if path.exists() {
            let srcinfo = Srcinfo::parse_file(path);
            if let Ok(srcinfo) = srcinfo {
                srcinfos.insert(srcinfo.base.pkgbase.to_string(), srcinfo);
            }
        }
    }

    download::new_aur_pkgbuilds(config, &bases, &srcinfos).await?;

    for base in &bases.bases {
        if srcinfos.contains_key(base.package_base()) {
            continue;
        }
        let path = config.build_dir.join(base.package_base()).join(".SRCINFO");
        if path.exists() {
            if let Entry::Vacant(vacant) = srcinfos.entry(base.package_base().to_string()) {
                let srcinfo = Srcinfo::parse_file(path)
                    .with_context(|| format!("failed to parse srcinfo for '{}'", base))?;
                vacant.insert(srcinfo);
            }
        } else {
            bail!("could not find .SRINFO for '{}'", base.package_base());
        }
    }
    Ok(srcinfos)
}

fn review<'a>(
    config: &Config,
    actions: &Actions<'a>,

    srcinfos: &HashMap<String, Srcinfo>,

    bases: &Bases,
) -> Result<i32> {
    let pkgs = actions
        .build
        .iter()
        .map(|b| b.package_base())
        .collect::<Vec<_>>();

    if let Some(ref fm) = config.fm {
        let _view = file_manager(config, fm, &pkgs)?;

        if !ask(config, "Proceed with installation?", true) {
            return Ok(1);
        }
    } else {
        let unseen = config.fetch.unseen(&pkgs)?;
        let has_diff = config.fetch.has_diff(&unseen)?;
        let printed = !has_diff.is_empty() || unseen.iter().any(|p| !has_diff.contains(p));
        let diffs = config.fetch.diff(&has_diff, config.color.enabled)?;

        if printed {
            let pager = std::env::var("PAGER").unwrap_or_else(|_| "less".to_string());

            let mut command = Command::new(&pager)
                .stdin(Stdio::piped())
                .env("LESS", "SRX")
                .spawn()
                .with_context(|| format!("failed to run {}", pager))?;

            let mut stdin = command.stdin.take().unwrap();

            for diff in diffs {
                stdin.write_all(diff.as_bytes())?;
                stdin.write_all(b"\n\n\n")?;
            }

            for pkg in &unseen {
                if !has_diff.contains(pkg) {
                    let path = config.build_dir.join(pkg).join("PKGBUILD");

                    let bat = config.color.enabled
                        && Command::new(&config.bat_bin).arg("-V").output().is_ok();

                    if bat {
                        let output = Command::new(&config.bat_bin)
                            .arg("-pp")
                            .arg("--color=always")
                            .arg("-lPKGBUILD")
                            .arg(path)
                            .args(&config.bat_flags)
                            .output()
                            .with_context(|| format!("failed to run {}", config.bat_bin))?;
                        stdin.write_all(&output.stdout)?;
                        stdin.write_all(b"\n\n\n")?;
                    } else {
                        let pkgbuild = std::fs::read_to_string(&path)
                            .context(format!("failed to open {}", path.display()))?;
                        stdin.write_all(pkgbuild.as_bytes())?;
                        stdin.write_all(b"\n\n\n")?;
                    }
                }
            }

            drop(stdin);
            command
                .wait()
                .with_context(|| format!("failed to run {}", pager))?;
        } else {
            println!(" nothing new to review");
        }

        if !ask(config, "Proceed with installation?", true) {
            return Ok(1);
        }
    }

    config.fetch.mark_seen(&pkgs)?;

    let incompatible = srcinfos
        .values()
        .flat_map(|s| &s.pkgs)
        .filter(|p| {
            !p.arch.iter().any(|a| a == "any") && !p.arch.iter().any(|a| a == config.alpm.arch())
        })
        .collect::<Vec<_>>();

    if !incompatible.is_empty() {
        let c = config.color;
        println!(
            "{} {}",
            c.error.paint("::"),
            c.bold
                .paint("The following packages are not compatible with your architecture:")
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
        if !ask(config, "Would you like to try build them anyway?", true) {
            return Ok(1);
        }
    }

    if config.pgp_fetch {
        check_pgp_keys(config, &bases, &srcinfos)?;
    }

    Ok(0)
}

fn file_manager(config: &Config, fm: &str, pkgs: &[&str]) -> Result<tempfile::TempDir> {
    let has_diff = config.fetch.has_diff(pkgs)?;
    config.fetch.save_diffs(&has_diff)?;
    let view = config.fetch.make_view(pkgs, &has_diff)?;

    let ret = Command::new(fm)
        .args(&config.fm_flags)
        .arg(view.path())
        .current_dir(view.path())
        .status()
        .with_context(|| format!("failed to execute file manager: {}", fm))?;
    ensure!(ret.success(), "file manager did not execute successfully");
    Ok(view)
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
        .remove("asexplicit")
        .remove("y")
        .remove("refresh");
    args.targets = targets.iter().map(|s| s.as_str()).collect();

    if !config.combined_upgrade || config.mode == "aur" {
        args.remove("u").remove("sysupgrade");
    }

    if config.globals.has_arg("asexplicit", "asexplicit") {
        exp.extend(install.iter().map(|p| p.pkg.name()));
    } else if config.globals.has_arg("asdeps", "asdeps") {
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

fn check_actions(config: &Config, actions: &Actions) -> Result<(Vec<Conflict>, Vec<Conflict>)> {
    let c = config.color;
    let dups = actions.duplicate_targets();
    ensure!(dups.is_empty(), "duplicate packages: {}", dups.join(" "));

    if !actions.missing.is_empty() {
        let mut err = "could not find all required packages:".to_string();
        for missing in &actions.missing {
            if missing.stack.is_empty() {
                err.push_str(&format!("\n    {} (target)", c.error.paint(&missing.dep)));
            } else {
                let stack = missing.stack.join(" -> ");
                err.push_str(&format!(
                    "\n    {} (wanted by: {})",
                    c.error.paint(&missing.dep),
                    stack
                ));
            };
        }

        bail!("{}", err);
    }

    for pkg in &actions.unneeded {
        eprintln!(
            "{} {}-{} is up to date -- skipping",
            c.warning.paint("::"),
            pkg.name,
            pkg.version
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
        c.bold.paint("Calculating conflicts...")
    );
    let conflicts = actions.calculate_conflicts(!config.chroot);
    println!(
        "{} {}",
        c.action.paint("::"),
        c.bold.paint("Calculating inner conflicts...")
    );
    let inner_conflicts = actions.calculate_inner_conflicts(!config.chroot);

    if !conflicts.is_empty() || !inner_conflicts.is_empty() {
        eprintln!();
    }

    if !inner_conflicts.is_empty() {
        eprintln!(
            "{} {}",
            c.error.paint("::"),
            c.bold.paint("Inner conflicts found:")
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
            c.bold.paint("Conflicts found:")
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
            c.bold
                .paint("Conflicting packages will have to be confirmed manually")
        );
        if config.no_confirm {
            bail!("can not install conflicting packages with --noconfirm");
        }
    }

    Ok((conflicts, inner_conflicts))
}

fn print_install(config: &Config, actions: &Actions) {
    let c = config.color;
    println!();

    let install = actions
        .install
        .iter()
        .filter(|p| !p.make)
        .map(|p| format!("{}-{}", p.pkg.name(), p.pkg.version()))
        .collect::<Vec<_>>();
    let make_install = actions
        .install
        .iter()
        .filter(|p| p.make)
        .map(|p| format!("{}-{}", p.pkg.name(), p.pkg.version()))
        .collect::<Vec<_>>();

    let mut build = actions.build.clone();
    for base in &mut build {
        base.pkgs.retain(|p| !p.make);
    }
    build.retain(|b| !b.pkgs.is_empty());
    let build = build.iter().map(|p| p.to_string()).collect::<Vec<_>>();

    let mut make_build = actions.build.clone();
    for base in &mut make_build {
        base.pkgs.retain(|p| p.make);
    }
    make_build.retain(|b| !b.pkgs.is_empty());
    let make_build = make_build.iter().map(|p| p.to_string()).collect::<Vec<_>>();

    if !install.is_empty() {
        let fmt = format!("{} ({}) ", "Repo", install.len());
        let start = 17 + install.len().to_string().len();
        print!("{}", c.bold.paint(fmt));
        print_indent(Style::new(), start, 4, config.cols, "  ", install);
    }

    if !make_install.is_empty() {
        let fmt = format!("{} ({}) ", "Repo Make", make_install.len());
        let start = 22 + make_install.len().to_string().len();
        print!("{}", c.bold.paint(fmt));
        print_indent(Style::new(), start, 4, config.cols, "  ", make_install);
    }

    if !build.is_empty() {
        let fmt = format!("{} ({}) ", "Aur", build.len());
        let start = 16 + build.len().to_string().len();
        print!("{}", c.bold.paint(fmt));
        print_indent(Style::new(), start, 4, config.cols, "  ", build);
    }

    if !make_build.is_empty() {
        let fmt = format!("{} ({}) ", "Aur Make", make_build.len());
        let start = 16 + make_build.len().to_string().len();
        print!("{}", c.bold.paint(fmt));
        print_indent(Style::new(), start, 4, config.cols, "  ", make_build);
    }

    println!();
}

/*fn download_pkgbuild_sources(config: &Config, build: &[aur_depends::Base]) -> Result<()> {
    for base in build {
        let pkg = base.package_base();
        let dir = config.build_dir.join(pkg);

        exec::makepkg(config, &dir, &["--verifysource", "-Ccf"])?
            .success()
            .with_context(|| format!("failed to download sources for '{}'", base))?;
    }

    Ok(())
}*/

fn do_install(
    config: &Config,
    deps: &mut Vec<&str>,
    exp: &mut Vec<&str>,
    install_queue: &mut Vec<String>,
    conflict: bool,
    devel_info: &mut DevelInfo,
) -> Result<()> {
    if !install_queue.is_empty() {
        let mut args = config.pacman_globals();
        let ask;
        args.op("upgrade");

        for _ in 0..args.count("d", "nodeps") {
            args.arg("d");
        }

        if conflict {
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

        args.targets = install_queue.iter().map(|s| s.as_str()).collect();
        exec::pacman(config, &args)?.success()?;

        if config.devel {
            save_devel_info(config, devel_info)?;
        }

        asdeps(config, &deps)?;
        asexp(config, &exp)?;
        deps.clear();
        exp.clear();
        install_queue.clear();
    }
    Ok(())
}

async fn build_install_pkgbuilds(
    config: &mut Config,
    build: &mut [aur_depends::Base],
    srcinfos: &HashMap<String, Srcinfo>,
    aur_repos: &HashMap<String, String>,
    bases: &Bases,
    conflicts: &HashSet<&str>,
) -> Result<i32> {
    let mut deps = Vec::new();
    let mut exp = Vec::new();
    let mut install_queue = Vec::new();
    let mut conflict = false;
    let mut failed = Vec::new();

    let chroot = Chroot {
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
        ro: repo::all_files(config)
            .iter()
            .map(|s| s.to_string())
            .collect(),
        rw: config.pacman.cache_dir.clone(),
    };

    let (mut devel_info, mut new_devel_info) = if config.devel {
        println!("fetching devel info...");
        (
            load_devel_info(config)?.unwrap_or_default(),
            fetch_devel_info(config, bases, srcinfos).await?,
        )
    } else {
        (DevelInfo::default(), DevelInfo::default())
    };

    let repo = repo::configured_local_repos(config);
    let repo = repo.get(0).map(|repo| {
        config
            .pacman
            .repos
            .iter()
            .find(|r| r.name == *repo)
            .unwrap()
    });

    if let Some(repo) = repo {
        let file = repo::file(repo).unwrap();
        repo::init(config, file, &repo.name)?;
    }

    let repo = repo.cloned();

    if config.chroot {
        if !chroot.exists() {
            chroot.create(config, &["base-devel"])?;
        } else {
            chroot.update()?;
        }
    }

    for base in build.iter_mut() {
        failed.push(base.clone());

        let err = build_install_pkgbuild(
            config,
            aur_repos,
            conflicts,
            &chroot,
            base,
            &mut deps,
            &mut exp,
            &mut install_queue,
            &mut conflict,
            &mut devel_info,
            &mut new_devel_info,
            repo.as_ref(),
        );

        if err.is_ok() {
            failed.pop().unwrap();
        }
    }

    if config.chroot {
        if !config.args.has_arg("w", "downloadonly") {
            let targets = build
                .iter()
                .filter(|b| !failed.iter().any(|f| b.package_base() == f.package_base()))
                .flat_map(|b| &b.pkgs)
                .filter(|p| p.target)
                .map(|p| p.pkg.name.as_str())
                .collect();

            let mut args = config.pacman_globals();
            args.op("sync");
            if config.args.has_arg("asexplicit", "asexplicit") {
                args.arg("asexplicit");
            } else if config.args.has_arg("asdeps", "asdeps") {
                args.arg("asdeps");
            }
            args.targets = targets;
            if !conflict {
                args.arg("noconfirm");
            }
            exec::pacman(config, &args)?.success()?;
        }
    } else {
        do_install(
            config,
            &mut deps,
            &mut exp,
            &mut install_queue,
            conflict,
            &mut devel_info,
        )?;
    }

    if !failed.is_empty() {
        let b = config.color.bold;
        let e = config.color.error;
        let len = ":: packages not in the AUR: ".len();
        let failed = failed.iter().map(|f| f.to_string());
        print!(
            "{} {}",
            e.paint("::"),
            b.paint("Packages failed to build: ")
        );
        print_indent(Style::new(), len, 4, config.cols, "  ", failed);
        Ok(1)
    } else {
        Ok(0)
    }
}

fn build_install_pkgbuild<'a>(
    config: &mut Config,
    aur_repos: &HashMap<String, String>,
    conflicts: &HashSet<&str>,
    chroot: &Chroot,
    base: &'a mut Base,
    deps: &mut Vec<&'a str>,
    exp: &mut Vec<&'a str>,
    install_queue: &mut Vec<String>,
    conflict: &mut bool,
    devel_info: &mut DevelInfo,
    new_devel_info: &mut DevelInfo,
    repo: Option<&Repository>,
) -> Result<()> {
    let c = config.color;
    let mut debug_paths = Vec::new();
    let dir = config.build_dir.join(base.package_base());

    let mut satisfied = false;

    if !config.chroot && config.batch_install {
        for pkg in &base.pkgs {
            let mut deps = pkg
                .pkg
                .depends
                .iter()
                .chain(&pkg.pkg.make_depends)
                .chain(&pkg.pkg.check_depends);

            satisfied = deps
                .find(|dep| {
                    config
                        .alpm
                        .localdb()
                        .pkgs()
                        .find_satisfier(dep.as_str())
                        .is_none()
                })
                .is_none();
        }
    }

    if !config.chroot && !satisfied {
        do_install(config, deps, exp, install_queue, *conflict, devel_info)?;
        *conflict = false;
    }

    if config.chroot {
        chroot
            .build(&dir, &["-cu"], &["-ofA"])
            .with_context(|| format!("failed to download sources for '{}'", base))?;
    } else {
        // download sources
        exec::makepkg(config, &dir, &["--verifysource", "-ACcf"])?
            .success()
            .with_context(|| format!("failed to download sources for '{}'", base))?;

        // pkgver bump
        exec::makepkg(config, &dir, &["-ofCA"])?
            .success()
            .with_context(|| format!("failed to build '{}'", base))?;
    }

    println!("{}: parsing pkg list...", base);
    let (mut pkgdest, version) = parse_package_list(config, &dir)?;

    if config.install_debug {
        let mut debug = Vec::new();
        for dest in pkgdest.values() {
            let file = dest.rsplit('/').next().unwrap();

            for pkg in &base.pkgs {
                let debug_pkg = format!("{}-debug-", pkg.pkg.name);

                if file.starts_with(&debug_pkg) {
                    let debug_pkg = format!("{}-debug", pkg.pkg.name);
                    let mut pkg = pkg.clone();
                    let mut raur_pkg = (*pkg.pkg).clone();
                    raur_pkg.name = debug_pkg;
                    pkg.pkg = raur_pkg.into();
                    debug_paths.push((pkg.pkg.name.clone(), dest));
                    debug.push(pkg);
                }
            }
        }

        base.pkgs.extend(debug);
    }

    if needs_build(config, base, &pkgdest, &version) {
        // actual build
        if config.chroot {
            chroot
                .build(
                    &dir,
                    &[],
                    &["-feA", "--noconfirm", "--noprepare", "--holdver"],
                )
                .with_context(|| format!("failed to build '{}'", base))?;
        } else {
            exec::makepkg(
                config,
                &dir,
                &["-cfeA", "--noconfirm", "--noprepare", "--holdver"],
            )?
            .success()
            .with_context(|| format!("failed to build '{}'", base))?;
        }
    } else {
        println!(
            "{} {}-{} is up to date -- skipping build",
            c.warning.paint("::"),
            base.package_base(),
            base.pkgs[0].pkg.version
        )
    }

    for (pkg, path) in debug_paths {
        if !Path::new(path).exists() {
            base.pkgs.retain(|p| p.pkg.name != pkg);
        } else {
            println!("adding {} to the install list", pkg);
        }
    }

    if let Some(ref repo) = repo {
        let pkgs = pkgdest.values().collect::<Vec<_>>();
        if let Some(repo) = aur_repos.get(base.package_base()) {
            let repo = config
                .pacman
                .repos
                .iter()
                .find(|db| db.name == *repo)
                .unwrap();
            let path = repo::file(repo).unwrap();
            let name = repo.name.clone();
            repo::add(config, path, &name, config.move_pkgs, &pkgs)?;
            repo::refresh(config, &[name])?;
        } else {
            let path = repo::file(&repo).unwrap();
            repo::add(config, path, &repo.name, config.move_pkgs, &pkgs)?;
            repo::refresh(config, &[repo.name.clone()])?;
        }
        if let Some(info) = new_devel_info.info.remove(base.package_base()) {
            devel_info
                .info
                .insert(base.package_base().to_string(), info);
        }
        if config.devel {
            save_devel_info(config, &devel_info)?;
        }
    }

    for pkg in &base.pkgs {
        if !needs_install(config, base, &version, pkg) {
            continue;
        }

        if config.args.has_arg("asexplicit", "asexplicit") {
            exp.push(pkg.pkg.name.as_str());
        } else if config.args.has_arg("asdeps", "asdeps") {
            deps.push(pkg.pkg.name.as_str());
        } else if config.alpm.localdb().pkg(&*pkg.pkg.name).is_err() {
            if pkg.target {
                exp.push(pkg.pkg.name.as_str())
            } else {
                deps.push(pkg.pkg.name.as_str())
            }
        }

        let path = pkgdest.remove(&pkg.pkg.name).with_context(|| {
            format!(
                "could not find package '{}' in package list for '{}'",
                pkg.pkg.name, base
            )
        })?;

        *conflict |= base
            .pkgs
            .iter()
            .any(|p| conflicts.contains(p.pkg.name.as_str()));
        install_queue.push(path);
    }

    if repo.is_none() {
        if let Some(info) = new_devel_info.info.remove(base.package_base()) {
            devel_info
                .info
                .insert(base.package_base().to_string(), info);
        }
    }

    Ok(())
}

fn asdeps(config: &Config, pkgs: &[&str]) -> Result<()> {
    if pkgs.is_empty() {
        return Ok(());
    }

    let mut args = config.pacman_globals();
    args.op("database")
        .arg("asdeps")
        .targets(pkgs.iter().cloned());
    let output = exec::pacman_output(config, &args)?;
    ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn asexp(config: &Config, pkgs: &[&str]) -> Result<()> {
    if pkgs.is_empty() {
        return Ok(());
    }

    let mut args = config.pacman_globals();
    args.op("database")
        .arg("asexplicit")
        .targets(pkgs.iter().cloned());
    let output = exec::pacman_output(config, &args)?;
    ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn parse_package_list(config: &Config, dir: &Path) -> Result<(HashMap<String, String>, String)> {
    let output = exec::makepkg_output(config, dir, &["--packagelist"])?;
    let output = String::from_utf8(output.stdout).context("pkgdest is not utf8")?;
    let mut pkgdests = HashMap::new();
    let mut version = String::new();

    for line in output.trim().lines() {
        let file = line.rsplit('/').next().unwrap();

        let split = file.split('-').collect::<Vec<_>>();
        ensure!(
            split.len() >= 4,
            "can't find package name in packagelist: {}",
            line
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

fn flags(config: &mut Config) -> aur_depends::Flags {
    let mut flags = Flags::new();

    if config.args.has_arg("needed", "needed") {
        flags |= Flags::NEEDED;
    }
    if config.args.count("u", "sysupgrade") > 1 {
        flags |= Flags::ENABLE_DOWNGRADE;
    }
    if config.args.count("d", "nodeps") > 0 {
        flags |= Flags::NO_DEP_VERSION;
        config.mflags.push("-d".to_string());
    }
    if config.args.count("d", "nodeps") > 1 {
        flags |= Flags::NO_DEPS;
    }
    if config.no_check {
        flags.remove(Flags::CHECK_DEPENDS);
        config.mflags.push("--nocheck".into());
    }
    if config.mode == "aur" {
        flags |= Flags::AUR_ONLY;
    }
    if config.mode == "repo" {
        flags |= Flags::REPO_ONLY;
    }
    if !config.provides {
        flags.remove(Flags::TARGET_PROVIDES | Flags::MISSING_PROVIDES);
    }
    if config.op == "yay" {
        flags.remove(Flags::TARGET_PROVIDES);
    }
    if config.repos != LocalRepos::None {
        flags |= Flags::LOCAL_REPO;
    }

    flags
}

fn resolver<'a, 'b>(
    config: &Config,
    alpm: &'a Alpm,
    raur: &'b raur::Handle,
    cache: &'b mut Cache,
    flags: Flags,
) -> Resolver<'a, 'b> {
    let devel_suffixes = config.devel_suffixes.clone();
    let c = config.color;
    let no_confirm = config.no_confirm;

    let mut resolver = aur_depends::Resolver::new(alpm, cache, raur, flags)
        .is_devel(move |pkg| devel_suffixes.iter().any(|suff| pkg.ends_with(suff)))
        .provider_callback(move |dep, pkgs| {
            let prompt = format!("There are {} providers available for {}:", pkgs.len(), dep);
            println!("{} {}", c.action.paint("::"), c.bold.paint(prompt));
            println!(
                "{} {} {}:",
                c.action.paint("::"),
                c.bold.paint("Repository"),
                color_repo(c.enabled, "AUR")
            );
            print!("    ");
            for (n, pkg) in pkgs.iter().enumerate() {
                print!("{}) {}  ", n + 1, pkg);
            }

            get_provider(pkgs.len())
        })
        .group_callback(move |groups| {
            let total: usize = groups.iter().map(|g| g.group.packages().len()).sum();
            let mut pkgs = Vec::new();
            println!(
                "{} {} {}:",
                c.action.paint("::"),
                c.bold
                    .paint(format!("There are {} members in group", total)),
                c.group.paint(groups[0].group.name()),
            );

            let mut repo = String::new();

            for group in groups {
                if group.db.name() != repo {
                    repo = group.db.name().to_string();
                    println!(
                        "{} {} {}",
                        c.action.paint("::"),
                        c.bold.paint("Repository"),
                        color_repo(c.enabled, group.db.name())
                    );
                    print!("    ");
                }

                let mut n = 1;
                for pkg in group.group.packages() {
                    print!("{}) {}  ", n, pkg.name());
                    n += 1;
                }
            }

            print!("\n\nEnter a selection (default=all): ");
            let _ = stdout().lock().flush();

            let stdin = stdin();
            let mut stdin = stdin.lock();
            let mut input = String::new();

            input.clear();
            if !no_confirm {
                let _ = stdin.read_line(&mut input);
            }

            let menu = NumberMenu::new(input.trim());
            let mut n = 1;

            for pkg in groups.iter().flat_map(|g| g.group.packages()) {
                if menu.contains(n, "") {
                    pkgs.push(pkg);
                }
                n += 1;
            }

            pkgs
        });

    resolver.custom_aur_namespace(config.aur_namespace().to_string());
    resolver
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

    if config.mode == "repo" {
        return;
    }

    if config.args.has_arg("u", "sysupgrade") {
        let repos = repo::configured_local_repos(config);
        let mut pkgs = Vec::new();
        let pkgs = if !repos.is_empty() {
            pkgs.clear();

            for db in config.alpm.syncdbs() {
                if repos.contains(&db.name()) {
                    pkgs.extend(db.pkgs().iter());
                }
            }
            pkgs
        } else {
            let mut pkgs = config.alpm.localdb().pkgs().iter().collect::<Vec<_>>();
            pkgs.retain(|pkg| config.alpm.syncdbs().pkg(pkg.name()).is_err());
            pkgs
        };

        warnings.missing = pkgs
            .iter()
            .filter(|pkg| !cache.contains(pkg.name()))
            .filter(|pkg| !is_debug(**pkg))
            .map(|pkg| pkg.name())
            .filter(|pkg| !config.no_warn.iter().any(|nw| nw == pkg))
            .collect::<Vec<_>>();

        warnings.ood = pkgs
            .iter()
            .filter(|pkg| !is_debug(**pkg))
            .filter_map(|pkg| cache.get(pkg.name()))
            .filter(|pkg| pkg.out_of_date.is_some())
            .map(|pkg| pkg.name.as_str())
            .filter(|pkg| !config.no_warn.iter().any(|nw| nw == pkg))
            .collect::<Vec<_>>();

        warnings.orphans = pkgs
            .iter()
            .filter(|pkg| !is_debug(**pkg))
            .filter_map(|pkg| cache.get(pkg.name()))
            .filter(|pkg| pkg.maintainer.is_none())
            .map(|pkg| pkg.name.as_str())
            .filter(|pkg| !config.no_warn.iter().any(|nw| nw == pkg))
            .collect::<Vec<_>>();
    }

    if let Some(actions) = actions {
        warnings.ood.extend(
            actions
                .iter_build_pkgs()
                .map(|pkg| &pkg.pkg)
                .filter(|pkg| pkg.out_of_date.is_some())
                .filter(|pkg| !config.no_warn.iter().any(|nw| *nw == pkg.name))
                .map(|pkg| pkg.name.as_str()),
        );

        warnings.orphans.extend(
            actions
                .iter_build_pkgs()
                .map(|pkg| &pkg.pkg)
                .filter(|pkg| pkg.maintainer.is_none())
                .filter(|pkg| !config.no_warn.iter().any(|nw| *nw == pkg.name))
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

fn needs_build(
    config: &Config,
    base: &Base,
    pkgdest: &HashMap<String, String>,
    version: &str,
) -> bool {
    if (config.rebuild == "yes" && base.pkgs.iter().any(|p| p.target)) || config.rebuild == "all" {
        return true;
    }

    if config.args.has_arg("needed", "needed") {
        let mut all_installed = true;
        let c = config.color;

        if config.repos != LocalRepos::None {
            let dbs = config.alpm.syncdbs();
            let repos = repo::configured_local_repos(config);

            for pkg in &base.pkgs {
                for repo in &repos {
                    let repo = dbs.iter().find(|db| db.name() == *repo).unwrap();
                    if let Ok(pkg) = repo.pkg(pkg.pkg.name.as_str()) {
                        if pkg.version() != version {
                            return true;
                        }
                    } else {
                        return true;
                    }
                }
            }
        } else {
            for pkg in &base.pkgs {
                if let Ok(pkg) = config.alpm.localdb().pkg(&*pkg.pkg.name) {
                    if pkg.version() == version {
                        continue;
                    }
                }

                all_installed = false;
                break;
            }

            if all_installed {
                println!(
                    "{} {}-{} is up to date -- skipping",
                    c.warning.paint("::"),
                    base.package_base(),
                    base.pkgs[0].pkg.version
                );
                return false;
            }
        }
    }

    !base
        .pkgs
        .iter()
        .all(|p| Path::new(pkgdest.get(&p.pkg.name).unwrap()).exists())
}

fn needs_install(config: &Config, base: &Base, version: &str, pkg: &AurPackage) -> bool {
    if config.args.has_arg("needed", "needed") {
        if let Ok(pkg) = config.alpm.localdb().pkg(&*pkg.pkg.name) {
            if pkg.version().as_str() == version {
                let c = config.color;
                println!(
                    "{} {}-{} is up to date -- skipping install",
                    c.warning.paint("::"),
                    base.package_base(),
                    base.pkgs[0].pkg.version
                );
                return false;
            }
        }
    }

    true
}

fn update_aur_list(config: &Config) {
    let url = config.aur_url.clone();
    let dir = config.cache_dir.clone();
    let interval = config.completion_interval;

    tokio::spawn(async move {
        let _ = update_aur_cache(&url, &dir, Some(interval)).await;
    });
}
