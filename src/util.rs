use crate::config::{Config, LocalRepos};
use crate::fmt::color_repo;
use crate::repo;

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fs::File;
use std::io::{BufRead, Write, stderr, stdin, stdout};
use std::mem::take;
use std::ops::Range;
use std::os::fd::{AsFd, OwnedFd};
use std::time::{SystemTime, UNIX_EPOCH};

use alpm::{Package, PackageReason};
use alpm_utils::depends::{satisfies_dep, satisfies_provide};
use alpm_utils::{AsTarg, DbListExt, Targ};
use anyhow::Result;
use nix::unistd::{dup2_stdin, dup2_stdout};
use tr::tr;

/// Current time as seconds since the unix epoch.
pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

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
            || dbs
                .iter()
                .filter(|db| targ.repo.is_none() || db.name() == targ.repo.unwrap())
                .any(|db| db.group(targ.pkg).is_ok())
        {
            local.push(targ);
        } else if let Some(provider) = dbs.find_target_satisfier(targ.pkg) {
            let Some(db) = provider.db() else {
                aur.push(targ);
                continue;
            };
            let repo_name = db.name();
            let pkg_name = provider.name();

            if !config.mode.aur() || alpm::Depend::new(targ.pkg).name() == pkg_name {
                local.push(targ);
            } else {
                let c = &config.color;
                println!(
                    "{} {}",
                    c.action.paint("::"),
                    tr!(
                        "{repo}/{pkg} provides {target}",
                        target = targ.pkg,
                        repo = color_repo(c.enabled, repo_name),
                        pkg = c.bold.paint(pkg_name)
                    )
                );
                if ask(
                    config,
                    &tr!("Install {repo}/{pkg}?", repo = repo_name, pkg = pkg_name),
                    true,
                ) {
                    local.push(targ);
                } else {
                    aur.push(Targ {
                        repo: Some(config.aur_namespace()),
                        ..targ
                    });
                }
            }
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

pub(crate) fn read_line_or_exit(input: &mut String) {
    let stdin = stdin();
    match stdin.lock().read_line(input) {
        Ok(0) | Err(_) => {
            println!();
            std::process::exit(1);
        }
        Ok(_) => {}
    }
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
    let mut input = String::new();
    read_line_or_exit(&mut input);
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
    let mut input = String::new();
    read_line_or_exit(&mut input);
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
            read_line_or_exit(&mut input);
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
    dup2_stdout(stderr())?;
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

#[cfg(all(test, feature = "mock"))]
mod provider_tests {
    use super::split_repo_aur_targets;
    use crate::config::{Config, Mode};
    use std::io::Write;
    use std::path::Path;
    use std::process::{Command, Stdio};

    #[test]
    fn provider_selection() {
        if let Ok(case) = std::env::var("PARU_PROVIDER_TEST_CASE") {
            check_provider(&case);
            return;
        }

        for (case, answer) in [
            ("decline", "n\n"),
            ("accept", "y\n"),
            ("no-confirm", ""),
            ("exact", ""),
            ("exact-version", ""),
            ("repo-qualified", ""),
            ("aur-qualified", ""),
            ("repo-only", ""),
            ("aur-only", ""),
            ("custom-namespace", "n\n"),
        ] {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "util::provider_tests::provider_selection",
                    "--nocapture",
                ])
                .env("PARU_PROVIDER_TEST_CASE", case)
                .env("LC_ALL", "C")
                .env("LANGUAGE", "C")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(answer.as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{case}:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
    }

    fn check_provider(case: &str) {
        let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
        let mut config = Config::default();
        config.pacman = pacmanconf::Config::with_opts(
            None,
            Some(testdata.join("pacman.conf").to_str().unwrap()),
            None,
        )
        .unwrap();
        config.pacman.root_dir = "/".to_string();
        config.pacman.db_path = testdata.join("db").to_str().unwrap().to_string();
        config.mode = match case {
            "repo-only" => Mode::REPO,
            "aur-only" => Mode::AUR,
            _ => Mode::AUR | Mode::REPO,
        };
        config.no_confirm = case == "no-confirm";
        if case == "custom-namespace" {
            let mut repo = pacmanconf::Repository::default();
            repo.name = "aur".to_string();
            config.pacman.repos.push(repo);
        }
        config.init_alpm().unwrap();
        config.raur = Some(crate::mock::Mock::default());

        let target = match case {
            "exact" => "bash",
            "exact-version" => "bash>=5",
            "repo-qualified" => "core/sh",
            "aur-qualified" => "aur/sh",
            _ => "sh",
        };
        let input_targets = [target];
        let (repo, aur) = split_repo_aur_targets(&mut config, &input_targets).unwrap();
        let from_aur = matches!(
            case,
            "decline" | "aur-qualified" | "aur-only" | "custom-namespace"
        );
        if from_aur {
            assert!(repo.is_empty());
            assert_eq!(aur.len(), 1);
            if case != "aur-only" {
                assert_eq!(aur[0].repo, Some(config.aur_namespace()));
            }
        } else {
            assert!(aur.is_empty());
            assert_eq!(repo.len(), 1);
        }

        config.init_alpm().unwrap();
        let flags = crate::resolver::flags(&mut config) | aur_depends::Flags::NO_DEPS;
        let mut cache = raur::Cache::new();
        cache.insert(
            raur::Package {
                name: "sh".to_string(),
                package_base: "sh".to_string(),
                version: "1-1".to_string(),
                ..Default::default()
            }
            .into(),
        );
        let resolver = crate::resolver::resolver(
            &config,
            &config.alpm,
            config.raur.as_ref().unwrap(),
            &mut cache,
            Vec::new(),
            flags,
        );
        let mut targets = repo;
        targets.extend(aur);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let actions = runtime
            .block_on(resolver.resolve_targets(&targets))
            .unwrap();
        assert!(actions.missing.is_empty());
        if from_aur {
            assert!(actions.install.is_empty());
            assert_eq!(
                actions
                    .iter_aur_pkgs()
                    .map(|pkg| pkg.pkg.name.as_str())
                    .collect::<Vec<_>>(),
                ["sh"],
            );
        } else {
            assert!(actions.build.is_empty());
            assert_eq!(
                actions
                    .install
                    .iter()
                    .map(|pkg| pkg.pkg.name())
                    .collect::<Vec<_>>(),
                ["bash"],
            );
        }
    }
}
