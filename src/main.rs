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
mod search;
mod sync;
mod upgrade;
mod util;

#[macro_use]
extern crate smart_default;

use crate::config::Config;
use crate::devel::{load_devel_info, save_devel_info};
use crate::query::print_upgrade_list;

use std::error::Error as StdError;
use std::fs::read_to_string;

use ansi_term::Style;
use anyhow::{bail, Error, Result};
use cini::Ini;

// Reimplementation of std's print function that ignore errors.
// Stops crashing when piping paru
#[macro_export]
macro_rules! sprintln {
    () => {{
        use std::io::Write;
        let _ = ::std::writeln!(::std::io::stdout());
    }};
    ($($arg:tt)*) => {{
        use std::io::Write;
        let _ = ::std::writeln!(::std::io::stdout(), $( $arg)* );
    }};
}
#[macro_export]
macro_rules! esprintln {
    () => {{
        use std::io::Write;
        let _ = ::std::writeln!(::std::io::stderr());
    }};
    ($($arg:tt)*) => {{
        use std::io::Write;
        let _ = ::std::writeln!(::std::io::stderr(), $( $arg)* );
    }};
}
#[macro_export]
macro_rules! sprint {
    ($($arg:tt)*) => {{
        use std::io::Write;
        let _ = ::std::write!(::std::io::stdout(), $( $arg)* );
    }};
}
#[macro_export]
macro_rules! esprint {
    ($($arg:tt)*) => {{
        use std::io::Write;
        let _ = ::std::write!(::std::io::stderr(), $( $arg)* );
    }};
}

fn print_error(color: Style, err: Error) {
    #[cfg(feature = "backtrace")]
    {
        let backtrace = err.backtrace();

        if backtrace.status() == std::backtrace::BacktraceStatus::Captured {
            esprint!("{}", backtrace);
        }
    }
    let mut iter = err.chain().peekable();

    if StdError::is::<exec::PacmanError>(*iter.peek().unwrap())
        || StdError::is::<exec::Status>(*iter.peek().unwrap())
    {
        esprint!("{}", iter.peek().unwrap());
        return;
    }

    esprint!("{} ", color.paint("error:"));
    while let Some(link) = iter.next() {
        esprint!("{}", link);
        if iter.peek().is_some() {
            esprint!(": ");
        }
    }
    esprintln!();
}

fn main() {
    let i = main2();
    std::process::exit(i);
}

fn main2() -> i32 {
    //env_logger::init();
    let mut config = match Config::new() {
        Ok(config) => config,
        Err(err) => {
            print_error(Style::new(), err);
            return 1;
        }
    };

    match run(&mut config) {
        Err(err) => {
            print_error(config.color.error, err);
            1
        }
        Ok(ret) => ret,
    }
}

fn run(config: &mut Config) -> Result<i32> {
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
    handle_cmd(config)
}

fn handle_cmd(config: &mut Config) -> Result<i32> {
    let ret = match config.op.as_str() {
        "database" | "files" | "deptest" | "upgrade" => exec::pacman(config, &config.args)?.code(),
        "query" => handle_query(config)?,
        "sync" => handle_sync(config)?,
        "remove" => handle_remove(config)?,
        "getpkgbuild" => handle_get_pkg_build(config)?,
        "show" => handle_show(config)?,
        "yay" => handle_yay(config)?,
        _ => bail!("unknown op '{}'", config.op),
    };

    Ok(ret)
}

fn handle_query(config: &mut Config) -> Result<i32> {
    let args = &config.args;
    if args.has_arg("u", "upgrades") {
        print_upgrade_list(config)
    } else {
        Ok(exec::pacman(config, args)?.code())
    }
}

fn handle_show(config: &Config) -> Result<i32> {
    if config.news > 0 {
        news::news(config)?;
        Ok(0)
    } else if config.complete {
        Ok(completion::print(config, None))
    } else {
        Ok(0)
    }
}

fn handle_get_pkg_build(config: &mut Config) -> Result<i32> {
    if config.print {
        download::show_pkgbuilds(config)
    } else {
        download::getpkgbuilds(config)
    }
}

fn handle_yay(config: &mut Config) -> Result<i32> {
    if config.gendb {
        devel::gendb(config)?;
        Ok(0)
    } else if config.clean > 0 {
        config.need_root = true;
        let unneeded = util::unneeded_pkgs(config, config.clean == 1);
        let mut args = config.pacman_args();
        args.remove("c").remove("clean");
        args.targets = unneeded;
        args.op = "remove";
        Ok(exec::pacman(config, &args)?.code())
    } else if !config.targets.is_empty() {
        search::search_install(config)
    } else {
        bail!("no operation specified (use -h for help)");
    }
}

fn handle_remove(config: &mut Config) -> Result<i32> {
    let mut devel_info = load_devel_info(config)?.unwrap_or_default();

    let ret = exec::pacman(config, &config.args)?.code();

    if ret == 0 {
        for target in &config.targets {
            devel_info.info.remove(target);
        }

        if let Err(err) = save_devel_info(config, &devel_info) {
            print_error(config.color.error, err);
        }
    }

    Ok(0)
}

fn handle_sync(config: &mut Config) -> Result<i32> {
    if config.args.has_arg("i", "info") {
        info::info(config, config.args.count("i", "info") > 1)
    } else if config.args.has_arg("c", "clean") {
        clean::clean(config)?;
        Ok(0)
    } else if config.args.has_arg("l", "list") {
        sync::list(config)
    } else if config.args.has_arg("s", "search") {
        search::search(config)
    } else if config.args.has_arg("g", "groups") {
        Ok(exec::pacman(config, &config.args)?.code())
    } else {
        let target = std::mem::take(&mut config.targets);
        install::install(config, &target)
    }
}
