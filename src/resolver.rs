use crate::config::{Alpm, Config, LocalRepos, Op, YesNoAll, YesNoAllTree};
use crate::fmt::color_repo;
use crate::util::{get_provider, NumberMenu};
use crate::RaurHandle;

use std::io::{stdin, stdout, BufRead, Write};

use aur_depends::{Flags, PkgbuildRepo, Resolver};
use raur::Cache;
use tr::tr;

pub fn flags(config: &mut Config) -> aur_depends::Flags {
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
    if !config.mode.pkgbuild() {
        flags &= !Flags::PKGBUILDS;
    }
    if !config.mode.aur() {
        flags &= !Flags::AUR;
    }
    if !config.mode.repo() {
        flags &= !Flags::REPO;
    }
    match config.provides {
        YesNoAll::Yes => flags |= Flags::TARGET_PROVIDES | Flags::MISSING_PROVIDES,
        YesNoAll::No => flags.remove(
            Flags::PROVIDES
                | Flags::MISSING_PROVIDES
                | Flags::TARGET_PROVIDES
                | Flags::NON_TARGET_PROVIDES,
        ),
        YesNoAll::All => flags |= Flags::PROVIDES,
    }
    if config.op == Op::Default {
        flags.remove(Flags::TARGET_PROVIDES);
    }
    if config.repos != LocalRepos::None || config.rebuild == YesNoAllTree::Tree || config.chroot {
        flags |= Flags::RESOLVE_SATISFIED_PKGBUILDS;
    }

    log::debug!("AUR depends flags: {:?}", flags);
    flags
}

pub fn resolver<'a, 'b>(
    config: &Config,
    alpm: &'a Alpm,
    raur: &'b RaurHandle,
    cache: &'b mut Cache,
    pkgbuild_repos: Vec<PkgbuildRepo<'a>>,
    flags: Flags,
) -> Resolver<'a, 'b, RaurHandle> {
    let devel_suffixes = config.devel_suffixes.clone();
    let c = config.color;
    let no_confirm = config.no_confirm;

    let mut resolver = aur_depends::Resolver::new(alpm, cache, raur, flags)
        .pkgbuild_repos(pkgbuild_repos)
        .custom_aur_namespace(Some(config.aur_namespace().to_string()))
        .is_devel(move |pkg| devel_suffixes.iter().any(|suff| pkg.ends_with(suff)))
        .group_callback(move |groups| {
            let total: usize = groups.iter().map(|g| g.group.packages().len()).sum();
            let mut pkgs = Vec::new();
            println!(
                "{} {} {}:",
                c.action.paint("::"),
                c.bold.paint(tr!("There are {} members in group", total)),
                c.group.paint(groups[0].group.name()),
            );

            let mut repo = String::new();

            for group in groups {
                if group.db.name() != repo {
                    repo = group.db.name().to_string();
                    println!(
                        "{} {} {}",
                        c.action.paint("::"),
                        c.bold.paint(tr!("Repository")),
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

            print!("{}", tr!("\n\nEnter a selection (default=all): "));
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

    if !config.args.has_arg("u", "sysupgrade") {
        resolver = resolver.provider_callback(move |dep, pkgs| {
            let prompt = tr!(
                "There are {n} providers available for {pkg}:",
                n = pkgs.len(),
                pkg = dep
            );
            println!("{} {}", c.action.paint("::"), c.bold.paint(prompt));
            println!(
                "{} {} {}:",
                c.action.paint("::"),
                c.bold.paint(tr!("Repository")),
                color_repo(c.enabled, "AUR")
            );
            print!("    ");
            for (n, pkg) in pkgs.iter().enumerate() {
                print!("{}) {}  ", n + 1, pkg);
            }

            get_provider(pkgs.len(), no_confirm)
        });
    }

    resolver
}
