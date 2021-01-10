use crate::config::{Config, NO_CONFIRM};

use std::cell::Cell;
use std::collections::HashMap;
use std::io::{stdin, stdout, BufRead, Write};
use std::ops::Range;

use alpm::PackageReason;
use alpm_utils::{AsTarg, DbListExt, Targ};

#[derive(Debug)]
pub struct NumberMenu<'a> {
    pub in_range: Vec<Range<usize>>,
    pub ex_range: Vec<Range<usize>>,
    pub in_word: Vec<&'a str>,
    pub ex_word: Vec<&'a str>,
}

pub fn split_repo_aur_pkgs<S: AsRef<str> + Clone>(config: &Config, pkgs: &[S]) -> (Vec<S>, Vec<S>) {
    let mut local = Vec::new();
    let mut aur = Vec::new();

    for pkg in pkgs {
        if config.alpm.syncdbs().pkg(pkg.as_ref()).is_ok() {
            local.push(pkg.clone());
        } else {
            aur.push(pkg.clone());
        }
    }

    (local, aur)
}

pub fn split_repo_aur_mode<S: AsRef<str> + Clone>(config: &Config, pkgs: &[S]) -> (Vec<S>, Vec<S>) {
    if config.mode == "aur" {
        (Vec::new(), pkgs.to_vec())
    } else if config.mode == "repo" {
        (pkgs.to_vec(), Vec::new())
    } else {
        split_repo_aur_pkgs(config, pkgs)
    }
}

pub fn split_repo_aur_targets<'a, T: AsTarg>(
    config: &Config,
    targets: &'a [T],
) -> (Vec<Targ<'a>>, Vec<Targ<'a>>) {
    let mut local = Vec::new();
    let mut aur = Vec::new();

    for targ in targets {
        let targ = targ.as_targ();
        if config.mode == "aur" {
            aur.push(targ);
        } else if config.mode == "repo" {
            local.push(targ);
        } else if let Some(repo) = targ.repo {
            if config.aur_namespace() && repo == "aur" {
                aur.push(targ);
            } else if repo == "__aur__" {
                // hack for search install
                aur.push(targ);
            } else {
                local.push(targ);
            }
        } else if config
            .alpm
            .syncdbs()
            .find_target_satisfier(targ.pkg)
            .is_some()
            || config
                .alpm
                .syncdbs()
                .iter()
                .filter(|db| targ.repo.is_none() || db.name() == targ.repo.unwrap())
                .any(|db| db.group(targ.pkg).is_ok())
        {
            local.push(targ);
        } else {
            aur.push(targ);
        }
    }

    (local, aur)
}

pub fn ask(config: &Config, question: &str, default: bool) -> bool {
    let action = config.color.action;
    let bold = config.color.bold;
    let yn = if default { "[Y/n]:" } else { "[y/N]:" };
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

    if input == "y" || input == "yes" {
        true
    } else if input == "n" || input == "no" {
        false
    } else {
        default
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

pub fn get_provider(max: usize) -> usize {
    let mut input = String::new();

    loop {
        print!("\nEnter a number (default=1): ");
        let _ = stdout().lock().flush();
        input.clear();

        if !NO_CONFIRM.get().unwrap() {
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
                eprintln!("invalid number: {}", num);
                continue;
            }
            Ok(num) => num,
        };

        if num < 1 || num > max {
            eprintln!("invalid value: {} is not between 1 and {}", num, max);
            continue;
        }

        return num - 1;
    }
}
