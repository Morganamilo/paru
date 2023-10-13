use std::{
    cell::OnceCell,
    env::current_dir,
    fs::{read_dir, File},
    io::Write,
    path::{Path, PathBuf},
};

use crate::{download::print_download, exec};
use anyhow::{anyhow, bail, Context, Result};
use aur_fetch::Fetch;
use indicatif::{ProgressBar, ProgressStyle};
use srcinfo::Srcinfo;
use tr::tr;
use url::Url;

use crate::{config::Config, print_error};

#[derive(Debug, Default)]
pub enum RepoSource {
    Url(Url),
    Path(PathBuf),
    #[default]
    None,
}

impl RepoSource {
    pub fn url(&self) -> Option<&Url> {
        match self {
            RepoSource::Url(url) => Some(url),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct PkgbuildPkg {
    pub repo: String,
    pub srcinfo: Srcinfo,
    pub path: PathBuf,
}

#[derive(Default, Debug)]
pub struct PkgbuildRepo {
    pub name: String,
    pub source: RepoSource,
    pub depth: u32,
    pub skip_review: bool,
    pub force_srcinfo: bool,
    path: PathBuf,
    pkgs: OnceCell<Vec<PkgbuildPkg>>,
}

impl PkgbuildRepo {
    pub fn new(name: String, path: PathBuf) -> Self {
        PkgbuildRepo {
            depth: 2,
            path,
            name,
            source: RepoSource::None,
            skip_review: false,
            force_srcinfo: false,
            pkgs: OnceCell::new(),
        }
    }

    pub fn path(&self) -> Result<&Path> {
        match &self.source {
            RepoSource::Url(_) => Ok(self.path.as_path()),
            RepoSource::Path(path) => Ok(path.as_path()),
            RepoSource::None => bail!(tr!("repo {} does not have a URL or Path")),
        }
    }

    pub fn pkgs(&self, config: &Config) -> &[PkgbuildPkg] {
        self.pkgs.get_or_init(move || self.read_pkgs(config))
    }

    pub fn base(&self, config: &Config, base: &str) -> Option<&PkgbuildPkg> {
        self.pkgs(config)
            .iter()
            .find(|p| p.srcinfo.base.pkgbase == base)
    }

    pub fn pkg(&self, config: &Config, pkg: &str) -> Option<(&PkgbuildPkg, &srcinfo::Package)> {
        self.pkgs(config)
            .iter()
            .find_map(|srcinfo| srcinfo.srcinfo.pkg(pkg).map(|p| (srcinfo, p)))
    }

    pub fn from_pkgbuilds(config: &Config, dirs: &[PathBuf]) -> Result<PkgbuildRepo> {
        let repo = Self::new(".".to_string(), ".".into());
        let c = config.color;
        let mut pkgs = Vec::new();

        for dir in dirs {
            println!(
                "{} {}",
                c.action.paint("::"),
                c.bold.paint(tr!(
                    "Generating .SRCINFO for {dir}...",
                    repo = repo.name,
                    dir = dir.display()
                ))
            );

            let srcinfo = read_srcinfo_from_pkgbuild(config, &dir)?;
            pkgs.push(PkgbuildPkg {
                repo: repo.name.clone(),
                srcinfo,
                path: dir.clone(),
            });
        }

        Ok(repo)
    }

    fn read_pkgs(&self, config: &Config) -> Vec<PkgbuildPkg> {
        if !config.mode.pkgbuild() {
            return Vec::new();
        }
        if matches!(self.source, RepoSource::Url(_)) && !self.path.join(".git").exists() {
            eprintln!(
                "{} {}",
                config.color.warning.paint("::"),
                tr!(
                    "repo {} not downloaded (use -Sy --pkgbuilds to download)",
                    self.name
                )
            );
        }

        self.for_each_pkgbuild(Vec::new(), |path, data| match self.read_pkg(config, path) {
            Ok(srcinfo) => data.push(srcinfo),
            Err(e) => print_error(config.color.error, e),
        })
    }

    fn generate_srcinfos(&self, config: &Config) {
        self.try_for_each_pkgbuild((), |path, _| {
            if let Err(e) = self.generate_srcinfo(config, path) {
                print_error(config.color.error, e);
            }
            Ok(())
        })
        .unwrap();
    }

    fn generate_srcinfo(&self, config: &Config, path: &Path) -> Result<()> {
        let c = config.color;

        if !self.force_srcinfo && path.join(".SRCINFO").exists() {
            return Ok(());
        }

        println!(
            "{} {}",
            c.action.paint("::"),
            c.bold.paint(tr!(
                "Generating .SRCINFO for {repo}/{dir}...",
                repo = self.name,
                dir = path.file_name().unwrap().to_string_lossy()
            ))
        );

        let output = exec::makepkg_output(config, path, &["--printsrcinfo"]);
        match output {
            Ok(output) => {
                let mut file = File::create(path.join(".SRCINFO"))?;
                file.write_all(&output.stdout)?;
            }
            Err(e) => {
                print_error(config.color.error, e);
            }
        }

        Ok(())
    }

    fn for_each_pkgbuild<T, F: Fn(&Path, &mut T)>(&self, data: T, f: F) -> T {
        self.try_for_each_pkgbuild(data, |path, data| {
            f(path, data);
            Ok(())
        })
        .unwrap()
    }

    fn try_for_each_pkgbuild<T, F: Fn(&Path, &mut T) -> Result<()>>(
        &self,
        mut data: T,
        f: F,
    ) -> Result<T> {
        self.try_for_each_pkgbuild_internal(&mut data, &f, self.path()?, self.depth)?;
        Ok(data)
    }

    fn try_for_each_pkgbuild_internal<T, F: Fn(&Path, &mut T) -> Result<()>>(
        &self,
        data: &mut T,
        f: &F,
        path: &Path,
        depth: u32,
    ) -> Result<()> {
        if depth == 0 {
            return Ok(());
        }

        if path.join("PKGBUILD").exists() {
            f(path, data)?;
        }

        if depth == 1 {
            return Ok(());
        }

        let dir = read_dir(path).context(path.display().to_string())?;

        for entry in dir {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => return Ok(()),
            };

            if entry.file_type()?.is_dir() {
                self.try_for_each_pkgbuild_internal(data, f, &entry.path(), depth - 1)?;
            }
        }

        Ok(())
    }

    fn read_pkg(&self, config: &Config, path: &Path) -> Result<PkgbuildPkg> {
        let srcinfo_path = path.join(".SRCINFO");

        if !srcinfo_path.exists() {
            self.generate_srcinfo(config, path)?;
        }

        let srcinfo = Srcinfo::parse_file(&srcinfo_path);
        match srcinfo {
            Ok(srcinfo) => Ok(PkgbuildPkg {
                repo: self.name.to_string(),
                srcinfo,
                path: path.to_path_buf(),
            }),
            Err(err) => Err(anyhow!(err).context(tr!(
                "failed to parse srcinfo \"{}\"",
                srcinfo_path.display().to_string()
            ))),
        }
    }
}

#[derive(Debug)]
pub struct PkgbuildRepos {
    pub fetch: Fetch,
    pub repos: Vec<PkgbuildRepo>,
}

impl PkgbuildRepos {
    pub fn new(fetch: Fetch) -> Self {
        Self {
            fetch,
            repos: Vec::new(),
        }
    }

    pub fn add_repo(&mut self, name: String) -> &mut PkgbuildRepo {
        let path = self.fetch.clone_dir.join(&name);
        self.repos.push(PkgbuildRepo::new(name, path));
        self.repos.last_mut().unwrap()
    }

    pub fn add_cwd(&mut self) -> Result<&mut PkgbuildRepo> {
        let dir = current_dir()?;
        let repo = PkgbuildRepo {
            name: ".".to_string(),
            source: RepoSource::Path(dir.clone()),
            depth: 3,
            skip_review: true,
            force_srcinfo: false,
            path: dir,
            pkgs: Default::default(),
        };
        self.repos.push(repo);
        Ok(self.repos.last_mut().unwrap())
    }

    pub fn repo(&self, name: &str) -> Option<&PkgbuildRepo> {
        self.repos.iter().find(|r| r.name == name)
    }

    pub fn pkg(&self, config: &Config, name: &str) -> Option<(&PkgbuildPkg, &srcinfo::Package)> {
        self.repos
            .iter()
            .flat_map(|r| r.pkgs(config))
            .find_map(|s| s.srcinfo.pkg(name).map(|p| (s, p)))
    }

    pub fn repo_mut(&mut self, name: &str) -> Option<&mut PkgbuildRepo> {
        self.repos.iter_mut().find(|r| r.name == name)
    }

    pub fn refresh(&self, config: &Config) -> Result<()> {
        if !config.mode.pkgbuild() || self.repos.is_empty() {
            return Ok(());
        }

        let cols = config.cols.unwrap_or(0);
        let action = config.color.action;
        let bold = config.color.bold;

        let repos = self
            .repos
            .iter()
            .filter_map(|r| {
                r.source
                    .url()
                    .map(|u| (r.name.as_str(), u))
                    .map(|(n, u)| aur_fetch::Repo {
                        url: u.clone(),
                        name: n.to_string(),
                    })
            })
            .collect::<Vec<_>>();

        println!(
            "\n{} {}",
            action.paint("::"),
            bold.paint(tr!("Downloading PKGBUILD Repos..."))
        );

        if cols < 80 {
            self.fetch.download_repos_cb(&repos, |cb| {
                print_download(config, cb.n, repos.len(), cb.pkg);
            })?;
        } else {
            let total = repos.len().to_string();
            let template = format!(
                " ({{pos:>{}}}/{{len}}) {{prefix:45!}} [{{wide_bar}}]",
                total.len()
            );
            let pb = ProgressBar::new(repos.len() as u64);
            pb.set_style(
                ProgressStyle::default_bar()
                    .template(&template)?
                    .progress_chars("-> "),
            );

            self.fetch.download_repos_cb(&repos, |cb| {
                pb.inc(1);
                pb.set_prefix(cb.pkg.to_string());
            })?;

            pb.finish();
        }

        self.repos.iter().for_each(|r| r.generate_srcinfos(config));
        Ok(())
    }
}

pub fn read_srcinfo_from_pkgbuild(config: &Config, dir: &Path) -> Result<Srcinfo> {
    let output = exec::makepkg_output(config, &dir, &["--printsrcinfo"])
        .with_context(|| dir.display().to_string())?;
    let srcinfo = Srcinfo::parse_buf(output.stdout.as_slice())
        .context(tr!("failed to parse srcinfo generated by makepkg"))
        .with_context(|| dir.display().to_string())?;
    Ok(srcinfo)
}
