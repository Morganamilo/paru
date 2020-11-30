use crate::config::{Config, LocalRepos};
use crate::exec;

use std::env::current_exe;
use std::ffi::OsStr;
use std::fs::read_link;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

pub fn add<P: AsRef<Path>, S: AsRef<OsStr>>(
    config: &Config,
    path: P,
    name: &str,
    mv: bool,
    pkgs: &[S],
) -> Result<()> {
    let db = path.as_ref().join(format!("{}.db", name));
    let name = if db.exists() {
        if pkgs.is_empty() {
            return Ok(());
        }
        read_link(db)?
    } else if !name.contains(".db.") {
        PathBuf::from(format!("{}.db.tar.gz", name))
    } else {
        PathBuf::from(name)
    };

    let path = path.as_ref();
    let file = path.join(&name);
    let sudo = OsStr::new(&config.sudo_bin);

    exec::command(
        sudo,
        ".",
        &[OsStr::new("mkdir"), OsStr::new("-p"), path.as_os_str()],
    )?
    .success()?;

    if !pkgs.is_empty() {
        let cmd = if mv {
            OsStr::new("mv")
        } else {
            OsStr::new("cp")
        };

        let mut args = vec![cmd, OsStr::new("-f")];
        args.extend(pkgs.iter().map(OsStr::new));
        args.push(path.as_os_str());
        exec::command(sudo, ".", &args)?.success()?;
    }

    let mut args = vec![OsStr::new("repo-add"), OsStr::new("-R"), file.as_os_str()];
    let pkgs = pkgs
        .iter()
        .map(|p| path.join(Path::new(p.as_ref()).file_name().unwrap()))
        .collect::<Vec<_>>();

    args.extend(pkgs.iter().map(|p| p.as_os_str()));
    exec::command(sudo, ".", &args)?.success()?;

    Ok(())
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
    let file = path.join(&name);

    let mut args = vec![OsStr::new("repo-remove"), file.as_os_str()];
    args.extend(pkgs.iter().map(|p| p.as_ref()));
    exec::command(&config.sudo_bin, ".", &args)?.success()?;

    Ok(())
}

pub fn init<P: AsRef<Path>>(config: &Config, path: P, name: &str) -> Result<()> {
    let pkgs: &[&str] = &[];
    add(config, path, name, false, pkgs)
}

pub fn configured_local_repos(config: &Config) -> Vec<&str> {
    config
        .pacman
        .repos
        .iter()
        .filter(|r| is_configured_local_repo(config, r))
        .map(|r| r.name.as_str())
        .collect()
}

pub fn is_configured_local_repo(config: &Config, repo: &pacmanconf::Repository) -> bool {
    match config.repos {
        LocalRepos::None => false,
        LocalRepos::Default => is_local(repo),
        LocalRepos::Repo(ref r) => r.iter().any(|r| *r == repo.name),
    }
}

pub fn file(repo: &pacmanconf::Repository) -> Option<&str> {
    repo.servers
        .first()
        .map(|s| s.trim_start_matches("file://"))
}

pub fn all_files(config: &Config) -> Vec<&str> {
    config
        .pacman
        .repos
        .iter()
        .filter(|r| is_configured_local_repo(config, r))
        .flat_map(|r| files(r))
        .collect()
}

pub fn files(repo: &pacmanconf::Repository) -> Vec<&str> {
    repo.servers
        .iter()
        .map(|s| s.trim_start_matches("file://"))
        .collect()
}

pub fn is_local(repo: &pacmanconf::Repository) -> bool {
    !repo.servers.is_empty() && repo.servers.iter().all(|s| s.starts_with("file://"))
}

pub fn is_local_db(db: &alpm::Db) -> bool {
    !db.servers().is_empty() && db.servers().iter().all(|s| s.starts_with("file://"))
}

pub fn refresh<S: AsRef<OsStr>>(config: &mut Config, repos: &[S]) -> Result<i32> {
    let exe = current_exe().context("failed to get current exe")?;
    let c = config.color;
    if !nix::unistd::getuid().is_root() {
        let mut cmd = Command::new(&config.sudo_bin);

        cmd.arg(exe);

        if let Some(ref conf) = config.pacman_conf {
            cmd.arg("--config").arg(conf);
        }

        cmd.arg("--dbpath")
            .arg(config.alpm.dbpath())
            .arg("-Sy")
            .arg("--local")
            .args(repos);

        let status = cmd.spawn()?.wait()?;

        return Ok(status.code().unwrap_or(1));
    }

    let mut dbs = config.alpm.syncdbs_mut().to_list();
    dbs.retain(|db| is_local_db(db));

    if !repos.is_empty() {
        dbs.retain(|db| repos.iter().any(|r| r.as_ref() == db.name()));
    }

    println!(
        "{} {}",
        c.action.paint("::"),
        c.bold.paint("syncing local databases...")
    );

    if !dbs.is_empty() {
        #[cfg(feature = "git")]
        dbs.update(false)?;
        #[cfg(not(feature = "git"))]
        for mut db in &dbs {
            println!("  syncing {}.db...", db.name());
            db.update(false)?;
        }
    } else {
        println!("  nothing to do");
    }

    Ok(0)
}

/*pub fn repo(globals: Globals, repo: Repo) -> Result<()> {
    sudo_reexec();

    let pacman = pacmanconf::Config::from_file(&globals.pacman_conf)?;

    if repo.create {
        for r in &repo.repos {
            let path = Path::new(&repo.path).join(r);
            init(&path, r, repo.quiet)?;
            println!("created repository {}", path.display());

            if repo.append {
                if pacman.repos.iter().any(|repo| repo.name == *r) {
                    eprintln!("error: repo {} already exists in pacman.conf", r);
                    continue;
                }
                println!("appending repo to pacman.conf");
                append(&globals, &path, r)?;
            }
        }
    } else if repo.add {
        let main_repo = match repo.repo {
            Some(ref r) => r.as_str(),
            None => {
                let repos = local_repos(&pacman);
                repos
                    .first()
                    .context("no local repos configured")?
                    .name
                    .as_str()
            }
        };

        let path = Path::new(&repo.path).join(&main_repo);
        add(&path, &main_repo, repo.quiet, repo.mv, &repo.repos)?;

        if repo.refresh {
            let mut cmd = globals.sudo_command();
            cmd.arg("refresh").arg(main_repo);
            run(cmd)?;
        }
    } else {
        for repo in local_repos(&pacman) {
            let file = file(&repo).unwrap();
            println!("[{}] {}", repo.name, file);
        }
    }
    Ok(())
}
*/
