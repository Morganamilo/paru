#![cfg_attr(feature = "backtrace", feature(backtrace))]

mod args;
mod clean;
mod command_line;
mod completion;
mod config;
mod devel;
mod download;
mod exec;
mod fmt;
mod info;
mod install;
mod keys;
mod news;
mod query;
mod remove;
mod repo;
mod search;
mod sync;
mod upgrade;
mod util;

#[macro_use]
extern crate smart_default;

use crate::config::Config;
use crate::query::print_upgrade_list;

use std::error::Error as StdError;
use std::fs::read_to_string;

use ansi_term::Style;
use anyhow::{bail, Error, Result};
use cini::Ini;

use nix::sys::signal::{signal, SigHandler, Signal};

fn print_error(color: Style, err: Error) {
    #[cfg(feature = "backtrace")]
    {
        let backtrace = err.backtrace();

        if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
            eprint!("{}", backtrace);
        }
    }
    let mut iter = err.chain().peekable();

    if StdError::is::<exec::PacmanError>(*iter.peek().unwrap())
        || StdError::is::<exec::Status>(*iter.peek().unwrap())
    {
        eprint!("{}", iter.peek().unwrap());
        return;
    }

    eprint!("{} ", color.paint("error:"));
    while let Some(link) = iter.next() {
        eprint!("{}", link);
        if iter.peek().is_some() {
            eprint!(": ");
        }
    }
    eprintln!();
}

#[tokio::main]
async fn main() {
    unsafe { signal(Signal::SIGPIPE, SigHandler::SigDfl).unwrap() };

    let i = main2().await;
    std::process::exit(i);
}

async fn main2() -> i32 {
    //env_logger::init();
    let mut config = match Config::new() {
        Ok(config) => config,
        Err(err) => {
            print_error(Style::new(), err);
            return 1;
        }
    };

    match run(&mut config).await {
        Err(err) => {
            print_error(config.color.error, err);
            1
        }
        Ok(ret) => ret,
    }
}

async fn run(config: &mut Config) -> Result<i32> {
    if let Some(ref config_path) = config.config_path {
        let file = read_to_string(config_path)?;
        let name = config_path.display().to_string();
        config.parse(Some(name.as_str()), &file)?;
    };

    let args = std::env::args().skip(1);
    if args.len() == 0 {
        config.parse_args(["-Syu"].iter())?;
    } else {
        config.parse_args(args)?;
    }
    handle_cmd(config).await
}

async fn handle_cmd(config: &mut Config) -> Result<i32> {
    let ret = match config.op.as_str() {
        "database" | "files" | "upgrade" => exec::pacman(config, &config.args)?.code(),
        "query" => handle_query(config).await?,
        "sync" => handle_sync(config).await?,
        "remove" => handle_remove(config)?,
        "deptest" => handle_test(config).await?,
        "getpkgbuild" => handle_get_pkg_build(config).await?,
        "show" => handle_show(config).await?,
        "yay" => handle_yay(config).await?,
        _ => bail!("unknown op '{}'", config.op),
    };

    Ok(ret)
}

async fn handle_query(config: &mut Config) -> Result<i32> {
    let args = &config.args;
    if args.has_arg("u", "upgrades") {
        print_upgrade_list(config).await
    } else {
        Ok(exec::pacman(config, args)?.code())
    }
}

async fn handle_show(config: &Config) -> Result<i32> {
    if config.news > 0 {
        news::news(config).await
    } else if config.complete {
        Ok(completion::print(config, None).await)
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
            println!(" there is nothing to do");
            Ok(0)
        }
    } else if !config.targets.is_empty() {
        search::search_install(config).await
    } else {
        bail!("no operation specified (use -h for help)");
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
    } else if config.args.has_arg("g", "groups") {
        Ok(exec::pacman(config, &config.args)?.code())
    } else {
        let target = std::mem::take(&mut config.targets);
        install::install(config, &target).await
    }
}
