use crate::args::Arg;
use crate::clean::clean_untracked;
use crate::config::Config;
use crate::devel::{fetch_devel_info, load_devel_info, save_devel_info, DevelInfo};
use crate::download::{self, Bases};
use crate::fmt::{color_repo, print_indent};
use crate::keys::check_pgp_keys;
use crate::print_error;
use crate::upgrade::get_upgrades;
use crate::util::{ask, get_provider, split_repo_aur_targets, NumberMenu};
use crate::{args, exec};

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::io::{stdin, stdout, BufRead, Write};
use std::iter::FromIterator;
use std::path::Path;
use std::process::Command;

use alpm::Alpm;
use alpm_utils::{DbListExt, Targ};
use ansi_term::Style;
use anyhow::{bail, ensure, Context, Result};
use aur_depends::{Actions, AurUpdates, Conflict, Flags, RepoPackage, Resolver};
use raur_ext::Cache;
use srcinfo::Srcinfo;

fn early_refresh(config: &Config) -> Result<()> {
    let mut args = config.pacman_globals();
    args.arg("y");
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

pub fn install(config: &mut Config, targets_str: &[String]) -> Result<i32> {
    let mut cache = raur_ext::Cache::new();
    let c = config.color;

    if config.sudo_loop {
        exec::spawn_sudo(config.sudo_bin.clone(), config.sudo_flags.clone())?;
    }

    config.op = "sync".to_string();
    config.args.op = config.op.clone();
    config.globals.op = config.op.clone();
    config.targets = targets_str.to_vec();
    config.args.targets = config.targets.clone();

    let targets = args::parse_targets(&targets_str);
    let (mut repo_targets, aur_targets) = split_repo_aur_targets(config, &targets);

    if repo_targets.is_empty()
        && aur_targets.is_empty()
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
            early_pacman(config, targets)?;
        }
    }

    if !config.combined_upgrade && aur_targets.is_empty() && !config.args.has_arg("u", "sysupgrade")
    {
        return Ok(0);
    }

    config.init_alpm()?;

    let flags = flags(config);
    let mut resolver = resolver(&config, &config.alpm, &config.raur, &mut cache, flags);

    let upgrades = if config.args.has_arg("u", "sysupgrade") {
        let aur_upgrades = if config.mode != "repo" {
            println!(
                "{} {}",
                c.action.paint("::"),
                c.bold.paint("Looking for AUR upgrades")
            );
            resolver.aur_updates()?
        } else {
            AurUpdates::default()
        };

        for pkg in aur_upgrades.ignored {
            eprintln!(
                "{} {}: ignoring package upgrade ({} => {})",
                c.warning.paint("warning:"),
                pkg.local.name(),
                pkg.local.version(),
                pkg.remote.version
            );
        }

        let upgrades = get_upgrades(config, aur_upgrades.updates)?;
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
    targets.extend(upgrades.aur_keep.iter().map(Targ::from));
    targets.extend(upgrades.repo_keep.iter().map(Targ::from));

    // No aur stuff, let's just use pacman
    if aur_targets.is_empty() && upgrades.aur_keep.is_empty() && config.combined_upgrade {
        let mut args = config.pacman_args();
        let targets = targets.iter().map(|t| t.to_string()).collect::<Vec<_>>();
        args.targets = targets.iter().map(|s| s.as_str()).collect();
        args.remove("y").remove("refresh");

        let code = exec::pacman(config, &args)?.code();
        return Ok(code);
    }

    if targets.is_empty() {
        print_warnings(config, &cache, None);
        println!(" there is nothing to do");
        return Ok(0);
    }

    println!(
        "{} {}",
        c.action.paint("::"),
        c.bold.paint("Resolving dependencies...")
    );

    let mut actions = resolver.resolve_targets(&targets)?;

    if !actions.build.is_empty() && nix::unistd::getuid().is_root() {
        bail!("can't install AUR package as root");
    }

    let conflicts = check_actions(config, &actions)?;

    print_warnings(config, &cache, Some(&actions));

    if actions.build.is_empty() && actions.install.is_empty() {
        if config.args.has_arg("u", "sysupgrade") || !aur_targets.is_empty() {
            println!(" there is nothing to do");
        }
        return Ok(0);
    }

    print_install(config, &actions);

    let remove_make = if actions.iter_build_pkgs().any(|p| p.make) {
        if config.remove_make == "ask" {
            println!();
            ask(config, "Remove make dependencies after install?", false)
        } else {
            config.remove_make == "yes"
        }
    } else {
        false
    };

    let err = install_actions(config, &mut actions, &conflicts.0, &conflicts.1);

    if remove_make {
        let mut args = config.pacman_globals();
        args.op("remove").arg("noconfirm");
        args.targets = actions
            .iter_build_pkgs()
            .filter(|p| p.make)
            .map(|p| p.pkg.name.as_str())
            .collect();

        if let Err(err) = exec::pacman(config, &args) {
            print_error(config.color.error, err);
        }
    }

    if config.clean_after {
        for base in &actions.build {
            let path = config.build_dir.join(base.package_base());
            if let Err(err) = clean_untracked(config, &path) {
                print_error(config.color.error, err);
            }
        }
    }

    err
}

fn install_actions(
    config: &Config,
    actions: &mut Actions,
    conflicts: &[Conflict],
    inner_conflicts: &[Conflict],
) -> Result<i32> {
    if !ask(config, "Proceed to review?", true) {
        return Ok(1);
    }

    let bases = Bases::from_iter(actions.iter_build_pkgs().map(|p| p.pkg.clone()));
    let mut srcinfos = HashMap::new();

    for base in &bases.bases {
        let path = config.build_dir.join(base.package_base()).join(".SRCINFO");
        if path.exists() {
            let srcinfo = Srcinfo::parse_file(path)
                .with_context(|| format!("failed to parse srcinfo for '{}'", base))?;
            srcinfos.insert(srcinfo.base.pkgbase.to_string(), srcinfo);
        }
    }

    download::new_aur_pkgbuilds(config, &bases, &srcinfos)?;

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

    let pkgs = actions
        .build
        .iter()
        .map(|b| b.package_base())
        .collect::<Vec<_>>();

    if let Some(ref fm) = config.fm {
        file_manager(config, fm, &pkgs)?;
    } else {
        let mut printed = false;
        let unseen = config.fetch.unseen(&pkgs)?;
        let has_diff = config.fetch.has_diff(&unseen)?;
        for pkg in &has_diff {
            config.fetch.print_diff(pkg)?;
            print!("\n\n\n");
            printed = true;
        }
        for pkg in &unseen {
            if !has_diff.contains(pkg) {
                let path = config.build_dir.join(pkg).join("PKGBUILD");

                let bat = config.color.enabled && Command::new("bat").arg("-V").output().is_ok();

                if bat {
                    Command::new("bat")
                        .arg("-pp")
                        .arg("--color=always")
                        .arg("-lPKGBUILD")
                        .arg(path)
                        .spawn()?
                        .wait()?;
                } else {
                    let pkgbuild = std::fs::read_to_string(&path)
                        .context(format!("failed to open {}", path.display()))?;
                    print!("{}\n\n\n", pkgbuild);
                }
                printed = true;
            }
        }

        if !printed {
            println!(" nothing new to review");
        }
    }

    if !ask(config, "Proceed with installation?", true) {
        return Ok(1);
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

    repo_install(config, &actions.install)?;

    let url = config.aur_url.clone();
    let cache_dir = config.cache_dir.clone();
    let interval = config.completion_interval;
    std::thread::spawn(move || {
        crate::completion::update_aur_cache(&url, &cache_dir, Some(interval)).unwrap();
    });

    let conflicts = conflicts
        .iter()
        .map(|c| c.pkg.as_str())
        .chain(inner_conflicts.iter().map(|c| c.pkg.as_str()))
        .collect::<HashSet<_>>();

    //download_pkgbuild_sources(config, &actions.build)?;
    build_install_pkgbuilds(config, &mut actions.build, srcinfos, &bases, &conflicts)?;

    Ok(0)
}

fn file_manager(config: &Config, fm: &str, pkgs: &[&str]) -> Result<()> {
    let has_diff = config.fetch.has_diff(pkgs)?;
    config.fetch.save_diffs(&has_diff)?;
    let view = config.fetch.make_view(pkgs, &has_diff)?;

    let ret = Command::new(fm)
        .args(&config.fm_flags)
        .arg(view.path())
        .current_dir(view.path())
        .spawn()?
        .wait()?;
    ensure!(ret.success(), "file manager did not execute successfully");
    Ok(())
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
            let stack = if missing.stack.is_empty() {
                "target".to_string()
            } else {
                missing.stack.join(" -> ")
            };
            err.push_str(&format!(
                "\n    {} ({})",
                c.error.paint(&missing.dep),
                stack
            ));
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

    println!(
        "{} {}",
        c.action.paint("::"),
        c.bold.paint("Calculating conflicts...")
    );
    let conflicts = actions.calculate_conflicts();
    println!(
        "{} {}",
        c.action.paint("::"),
        c.bold.paint("Calculating inner conflicts...")
    );
    let inner_conflicts = actions.calculate_inner_conflicts()?;

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

fn build_install_pkgbuilds(
    config: &Config,
    build: &mut [aur_depends::Base],
    srcinfos: HashMap<String, Srcinfo>,
    bases: &Bases,
    conflicts: &HashSet<&str>,
) -> Result<()> {
    let mut deps = Vec::new();
    let mut exp = Vec::new();
    let mut install_queue = Vec::new();
    let mut conflict = false;
    let c = config.color;

    let (mut devel_info, mut new_devel_info) = if config.devel {
        println!("fetching devel info...");
        (
            load_devel_info(config)?.unwrap_or_default(),
            fetch_devel_info(config, bases, srcinfos)?,
        )
    } else {
        (DevelInfo::default(), DevelInfo::default())
    };

    for base in build {
        let dir = config.build_dir.join(base.package_base());

        let mut satisfied = false;

        if config.batch_install {
            for pkg in &base.pkgs {
                let mut deps = pkg
                    .pkg
                    .depends
                    .iter()
                    .chain(&pkg.pkg.make_depends)
                    .chain(&pkg.pkg.check_depends);

                satisfied = deps
                    .find(|dep| config.alpm.localdb().pkgs().find_satisfier(*dep).is_none())
                    .is_none();
            }
        }

        if !satisfied {
            do_install(
                config,
                &mut deps,
                &mut exp,
                &mut install_queue,
                conflict,
                &mut devel_info,
            )?;
            conflict = false;
        }

        // download sources
        exec::makepkg(config, &dir, &["--verifysource", "-ACcf"])?
            .success()
            .with_context(|| format!("failed to download sources for '{}'", base))?;

        // pkgver bump
        exec::makepkg(config, &dir, &["-ofCA"])?
            .success()
            .with_context(|| format!("failed to build '{}'", base))?;

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
                        println!("adding {} to the install list", debug_pkg);
                        let mut pkg = pkg.clone();
                        let mut raur_pkg = (*pkg.pkg).clone();
                        raur_pkg.name = debug_pkg;
                        pkg.pkg = raur_pkg.into();
                        debug.push(pkg);
                    }
                }
            }

            base.pkgs.extend(debug);
        }

        if config.args.has_arg("needed", "needed") {
            let mut all_installed = true;

            for pkg in &base.pkgs {
                if let Ok(pkg) = config.alpm.localdb().pkg(&pkg.pkg.name) {
                    if pkg.version().as_str() == version {
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
                continue;
            }
        }

        let needs_build = base
            .pkgs
            .iter()
            .any(|p| !Path::new(pkgdest.get(&p.pkg.name).unwrap()).exists());

        if needs_build
            || (config.rebuild == "yes" && base.pkgs.iter().any(|p| p.target))
            || config.rebuild == "all"
        {
            // actual build
            exec::makepkg(
                config,
                &dir,
                &["-cfeA", "--noconfirm", "--noprepare", "--holdver"],
            )?
            .success()
            .with_context(|| format!("failed to build '{}'", base))?;
        }

        if !needs_build {
            println!(
                "{} {}-{} is up to date -- skipping build",
                c.warning.paint("::"),
                base.package_base(),
                base.pkgs[0].pkg.version
            )
        }

        for pkg in &base.pkgs {
            if config.args.has_arg("needed", "needed") {
                if let Ok(pkg) = config.alpm.localdb().pkg(&pkg.pkg.name) {
                    if pkg.version().as_str() == version {
                        println!(
                            "{} {}-{} is up to date -- skipping install",
                            c.warning.paint("::"),
                            base.package_base(),
                            base.pkgs[0].pkg.version
                        );
                        continue;
                    }
                }
            }

            if config.globals.has_arg("asexplicit", "asexplicit") {
                exp.push(pkg.pkg.name.as_str());
            } else if config.globals.has_arg("asdeps", "asdeps") {
                deps.push(pkg.pkg.name.as_str());
            } else if config.alpm.localdb().pkg(&pkg.pkg.name).is_err() {
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

            conflict |= base
                .pkgs
                .iter()
                .any(|p| conflicts.contains(p.pkg.name.as_str()));
            install_queue.push(path);
        }

        if let Some(info) = new_devel_info.info.remove(base.package_base()) {
            devel_info
                .info
                .insert(base.package_base().to_string(), info);
        }
    }

    do_install(
        config,
        &mut deps,
        &mut exp,
        &mut install_queue,
        conflict,
        &mut devel_info,
    )?;

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

    aur_depends::Resolver::new(alpm, cache, raur, flags)
        .is_devel(move |pkg| devel_suffixes.iter().any(|suff| pkg.ends_with(suff)))
        .provider_callback(move |dep, pkgs| {
            let prompt = format!("There are {} providers avaliable for {}:", pkgs.len(), dep);
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
        })
}

fn print_warnings(config: &Config, cache: &Cache, actions: Option<&Actions>) {
    let mut warnings = crate::download::Warnings::default();

    if config.args.has_arg("u", "sysupgrade") {
        let pkgs = config.alpm.localdb().pkgs();

        warnings.missing = pkgs
            .iter()
            .filter(|pkg| config.alpm.syncdbs().pkg(pkg.name()).is_err())
            .filter(|pkg| !cache.contains(pkg.name()))
            .map(|pkg| pkg.name())
            .filter(|pkg| !config.no_warn.iter().any(|nw| nw == pkg))
            .collect::<Vec<_>>();

        warnings.ood = pkgs
            .iter()
            .filter(|pkg| config.alpm.syncdbs().pkg(pkg.name()).is_err())
            .filter_map(|pkg| cache.get(pkg.name()))
            .filter(|pkg| pkg.out_of_date.is_some())
            .map(|pkg| pkg.name.as_str())
            .filter(|pkg| !config.no_warn.iter().any(|nw| nw == pkg))
            .collect::<Vec<_>>();

        warnings.orphans = pkgs
            .iter()
            .filter(|pkg| config.alpm.syncdbs().pkg(pkg.name()).is_err())
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
