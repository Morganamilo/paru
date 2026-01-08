use crate::config::{Config, LocalRepos, Sign};
use crate::exec::{self, command_status};
use crate::fmt::print_indent;
use crate::printtr;
use crate::util::ask;

use std::collections::HashMap;
use std::env::current_exe;
use std::ffi::OsStr;
use std::fs::{read_dir, read_link};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use alpm::{AlpmListMut, Db};
use ansiterm::Style;
use anyhow::{Context, Error, Result};
use nix::unistd::{Gid, Uid, User};
use tr::tr;
use unicode_width::UnicodeWidthStr;

pub fn add<P: AsRef<Path>, S: AsRef<OsStr>>(
    config: &Config,
    path: P,
    name: &str,
    pkgs: &[S],
) -> Result<()> {
    let db = path.as_ref().join(format!("{}.db", name));
    let name = if db.exists() {
        if pkgs.is_empty() {
            return Ok(());
        }
        read_link(&db).context("readlink")?
    } else if !name.contains(".db.") {
        PathBuf::from(format!("{}.db.tar.gz", name))
    } else {
        PathBuf::from(name)
    };

    let path = path.as_ref();
    let file = path.join(name);

    if !db.exists() {
        let mut cmd = Command::new("install");
        cmd.arg("-dm755").arg(path);
        if exec::command_output(&mut cmd).is_err() {
            let mut cmd = Command::new(&config.sudo_bin);
            cmd.arg("install")
                .arg("-dm755")
                .arg("-o")
                .arg(Uid::current().as_raw().to_string())
                .arg("-g")
                .arg(Gid::current().as_raw().to_string())
                .arg(path);
            exec::command(&mut cmd)?;
        }
    }

    let pkgs = pkgs
        .iter()
        .map(|p| path.join(Path::new(p.as_ref()).file_name().unwrap()))
        .collect::<Vec<_>>();

    let mut cmd = Command::new("repo-add");

    if !config.keep_repo_cache {
        cmd.arg("-R");
    }

    cmd.arg(file).args(pkgs);

    if config.sign_db != Sign::No {
        cmd.arg("-s");
        if let Sign::Key(ref k) = config.sign_db {
            cmd.arg("-k");
            cmd.arg(k);
        }
    }

    let err = exec::command(&mut cmd);

    let user = User::from_uid(Uid::current()).unwrap().unwrap();

    if err.is_err() {
        eprintln!(
            "Could not add packages to repo:
    paru now expects local repos to be writable as your user:
    You should chown/chmod your repos to be writable by you:
    chown -R {}: {}",
            user.name,
            path.display()
        );
    }

    err
}

pub fn remove<P: AsRef<Path>, S: AsRef<OsStr>>(
    config: &Config,
    path: P,
    name: &str,
    pkgs: &[S],
) -> Result<()> {
    let path = path.as_ref();
    let db = path.join(format!("{}.db", name));
    if pkgs.is_empty() || !db.exists() {
        return Ok(());
    }

    let name = read_link(db)?;
    let file = path.join(name);

    let mut cmd = Command::new("repo-remove");
    cmd.arg(file);

    if config.sign_db != Sign::No {
        cmd.arg("-s");
        if let Sign::Key(ref k) = config.sign_db {
            cmd.arg("-k");
            cmd.arg(k);
        }
    }

    cmd.args(pkgs);
    exec::command(&mut cmd)?;

    Ok(())
}

pub fn init<P: AsRef<Path>>(config: &Config, path: P, name: &str) -> Result<()> {
    let pkgs: &[&str] = &[];
    add(config, path, name, pkgs)
}

fn is_configured_local_db(config: &Config, db: &Db) -> bool {
    match config.repos {
        LocalRepos::None => false,
        LocalRepos::Default => is_local_db(db),
        LocalRepos::Repo(ref r) => is_local_db(db) && r.iter().any(|r| *r == db.name()),
    }
}

pub fn file(repo: &Db) -> Option<&str> {
    repo.servers()
        .first()
        .map(|s| s.trim_start_matches("file://"))
}

fn is_local_db(db: &alpm::Db) -> bool {
    !db.servers().is_empty() && db.servers().iter().all(|s| s.starts_with("file://"))
}

pub fn repo_aur_dbs(config: &Config) -> (AlpmListMut<&Db>, AlpmListMut<&Db>) {
    let dbs = config.alpm.syncdbs();
    let mut aur = dbs.to_list_mut();
    let mut repo = dbs.to_list_mut();
    aur.retain(|db| is_configured_local_db(config, db));
    repo.retain(|db| !is_configured_local_db(config, db));
    (repo, aur)
}

pub fn delete(config: &mut Config) -> Result<(), Error> {
    let (_, mut repos) = repo_aur_dbs(config);
    repos.retain(|r| {
        config.delete >= 1
            || config.uninstall
            || config.targets.is_empty()
            || config.targets.contains(&r.name().to_string())
    });

    if config.delete >= 1 {
        let mut rm = HashMap::<&str, Vec<&str>>::new();
        let mut rmfiles = Vec::new();
        for repo in &repos {
            for pkg in repo.pkgs() {
                if config.targets.iter().any(|p| p == pkg.name()) {
                    rm.entry(repo.name()).or_default().push(pkg.name());
                }
            }
        }

        let cb = config.alpm.take_raw_log_cb();
        for repo in &repos {
            if let Some(pkgs) = rm.get(&repo.name()) {
                let path = repo
                    .servers()
                    .first()
                    .unwrap()
                    .trim_start_matches("file://");
                remove(config, path, repo.name(), pkgs)?;

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
        refresh(config, &repos)?;

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

        return Ok(());
    }

    Ok(())
}

pub fn refresh<S: AsRef<OsStr>>(config: &mut Config, repos: &[S]) -> Result<i32> {
    let exe = current_exe().context(tr!("failed to get current exe"))?;
    let c = config.color;

    let mut dbs = config.alpm.syncdbs().to_list_mut();

    dbs.retain(|db| is_local_db(db));

    if !repos.is_empty() {
        dbs.retain(|db| repos.iter().any(|r| r.as_ref() == db.name()));
    }

    for db in dbs {
        let path = file(db);
        if let Some(path) = path {
            init(config, path, db.name())?;
        }
    }

    if !nix::unistd::getuid().is_root() && !cfg!(feature = "mock") {
        let mut cmd = Command::new(&config.sudo_bin);

        cmd.arg(exe);

        if let Some(ref conf) = config.pacman_conf {
            cmd.arg("--config").arg(conf);
        }

        cmd.arg("--dbpath")
            .arg(config.alpm.dbpath())
            .arg("-Ly")
            .args(repos);

        return Ok(command_status(&mut cmd)?.code());
    }

    let mut dbs = config.alpm.syncdbs_mut().to_list_mut();
    dbs.retain(|db| is_local_db(db));

    if !repos.is_empty() {
        dbs.retain(|db| repos.iter().any(|r| r.as_ref() == db.name()));
    }

    println!(
        "{} {}",
        c.action.paint("::"),
        c.bold.paint(tr!("syncing local databases..."))
    );

    if !dbs.is_empty() {
        dbs.list().update(cfg!(feature = "mock"))?;
    } else {
        printtr!("  nothing to do");
    }

    Ok(0)
}

pub fn clean(config: &mut Config) -> Result<i32> {
    let c = config.color;
    let (_, repos) = repo_aur_dbs(config);
    let repo_names = repos
        .iter()
        .map(|r| r.name().to_string())
        .collect::<Vec<_>>();
    drop(repos);
    refresh(config, &repo_names)?;
    let (_, repos) = repo_aur_dbs(config);
    let db = config.alpm.localdb();

    let mut rem = repos
        .iter()
        .map(|repo| {
            repo.pkgs()
                .iter()
                .filter(|pkg| db.pkg(pkg.name()).is_err())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    rem.retain(|r| !r.is_empty());
    drop(repos);

    if rem.is_empty() {
        printtr!("there is nothing to do");
        return Ok(0);
    }

    println!();
    let count = rem.iter().fold(0, |acc, r| acc + r.len());
    let fmt = format!("{} ({}) ", tr!("Packages"), count);
    let start = fmt.width();
    print!("{}", c.bold.paint(fmt));
    print_indent(
        Style::new(),
        start,
        4,
        config.cols,
        "  ",
        rem.iter().flatten().map(|p| p.name()),
    );

    println!();
    if !ask(config, &tr!("Proceed with removal?"), true) {
        return Ok(1);
    }

    for pkgs in &rem {
        let repo = pkgs[0].db().unwrap();
        let path = file(repo).unwrap();
        let pkgs = pkgs.iter().map(|p| p.name()).collect::<Vec<_>>();
        remove(config, path, repo.name(), &pkgs)?;
    }

    let mut rmfiles = Vec::new();

    for pkg in rem.iter().flatten() {
        let repo = pkg.db().unwrap();
        let path = file(repo).unwrap();
        let pkgfile = Path::new(path).join(pkg.filename().unwrap());
        rmfiles.push(pkgfile);
    }

    if !rmfiles.is_empty() {
        let mut cmd = Command::new(&config.sudo_bin);
        cmd.arg("rm").args(rmfiles);
        exec::command(&mut cmd)?;
    }

    let (_, repos) = repo_aur_dbs(config);
    let repo_names = repos
        .iter()
        .map(|r| r.name().to_string())
        .collect::<Vec<_>>();
    drop(repos);
    refresh(config, &repo_names)?;

    Ok(0)
}

pub fn print(
    repos: AlpmListMut<&alpm::Db>,
    config: &Config,
    repoc: Style,
    pkgc: Style,
    version: Style,
    installedc: Style,
) {
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
}
