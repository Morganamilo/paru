#![cfg_attr(feature = "backtrace", feature(backtrace))]

mod args;
mod chroot;
mod clean;
mod command_line;
mod completion;
mod config;
mod devel;
mod download;
mod exec;
mod fmt;
mod help;
mod info;
mod install;
mod keys;
mod news;
mod order;
mod query;
mod remove;
mod repo;
mod search;
mod stats;
mod sync;
mod upgrade;
mod util;

#[cfg(feature = "mock")]
mod mock;

#[cfg(not(feature = "mock"))]
type RaurHandle = raur::Handle;
#[cfg(feature = "mock")]
type RaurHandle = crate::mock::Mock;

#[macro_use]
extern crate smart_default;

use crate::chroot::Chroot;
use crate::config::{Config, Op};
use crate::query::print_upgrade_list;

use std::collections::HashMap;
use std::env;
use std::error::Error as StdError;
use std::fs::{read_dir, read_to_string};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use ansi_term::Style;
use anyhow::{bail, Error, Result};
use cini::Ini;
use tr::{tr, tr_init};

#[macro_export]
macro_rules! printtr {
    ($($tail:tt)* ) => {{
        println!("{}", ::tr::tr!($($tail)*));
    }};
}

fn debug_enabled() -> bool {
    env::var("PARU_DEBUG").as_deref().unwrap_or("0") != "0"
}

fn alpm_debug_enabled() -> bool {
    debug_enabled() && env::var("PARU_ALPM_DEBUG").as_deref().unwrap_or("1") != "0"
}

fn print_error(color: Style, err: Error) {
    #[cfg(feature = "backtrace")]
    {
        let backtrace = err.backtrace();

        if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
            eprint!("{}", backtrace);
        }
    }
    let mut iter = err.chain().peekable();

    if <dyn StdError>::is::<exec::PacmanError>(*iter.peek().unwrap())
        || <dyn StdError>::is::<exec::Status>(*iter.peek().unwrap())
    {
        eprint!("{}", iter.peek().unwrap());
        return;
    }

    eprint!("{} ", color.paint(tr!("error:")));
    while let Some(link) = iter.next() {
        eprint!("{}", link);
        if iter.peek().is_some() {
            eprint!(": ");
        }
    }
    eprintln!();
}

pub async fn run<S: AsRef<str>>(args: &[S]) -> i32 {
    tr_init!(env::var("LOCALE_DIR")
        .as_deref()
        .unwrap_or("/usr/share/locale/"));
    if debug_enabled() {
        env_logger::Builder::new()
            .filter_level(log::LevelFilter::Debug)
            .format(|buf, record| {
                writeln!(
                    buf,
                    "{}: <{}> {}",
                    record.level().to_string().to_lowercase(),
                    record.module_path().unwrap_or("unknown"),
                    record.args()
                )
            })
            .format_timestamp(None)
            .init();
    }

    let _ = &*exec::DEFAULT_SIGNALS;
    let _ = &*exec::RAISE_SIGPIPE;

    let mut config = match Config::new() {
        Ok(config) => config,
        Err(err) => {
            print_error(Style::new(), err);
            return 1;
        }
    };

    match run2(&mut config, args).await {
        Err(err) => {
            print_error(config.color.error, err);
            1
        }
        Ok(ret) => ret,
    }
}

async fn run2<S: AsRef<str>>(config: &mut Config, args: &[S]) -> Result<i32> {
    if let Some(ref config_path) = config.config_path {
        let file = read_to_string(config_path)?;
        let name = config_path.display().to_string();
        config.parse(Some(name.as_str()), &file)?;
    };

    if args.is_empty() {
        config.parse_args(&["-Syu"])?;
    } else {
        config.parse_args(args)?;
    }

    handle_cmd(config).await
}

async fn handle_cmd(config: &mut Config) -> Result<i32> {
    if (config.op == Op::ChrootCtl || config.chroot)
        && Command::new("arch-nspawn").arg("-h").output().is_err()
    {
        bail!(tr!("can not use chroot builds: devtools is not installed"));
    }

    let ret = match config.op {
        Op::Database | Op::Files => exec::pacman(config, &config.args)?.code(),
        Op::Upgrade => handle_upgrade(config).await?,
        Op::Query => handle_query(config).await?,
        Op::Sync => handle_sync(config).await?,
        Op::Remove => handle_remove(config)?,
        Op::DepTest => handle_test(config).await?,
        Op::GetPkgBuild => handle_get_pkg_build(config).await?,
        Op::Show => handle_show(config).await?,
        Op::Yay => handle_yay(config).await?,
        Op::RepoCtl => handle_repo(config)?,
        Op::ChrootCtl => handle_chroot(config)?,
        // _ => bail!("unknown op '{}'", config.op),
    };

    Ok(ret)
}

async fn handle_upgrade(config: &mut Config) -> Result<i32> {
    if config.targets.is_empty() {
        install::build_pkgbuild(config).await
    } else {
        Ok(exec::pacman(config, &config.args)?.code())
    }
}

async fn handle_query(config: &mut Config) -> Result<i32> {
    let args = &config.args;
    if args.has_arg("u", "upgrades") {
        print_upgrade_list(config).await
    } else {
        Ok(exec::pacman(config, args)?.code())
    }
}

async fn handle_show(config: &mut Config) -> Result<i32> {
    if config.news > 0 {
        news::news(config).await
    } else if config.complete {
        Ok(completion::print(config, None).await)
    } else if config.stats {
        stats::stats(config).await
    } else if config.order {
        order::order(config).await
    } else {
        Ok(0)
    }
}

async fn handle_get_pkg_build(config: &mut Config) -> Result<i32> {
    if config.print {
        download::show_pkgbuilds(config).await
    } else if config.comments {
        download::show_comments(config).await
    } else {
        download::getpkgbuilds(config).await
    }
}

async fn handle_yay(config: &mut Config) -> Result<i32> {
    if config.gendb {
        devel::gendb(config).await?;
        Ok(0)
    } else if config.clean > 0 {
        config.need_root = true;
        let unneeded = util::unneeded_pkgs(config, config.clean == 1);
        if !unneeded.is_empty() {
            let mut args = config.pacman_args();
            args.remove("c").remove("clean");
            args.targets = unneeded;
            args.op = "remove";
            Ok(exec::pacman(config, &args)?.code())
        } else {
            printtr!(" there is nothing to do");
            Ok(0)
        }
    } else if !config.targets.is_empty() {
        search::search_install(config).await
    } else {
        bail!(tr!("no operation specified (use -h for help)"));
    }
}

fn handle_remove(config: &mut Config) -> Result<i32> {
    remove::remove(config)
}

async fn handle_test(config: &Config) -> Result<i32> {
    if config.aur_filter {
        sync::filter(config).await
    } else {
        Ok(exec::pacman(config, &config.args)?.code())
    }
}

async fn handle_sync(config: &mut Config) -> Result<i32> {
    if config.args.has_arg("i", "info") {
        info::info(config, config.args.count("i", "info") > 1).await
    } else if config.args.has_arg("c", "clean") {
        clean::clean(config)?;
        Ok(0)
    } else if config.args.has_arg("l", "list") {
        sync::list(config).await
    } else if config.args.has_arg("s", "search") {
        search::search(config).await
    } else if config.args.has_arg("g", "groups")
        || config.args.has_arg("p", "print")
        || config.args.has_arg("p", "print-format")
    {
        Ok(exec::pacman(config, &config.args)?.code())
    } else {
        let target = std::mem::take(&mut config.targets);
        install::install(config, &target).await
    }
}

fn handle_repo(config: &mut Config) -> Result<i32> {
    use std::os::unix::ffi::OsStrExt;

    let repoc = config.color.sl_repo;
    let pkgc = config.color.sl_pkg;
    let version = config.color.sl_version;
    let installedc = config.color.sl_installed;

    let (_, repos) = repo::repo_aur_dbs(config);
    let repos = repos
        .into_iter()
        .map(|r| r.name().to_string())
        .filter(|r| config.delete >= 1 || config.targets.is_empty() || config.targets.contains(r))
        .collect::<Vec<_>>();

    if config.refresh || config.sysupgrade {
        repo::refresh(config, &repos)?;
    }

    let (_, mut repos) = repo::repo_aur_dbs(config);
    repos.retain(|r| {
        config.delete >= 1
            || config.uninstall
            || config.targets.is_empty()
            || config.targets.contains(&r.name().to_string())
    });

    if config.delete >= 1 {
        let mut remove = HashMap::<&str, Vec<&str>>::new();
        let mut rmfiles = Vec::new();
        for repo in &repos {
            for pkg in repo.pkgs() {
                if config.targets.iter().any(|p| p == pkg.name()) {
                    remove.entry(repo.name()).or_default().push(pkg.name());
                }
            }
        }

        let cb = config.alpm.take_raw_log_cb();
        for repo in &repos {
            if let Some(pkgs) = remove.get(&repo.name()) {
                let path = repo
                    .servers()
                    .first()
                    .unwrap()
                    .trim_start_matches("file://");
                repo::remove(config, path, repo.name(), pkgs)?;

                let files = read_dir(path)?;

                for file in files {
                    let file = file?;
                    if let Ok(pkg) = config.alpm.pkg_load(
                        file.path().as_os_str().as_bytes(),
                        false,
                        alpm::SigLevel::NONE,
                    ) {
                        if pkgs.contains(&pkg.name()) {
                            rmfiles.push(file.path());

                            let mut sig = file.path().to_path_buf().into_os_string();
                            sig.push(".sig");
                            let sig = PathBuf::from(sig);
                            if sig.exists() {
                                rmfiles.push(sig);
                            }
                        }
                    }
                }
            }
        }
        config.alpm.set_raw_log_cb(cb);

        if !rmfiles.is_empty() {
            let mut cmd = Command::new(&config.sudo_bin);
            cmd.arg("rm").args(rmfiles);
            exec::command(&mut cmd)?;
        }

        let repos = repos
            .into_iter()
            .map(|r| r.name().to_string())
            .collect::<Vec<_>>();
        repo::refresh(config, &repos)?;

        if config.delete >= 2 {
            config.need_root = true;
            let db = config.alpm.localdb();
            let pkgs = config
                .targets
                .iter()
                .map(|p| p.as_str())
                .filter(|p| db.pkg(*p).is_ok());

            let mut args = config.pacman_globals();
            args.op("remove");
            args.targets = pkgs.collect();
            if !args.targets.is_empty() {
                exec::pacman(config, &args)?.success()?;
            }
        }

        return Ok(0);
    }

    if config.refresh || config.sysupgrade {
        return Ok(0);
    }

    let (_, mut repos) = repo::repo_aur_dbs(config);
    repos.retain(|r| {
        config.delete >= 1
            || config.targets.is_empty()
            || config.targets.contains(&r.name().to_string())
    });

    for repo in repos {
        if config.list {
            for pkg in repo.pkgs() {
                if config.quiet {
                    println!("{}", pkg.name());
                } else {
                    print!(
                        "{} {} {}",
                        repoc.paint(repo.name()),
                        pkgc.paint(pkg.name()),
                        version.paint(pkg.version().as_str())
                    );
                    let local_pkg = config.alpm.localdb().pkg(pkg.name());

                    if let Ok(local_pkg) = local_pkg {
                        let installed = if local_pkg.version() != pkg.version() {
                            tr!(" [installed: {}]", local_pkg.version())
                        } else {
                            tr!(" [installed]")
                        };
                        print!("{}", installedc.paint(installed));
                    }
                    println!();
                }
            }
        } else if config.quiet {
            println!("{}", repo.name());
        } else {
            println!(
                "{} {}",
                repo.name(),
                repo.servers()
                    .first()
                    .unwrap()
                    .trim_start_matches("file://")
            );
        }
    }

    Ok(0)
}

fn handle_chroot(config: &Config) -> Result<i32> {
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
        mflags: config.mflags.clone(),
        ro: repo::all_files(config),
        rw: config.pacman.cache_dir.clone(),
    };

    if !chroot.exists() {
        chroot.create(config, &["base-devel"])?;
    }

    if config.sysupgrade {
        chroot.update()?;
    }

    if config.install {
        let mut args = vec!["pacman", "-S"];
        args.extend(config.targets.iter().map(|s| s.as_str()));
        chroot.run(&args)?;
    } else if !config.sysupgrade || !config.targets.is_empty() {
        chroot.run(&config.targets)?;
    }
    Ok(0)
}
