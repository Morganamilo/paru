use crate::config::{Config, LocalRepos, Mode};
use crate::repo;

use std::cell::Cell;
use std::collections::HashMap;
use std::io::{stdin, stdout, BufRead, Write};
use std::ops::Range;

use alpm::{Package, PackageReason};
use alpm_utils::{AsTarg, DbListExt, Targ};
use anyhow::Result;
use tr::tr;

#[derive(Debug)]
pub struct NumberMenu<'a> {
    pub in_range: Vec<Range<usize>>,
    pub ex_range: Vec<Range<usize>>,
    pub in_word: Vec<&'a str>,
    pub ex_word: Vec<&'a str>,
}

pub fn pkg_base_or_name<'a>(pkg: &Package<'a>) -> &'a str {
    pkg.base().unwrap_or_else(|| pkg.name())
}

pub fn split_repo_aur_targets<'a, T: AsTarg>(
    config: &mut Config,
    targets: &'a [T],
) -> Result<(Vec<Targ<'a>>, Vec<Targ<'a>>)> {
    let mut local = Vec::new();
    let mut aur = Vec::new();

    let cb = config.alpm.take_raw_question_cb();
    let empty: [&str; 0] = [];
    config.alpm.set_ignorepkgs(empty.iter())?;
    config.alpm.set_ignoregroups(empty.iter())?;

    let dbs = config.alpm.syncdbs();

    for targ in targets {
        let targ = targ.as_targ();
        if config.mode == Mode::Aur {
            aur.push(targ);
        } else if config.mode == Mode::Repo {
            local.push(targ);
        } else if let Some(repo) = targ.repo {
            if repo == config.aur_namespace() {
                aur.push(targ);
            } else {
                local.push(targ);
            }
        } else if dbs.pkg(targ.pkg).is_ok()
            || dbs.find_target_satisfier(targ.pkg).is_some()
            || dbs
                .iter()
                .filter(|db| targ.repo.is_none() || db.name() == targ.repo.unwrap())
                .any(|db| db.group(targ.pkg).is_ok())
        {
            local.push(targ);
        } else {
            aur.push(targ);
        }
    }

    config.alpm.set_raw_question_cb(cb);
    config
        .alpm
        .set_ignorepkgs(config.pacman.ignore_pkg.iter())?;
    config
        .alpm
        .set_ignorepkgs(config.pacman.ignore_pkg.iter())?;

    Ok((local, aur))
}

pub fn split_repo_aur_info<'a, T: AsTarg>(
    config: &mut Config,
    targets: &'a [T],
) -> Result<(Vec<Targ<'a>>, Vec<Targ<'a>>)> {
    let mut local = Vec::new();
    let mut aur = Vec::new();

    let dbs = config.alpm.syncdbs();

    for targ in targets {
        let targ = targ.as_targ();
        if config.mode == Mode::Aur {
            aur.push(targ);
        } else if config.mode == Mode::Repo {
            local.push(targ);
        } else if let Some(repo) = targ.repo {
            if repo == config.aur_namespace() {
                aur.push(targ);
            } else {
                local.push(targ);
            }
        } else if dbs.pkg(targ.pkg).is_ok() {
            local.push(targ);
        } else {
            aur.push(targ);
        }
    }

    Ok((local, aur))
}

pub fn ask(config: &Config, question: &str, default: bool) -> bool {
    let action = config.color.action;
    let bold = config.color.bold;
    let yn = if default {
        tr!("[Y/n]:")
    } else {
        tr!("[y/N]:")
    };
    print!(
        "{} {} {} ",
        action.paint("::"),
        bold.paint(question),
        bold.paint(yn)
    );
    let _ = stdout().lock().flush();
    if config.no_confirm {
        println!();
        return default;
    }
    let stdin = stdin();
    let mut input = String::new();
    let _ = stdin.read_line(&mut input);
    let input = input.to_lowercase();
    let input = input.trim();

    if input == tr!("y") || input == tr!("yes") {
        true
    } else if input.trim().is_empty() {
        default
    } else {
        false
    }
}

pub fn input(config: &Config, question: &str) -> String {
    let action = config.color.action;
    let bold = config.color.bold;
    println!("{} {}", action.paint("::"), bold.paint(question));
    print!("{} ", action.paint("::"));
    let _ = stdout().lock().flush();
    if config.no_confirm {
        println!();
        return "".into();
    }
    let stdin = stdin();
    let mut input = String::new();
    let _ = stdin.read_line(&mut input);
    input
}

#[derive(Hash, PartialEq, Eq, SmartDefault, Copy, Clone)]
enum State {
    #[default]
    Remove,
    CheckDeps,
    Keep,
}

pub fn unneeded_pkgs(config: &Config, optional: bool) -> Vec<&str> {
    let mut states = HashMap::new();
    let mut remove = Vec::new();
    let mut providers = HashMap::<_, Vec<_>>::new();
    let db = config.alpm.localdb();

    for pkg in db.pkgs() {
        providers
            .entry(pkg.name().to_string())
            .or_default()
            .push(pkg.name());
        for dep in pkg.provides() {
            providers
                .entry(dep.name().to_string())
                .or_default()
                .push(pkg.name())
        }

        if pkg.reason() == PackageReason::Explicit {
            states.insert(pkg.name(), Cell::new(State::CheckDeps));
        } else {
            states.insert(pkg.name(), Cell::new(State::Remove));
        }
    }

    let mut again = true;

    while again {
        again = false;

        let mut check_deps = |deps: alpm::AlpmList<alpm::Dep>| {
            for dep in deps {
                if let Some(deps) = providers.get(dep.name()) {
                    for dep in deps {
                        let state = states.get(dep).unwrap();

                        if state.get() != State::Keep {
                            state.set(State::CheckDeps);
                            again = true;
                        }
                    }
                }
            }
        };

        for (&pkg, state) in &states {
            if state.get() != State::CheckDeps {
                continue;
            }

            if let Ok(pkg) = db.pkg(pkg) {
                state.set(State::Keep);
                check_deps(pkg.depends());

                if optional {
                    check_deps(pkg.optdepends());
                }

                if config.clean > 1 {
                    continue;
                }

                if config.alpm.syncdbs().pkg(pkg.name()).is_err() {
                    check_deps(pkg.makedepends());
                    check_deps(pkg.checkdepends());
                }
            }
        }
    }

    for pkg in db.pkgs() {
        if states.get(pkg.name()).unwrap().get() == State::Remove {
            remove.push(pkg.name());
        }
    }

    remove
}

impl<'a> NumberMenu<'a> {
    pub fn new(input: &'a str) -> Self {
        let mut include_range = Vec::new();
        let mut exclude_range = Vec::new();
        let mut include_repo = Vec::new();
        let mut exclude_repo = Vec::new();

        let words = input
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|s| !s.is_empty());

        for mut word in words {
            let mut invert = false;
            if word.starts_with('^') {
                word = word.trim_start_matches('^');
                invert = true;
            }

            let mut split = word.split('-');
            let start_str = split.next().unwrap();

            let start = match start_str.parse::<usize>() {
                Ok(start) => start,
                Err(_) => {
                    if invert {
                        exclude_repo.push(start_str);
                    } else {
                        include_repo.push(start_str);
                    }
                    continue;
                }
            };

            let end = match split.next() {
                Some(end) => end,
                None => {
                    if invert {
                        exclude_range.push(start..start + 1);
                    } else {
                        include_range.push(start..start + 1);
                    }
                    continue;
                }
            };

            match end.parse::<usize>() {
                Ok(end) => {
                    if invert {
                        exclude_range.push(start..end + 1)
                    } else {
                        include_range.push(start..end + 1)
                    }
                }
                _ => {
                    if invert {
                        exclude_repo.push(start_str)
                    } else {
                        include_repo.push(start_str)
                    }
                }
            }
        }

        NumberMenu {
            in_range: include_range,
            ex_range: exclude_range,
            in_word: include_repo,
            ex_word: exclude_repo,
        }
    }

    pub fn contains(&self, n: usize, word: &str) -> bool {
        if self.in_range.iter().any(|r| r.contains(&n)) || self.in_word.contains(&word) {
            true
        } else if self.ex_range.iter().any(|r| r.contains(&n)) || self.ex_word.contains(&word) {
            false
        } else {
            self.in_range.is_empty() && self.in_word.is_empty()
        }
    }
}

pub fn get_provider(max: usize, no_confirm: bool) -> usize {
    let mut input = String::new();

    loop {
        print!("\n{}", tr!("Enter a number (default=1): "));
        let _ = stdout().lock().flush();
        input.clear();

        if !no_confirm {
            let stdin = stdin();
            let mut stdin = stdin.lock();
            let _ = stdin.read_line(&mut input);
        }

        let num = input.trim();
        if num.is_empty() {
            return 0;
        }

        let num = match num.parse::<usize>() {
            Err(_) => {
                eprintln!("{}", tr!("invalid number: {}", num));
                continue;
            }
            Ok(num) => num,
        };

        if num < 1 || num > max {
            eprintln!(
                "{}",
                tr!(
                    "invalid value: {n} is not between 1 and {max}",
                    n = num,
                    max = max
                )
            );
            continue;
        }

        return num - 1;
    }
}

pub fn split_repo_aur_pkgs<S: AsRef<str> + Clone>(config: &Config, pkgs: &[S]) -> (Vec<S>, Vec<S>) {
    let mut aur = Vec::new();
    let mut repo = Vec::new();
    let (repo_dbs, aur_dbs) = repo::repo_aur_dbs(config);

    for pkg in pkgs {
        if repo_dbs.pkg(pkg.as_ref()).is_ok() {
            repo.push(pkg.clone());
        } else if config.repos == LocalRepos::None || aur_dbs.pkg(pkg.as_ref()).is_ok() {
            aur.push(pkg.clone());
        }
    }

    (repo, aur)
}

pub fn repo_aur_pkgs(config: &Config) -> (Vec<alpm::Package<'_>>, Vec<alpm::Package<'_>>) {
    if config.repos != LocalRepos::None {
        let (repo, aur) = repo::repo_aur_dbs(config);
        let repo = repo.iter().flat_map(|db| db.pkgs()).collect::<Vec<_>>();
        let aur = aur.iter().flat_map(|db| db.pkgs()).collect::<Vec<_>>();
        (repo, aur)
    } else {
        let (repo, aur) = config
            .alpm
            .localdb()
            .pkgs()
            .iter()
            .partition(|pkg| config.alpm.syncdbs().pkg(pkg.name()).is_ok());
        (repo, aur)
    }
}
