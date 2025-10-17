use crate::config::{Config, LocalRepos};
use crate::repo;

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{stderr, stdin, stdout, BufRead, Write};
use std::mem::take;
use std::ops::Range;
use std::os::fd::{AsFd, OwnedFd};

use alpm::{Package, PackageReason};
use alpm_utils::depends::{satisfies_dep, satisfies_provide};
use alpm_utils::{AsTarg, DbListExt, Targ};
use anyhow::Result;
use nix::unistd::{dup2_stdin, dup2_stdout};
use tr::tr;

#[derive(Debug)]
pub struct NumberMenu<'a> {
    pub in_range: Vec<Range<usize>>,
    pub ex_range: Vec<Range<usize>>,
    pub in_word: Vec<&'a str>,
    pub ex_word: Vec<&'a str>,
}

pub fn pkg_base_or_name(pkg: &Package) -> &str {
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
        if !config.mode.repo() {
            aur.push(targ);
        } else if !config.mode.aur() && !config.mode.pkgbuild() {
            local.push(targ);
        } else if let Some(repo) = targ.repo {
            if config.alpm.syncdbs().iter().any(|db| db.name() == repo) {
                local.push(targ);
            } else if config.pkgbuild_repos.repo(repo).is_some()
                || repo == config.aur_namespace()
                || repo == "."
            {
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
    config: &Config,
    targets: &'a [T],
) -> Result<(Vec<Targ<'a>>, Vec<Targ<'a>>)> {
    let mut local = Vec::new();
    let mut aur = Vec::new();

    let dbs = config.alpm.syncdbs();

    for targ in targets {
        let targ = targ.as_targ();
        if !config.mode.repo() {
            aur.push(targ);
        } else if !config.mode.aur() && !config.mode.pkgbuild() {
            local.push(targ);
        } else if let Some(repo) = targ.repo {
            if config.alpm.syncdbs().iter().any(|db| db.name() == repo) {
                local.push(targ);
            } else {
                aur.push(targ);
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
    match stdin.read_line(&mut input) {
        Ok(0) => {
            println!();
            false
        }
        Ok(_) => {
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
        Err(_) => {
            println!(" -> Error reading input");
            false
        }
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

pub fn unneeded_pkgs(config: &Config, keep_optional: bool) -> Vec<&str> {
    let db = config.alpm.localdb();
    let mut next = db
        .pkgs()
        .into_iter()
        .filter(|p| p.reason() == PackageReason::Explicit)
        .collect::<Vec<_>>();
    let mut deps = db
        .pkgs()
        .into_iter()
        .filter(|p| p.reason() != PackageReason::Explicit)
        .map(|p| (p.name(), p))
        .collect::<BTreeMap<_, _>>();

    let mut provides: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for dep in deps.values() {
        for prov in dep.provides() {
            provides.entry(prov.name()).or_default().push((*dep, prov));
        }
    }

    while !next.is_empty() {
        for new in take(&mut next) {
            let opt = keep_optional.then(|| new.optdepends());
            let depends = new.depends().into_iter().chain(opt.into_iter().flatten());

            for dep in depends {
                if let Entry::Occupied(entry) = deps.entry(dep.name()) {
                    let pkg = entry.get();
                    if satisfies_dep(dep, pkg.name(), pkg.version()) {
                        next.push(entry.remove());
                    }
                }
                if let Entry::Occupied(mut entry) = provides.entry(dep.name()) {
                    let provides = entry
                        .get_mut()
                        .extract_if(.., |(_, prov)| satisfies_provide(dep, prov))
                        .filter_map(|(pkg, _)| deps.remove(pkg.name()));
                    next.extend(provides);
                };
            }
        }
    }

    deps.into_keys().collect::<Vec<_>>()
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

pub fn repo_aur_pkgs(config: &Config) -> (Vec<&alpm::Package>, Vec<&alpm::Package>) {
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

pub fn redirect_to_stderr() -> Result<OwnedFd> {
    let stdout = stdout().as_fd().try_clone_to_owned()?;
    dup2_stdout(&stderr())?;
    Ok(stdout)
}

pub fn reopen_stdin() -> Result<()> {
    let file = File::open("/dev/tty")?;
    dup2_stdin(&file)?;
    Ok(())
}

pub fn reopen_stdout<Fd: AsFd>(file: Fd) -> Result<()> {
    dup2_stdout(file)?;
    Ok(())
}

pub fn is_arch_repo(name: &str) -> bool {
    matches!(
        name,
        "testing"
            | "community-testing"
            | "core"
            | "extra"
            | "community"
            | "multilib"
            | "core-testing"
            | "extra-testing"
            | "multilib-testing"
    )
}
