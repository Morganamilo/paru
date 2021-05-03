use crate::config::{Config, LocalRepos};
use crate::exec;

use std::env::current_exe;
use std::ffi::{OsStr, OsString};
use std::fs::read_link;
use std::path::{Path, PathBuf};
use std::process::Command;

use alpm::{AlpmListMut, Db};
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
    )?;

    if !pkgs.is_empty() {
        let cmd = if mv {
            OsString::from("mv")
        } else {
            OsString::from("cp")
        };

        let mut args = vec![cmd, OsString::from("-f")];

        for pkg in pkgs {
            let mut sig = pkg.as_ref().to_os_string();
            sig.push(".sig");
            if Path::new(&sig).exists() {
                args.push(sig);
            }
        }

        args.extend(pkgs.iter().map(OsString::from));
        args.push(path.as_os_str().to_os_string());
        exec::command(sudo, ".", &args)?;
    }

    let mut args = vec![OsStr::new("repo-add"), OsStr::new("-R"), file.as_os_str()];
    let pkgs = pkgs
        .iter()
        .map(|p| path.join(Path::new(p.as_ref()).file_name().unwrap()))
        .collect::<Vec<_>>();

    args.extend(pkgs.iter().map(|p| p.as_os_str()));
    exec::command(sudo, ".", &args)?;

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
    exec::command(&config.sudo_bin, ".", &args)?;

    Ok(())
}

pub fn init<P: AsRef<Path>>(config: &Config, path: P, name: &str) -> Result<()> {
    let pkgs: &[&str] = &[];
    add(config, path, name, false, pkgs)
}

fn is_configured_local_db(config: &Config, db: &Db) -> bool {
    match config.repos {
        LocalRepos::None => false,
        LocalRepos::Default => is_local_db(db),
        LocalRepos::Repo(ref r) => is_local_db(db) && r.iter().any(|r| *r == db.name()),
    }
}

pub fn file<'a>(repo: &Db<'a>) -> Option<&'a str> {
    repo.servers()
        .first()
        .map(|s| s.trim_start_matches("file://"))
}

pub fn all_files(config: &Config) -> Vec<String> {
    config
        .alpm
        .syncdbs()
        .iter()
        .flat_map(|db| db.servers())
        .filter(|f| f.starts_with("file://"))
        .map(|s| s.trim_start_matches("file://").to_string())
        .collect()
}

fn is_local_db(db: &alpm::Db) -> bool {
    !db.servers().is_empty() && db.servers().iter().all(|s| s.starts_with("file://"))
}

pub fn repo_aur_dbs(config: &Config) -> (AlpmListMut<Db>, AlpmListMut<Db>) {
    let dbs = config.alpm.syncdbs();
    let mut aur = dbs.to_list();
    let mut repo = dbs.to_list();
    aur.retain(|db| is_configured_local_db(config, db));
    repo.retain(|db| !is_configured_local_db(config, db));
    (repo, aur)
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
            .arg("-Lu")
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
