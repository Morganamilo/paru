use std::{
    cell::{Cell, OnceCell},
    env::current_dir,
    ffi::OsStr,
    fs::{File, read_dir},
    io::Write,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use crate::{
    download::print_download,
    exec,
    install::{merge_reviewed, review, seen_repos},
};
use alpm_utils::Targ;
use anyhow::{Context, Result, anyhow, bail};
use aur_fetch::Fetch;
use indicatif::{ProgressBar, ProgressStyle};
use srcinfo::Srcinfo;
use tr::tr;
use url::Url;

use crate::{config::Config, print_error};

#[derive(Debug, Default, Clone)]
pub enum RepoSource {
    Url(Url, Option<PathBuf>),
    Path(PathBuf),
    #[default]
    None,
}

impl RepoSource {
    pub fn url(&self) -> Option<&Url> {
        match self {
            RepoSource::Url(url, _) => Some(url),
            _ => None,
        }
    }

    pub fn set_path<P: Into<PathBuf>>(&mut self, p: P) {
        match self {
            RepoSource::Url(_, path) => *path = Some(p.into()),
            _ => *self = RepoSource::Path(p.into()),
        }
    }

    pub fn set_url(&mut self, u: Url) {
        match self {
            RepoSource::Url(url, _) => *url = u,
            RepoSource::Path(path) => *self = RepoSource::Url(u, Some(path.clone())),
            _ => *self = RepoSource::Url(u, None),
        }
    }
}

#[derive(Debug)]
pub struct PkgbuildPkg {
    pub repo: String,
    pub srcinfo: Srcinfo,
    pub path: PathBuf,
}

#[derive(Default, Debug, Clone)]
pub struct PkgbuildRepo {
    pub name: String,
    pub source: RepoSource,
    pub depth: u32,
    pub skip_review: bool,
    pub force_srcinfo: bool,
    pub path: PathBuf,
    pkgs: OnceCell<Arc<Vec<PkgbuildPkg>>>,
    warned: Cell<bool>,
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
            warned: Cell::new(false),
        }
    }

    pub fn path(&self) -> Result<PathBuf> {
        match &self.source {
            RepoSource::Url(_, Some(path)) => {
                Ok(self.path.join(path.strip_prefix("/").unwrap_or(path)))
            }
            RepoSource::Url(_, None) => Ok(self.path.clone()),
            RepoSource::Path(path) => Ok(path.clone()),
            RepoSource::None => bail!(tr!("repo {} does not have a URL or Path")),
        }
    }

    pub fn pkgs(&self, config: &Config) -> &[PkgbuildPkg] {
        if let Some(pkgs) = self.pkgs.get() {
            return pkgs;
        }
        let Some(pkgs) = self.read_pkgs(config) else {
            return &[];
        };
        self.pkgs.get_or_init(|| Arc::new(pkgs))
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

    pub fn from_cwd(config: &Config) -> Result<PkgbuildRepo> {
        let dir = current_dir()?;
        let repo = PkgbuildRepo {
            name: ".".to_string(),
            source: RepoSource::Path(dir.clone()),
            depth: 3,
            skip_review: true,
            force_srcinfo: false,
            path: dir,
            pkgs: Default::default(),
            warned: Cell::new(false),
        };

        repo.pkgs(config);
        Ok(repo)
    }

    pub fn from_pkgbuilds(config: &Config, dirs: &[PathBuf]) -> Result<PkgbuildRepo> {
        let mut pkgs = Vec::new();
        let mut repo = Self::from_cwd(config)?;

        for dir in dirs {
            let dir = dir.canonicalize()?;
            repo.print_generate_srcinfo(config, &dir.file_name().unwrap().to_string_lossy());
            let srcinfo = read_srcinfo_from_pkgbuild(config, &dir)?;
            pkgs.push(PkgbuildPkg {
                repo: repo.name.clone(),
                srcinfo,
                path: dir.clone(),
            });
        }

        repo.pkgs = OnceCell::from(Arc::new(pkgs));
        Ok(repo)
    }

    fn read_pkgs(&self, config: &Config) -> Option<Vec<PkgbuildPkg>> {
        if matches!(self.source, RepoSource::Url(_, _)) {
            if !self.path.join(".git").exists() {
                if !self.warned.replace(true) {
                    eprintln!(
                        "{} {}",
                        config.color.warning.paint("::"),
                        tr!(
                            "repo {} not downloaded (use -Sy --pkgbuilds to download)",
                            self.name
                        )
                    );
                }
                return None;
            } else if !self.skip_review && !self.reviewed(config) {
                if !self.warned.replace(true) {
                    eprintln!(
                        "{} {}",
                        config.color.warning.paint("::"),
                        tr!(
                            "repo {} has unreviewed changes (use -Sy --pkgbuilds to review)",
                            self.name
                        )
                    );
                }
                return None;
            }
        }

        Some(
            self.for_each_pkgbuild(Vec::new(), |path, data| match self.read_pkg(config, path) {
                Ok(srcinfo) => data.push(srcinfo),
                Err(e) => print_error(config.color.error, e),
            }),
        )
    }

    fn reviewed(&self, config: &Config) -> bool {
        let mut cmd = Command::new(&config.git_bin);
        cmd.current_dir(&self.path).args(&config.git_flags).args([
            "-c",
            "core.warnAmbiguousRefs=false",
            "rev-parse",
            "AUR_SEEN^{tree}",
            "HEAD^{tree}",
            "--symbolic-full-name",
            "AUR_SEEN",
            "--absolute-git-dir",
        ]);

        exec::command_output(&mut cmd).is_ok_and(|output| {
            let mut lines = output.stdout.splitn(4, |b| *b == b'\n');
            let (Some(seen), Some(head), Some(reference), Some(git_dir)) =
                (lines.next(), lines.next(), lines.next(), lines.next())
            else {
                return false;
            };
            let Some(git_dir) = git_dir.strip_suffix(b"\n") else {
                return false;
            };
            if seen.is_empty() || seen != head || reference != b"AUR_SEEN" || git_dir.is_empty() {
                return false;
            }

            // An interrupted merge can leave unreviewed files with HEAD still at AUR_SEEN.
            let git_dir = Path::new(OsStr::from_bytes(git_dir));
            ["MERGE_HEAD", "rebase-merge", "rebase-apply"]
                .iter()
                .all(|name| git_dir.join(name).try_exists().is_ok_and(|exists| !exists))
        })
    }

    fn generate_srcinfos(&self, config: &Config) {
        self.for_each_pkgbuild((), |path, _| {
            if let Err(e) = self.generate_srcinfo(config, path) {
                print_error(config.color.error, e);
            }
        })
    }

    fn generate_srcinfo(&self, config: &Config, path: &Path) -> Result<()> {
        if !self.force_srcinfo && path.join(".SRCINFO").exists() {
            return Ok(());
        }

        self.print_generate_srcinfo(config, &path.file_name().unwrap().to_string_lossy());
        let output = exec::makepkg_output(config, path, &["--printsrcinfo"])
            .context(path.display().to_string());
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

    fn print_generate_srcinfo(&self, config: &Config, pkg: &str) {
        let c = config.color;
        println!(
            "{} {}",
            c.action.paint("::"),
            c.bold.paint(tr!(
                "Generating .SRCINFO for {repo}/{dir}...",
                repo = self.name,
                dir = pkg,
            ))
        );
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
        let path = self.path()?;
        if path.exists() {
            Self::try_for_each_pkgbuild_internal(&mut data, &f, &path, self.depth)?;
        }
        Ok(data)
    }

    fn try_for_each_pkgbuild_internal<T, F: Fn(&Path, &mut T) -> Result<()>>(
        data: &mut T,
        f: &F,
        path: &Path,
        depth: u32,
    ) -> Result<()> {
        if depth == 0 {
            return Ok(());
        }

        //log::debug!("for each pkgbuild: {}", path.display());

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
                Self::try_for_each_pkgbuild_internal(data, f, &entry.path(), depth - 1)?;
            }
        }

        Ok(())
    }

    fn read_pkg(&self, config: &Config, path: &Path) -> Result<PkgbuildPkg> {
        let srcinfo_path = path.join(".SRCINFO");

        if !srcinfo_path.exists() {
            self.generate_srcinfo(config, path)?;
        }

        let srcinfo = Srcinfo::from_path(&srcinfo_path);
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

#[derive(Debug, Clone)]
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
        self.repos
            .push(PkgbuildRepo::new(name.clone(), name.into()));
        self.repos.last_mut().unwrap()
    }

    pub fn repo(&self, name: &str) -> Option<&PkgbuildRepo> {
        self.repos.iter().find(|r| r.name == name)
    }

    pub fn target(
        &self,
        config: &Config,
        target: Targ,
    ) -> Option<(&PkgbuildPkg, &srcinfo::Package)> {
        if let Some(repo) = target.repo {
            self.repo(repo).and_then(|r| r.pkg(config, target.pkg))
        } else {
            self.pkg(config, target.pkg)
        }
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

    pub fn ensure_reviewed(&self, config: &Config) -> Result<()> {
        for repo in &self.repos {
            if matches!(repo.source, RepoSource::Url(_, _))
                && !repo.skip_review
                && repo.path.join(".git").exists()
                && !repo.reviewed(config)
            {
                bail!(tr!(
                    "repo {} has unreviewed changes (use -Sy --pkgbuilds to review)",
                    repo.name
                ));
            }
        }
        Ok(())
    }

    pub fn aur_depends_repo(&self, config: &Config) -> Vec<aur_depends::PkgbuildRepo<'_>> {
        self.repos
            .iter()
            .map(|r| aur_depends::PkgbuildRepo {
                name: &r.name,
                pkgs: r.pkgs(config).iter().map(|p| &p.srcinfo).collect(),
            })
            .collect()
    }

    pub fn refresh(&self, config: &Config) -> Result<()> {
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

        if repos.is_empty() {
            return Ok(());
        }

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
            println!();
        }

        let review_repos = repos
            .iter()
            .filter(|r| {
                !config
                    .pkgbuild_repos
                    .repo(&r.name)
                    .map(|r| r.skip_review)
                    .unwrap_or(false)
            })
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>();

        // Without a seen marker, review reads the working tree rather than a diff.
        let seen = seen_repos(&self.fetch, &review_repos);
        let never_accepted = review_repos
            .iter()
            .copied()
            .filter(|r| !seen.contains(r))
            .collect::<Vec<_>>();
        self.fetch.merge(&never_accepted)?;

        let reviewed = review(config, &self.fetch, &review_repos, true)?;

        let all_repos = repos.iter().map(|r| r.name.as_str()).collect::<Vec<_>>();
        merge_reviewed(&self.fetch, &reviewed, &all_repos)?;

        self.repos.iter().for_each(|r| r.generate_srcinfos(config));
        Ok(())
    }
}

pub fn read_srcinfo_from_pkgbuild(config: &Config, dir: &Path) -> Result<Srcinfo> {
    let output = exec::makepkg_output(config, dir, &["--printsrcinfo"])
        .with_context(|| dir.display().to_string())?;
    let srcinfo = Srcinfo::from_buf(output.stdout.as_slice())
        .context(tr!("failed to parse srcinfo generated by makepkg"))
        .with_context(|| dir.display().to_string())?;
    Ok(srcinfo)
}

#[cfg(test)]
mod tests {
    use super::{PkgbuildRepo, PkgbuildRepos, RepoSource};
    use crate::config::Config;
    use std::{fs, process::Command};

    #[test]
    fn rejected_repo_is_loaded_and_resolved_after_its_tree_is_accepted() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let git = |args: &[&str]| {
            let output = Command::new("git")
                .current_dir(dir)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .args([
                    "-c",
                    "user.name=fixture",
                    "-c",
                    "user.email=fixture@localhost",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
        };
        git(&["init", "-b", "master"]);
        fs::write(dir.join("PKGBUILD"), "exit 99\n").unwrap();
        fs::write(
            dir.join(".SRCINFO"),
            "pkgbase = fixture\n\tpkgver = 1\n\tpkgrel = 1\npkgname = fixture\n",
        )
        .unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "fixture"]);

        let mut repo = PkgbuildRepo::new("fixture".into(), dir.into());
        repo.source = RepoSource::Url("https://paru.invalid/fixture.git".parse().unwrap(), None);
        let config = Config::default();
        let mut repos = PkgbuildRepos::new(aur_fetch::Fetch::with_cache_dir(dir));
        repos.repos.push(repo);
        assert!(repos.repos[0].pkgs(&config).is_empty());
        assert!(repos.ensure_reviewed(&config).is_err());

        repos.repos[0].skip_review = true;
        assert!(repos.ensure_reviewed(&config).is_ok());
        repos.repos[0].skip_review = false;
        let source = repos.repos[0].source.clone();
        repos.repos[0].source = RepoSource::Path(dir.into());
        assert!(repos.ensure_reviewed(&config).is_ok());
        repos.repos[0].source = source;

        git(&["update-ref", "AUR_SEEN", "HEAD"]);
        assert!(repos.ensure_reviewed(&config).is_ok());
        let pkgs = repos.repos[0].pkgs(&config);
        assert_eq!(pkgs.len(), 1);
        assert_eq!(pkgs[0].srcinfo.pkgbase(), "fixture");

        fs::write(dir.join("PKGBUILD"), "exit 98\n").unwrap();
        git(&["commit", "-am", "unreviewed"]);
        assert!(repos.ensure_reviewed(&config).is_err());

        repos.repos[0].path = dir.join("missing");
        assert!(repos.ensure_reviewed(&config).is_ok());
    }
}
