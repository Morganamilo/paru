use crate::config::{Colors, Config, SortMode, YesNoAll};
use crate::fmt::print_indent;
use crate::printtr;
use crate::RaurHandle;

use std::collections::HashMap;
use std::io::Write;
use std::iter::FromIterator;
use std::process::{Command, Stdio};
use std::result::Result as StdResult;

use alpm::Version;
use alpm_utils::{AsTarg, DbListExt, Targ};
use ansi_term::Style;
use anyhow::{bail, ensure, Context, Result};
use aur_depends::AurBase;

use globset::GlobSet;
use indicatif::{ProgressBar, ProgressStyle};
use raur::{ArcPackage as Package, Raur};
use srcinfo::Srcinfo;
use tr::tr;
use url::Url;

#[derive(Debug, Clone, Default)]
pub struct Bases {
    pub bases: Vec<AurBase>,
}

impl FromIterator<Package> for Bases {
    fn from_iter<T: IntoIterator<Item = Package>>(iter: T) -> Self {
        let mut bases = Bases::new();
        bases.extend(iter);
        bases
    }
}

impl FromIterator<aur_depends::AurPackage> for Bases {
    fn from_iter<T: IntoIterator<Item = aur_depends::AurPackage>>(iter: T) -> Self {
        let mut bases = Bases::new();
        bases.extend_aur(iter);
        bases
    }
}

impl Bases {
    pub fn new() -> Self {
        Self { bases: Vec::new() }
    }

    pub fn push(&mut self, pkg: Package) {
        self.push_aur(aur_depends::AurPackage {
            pkg,
            make: false,
            target: false,
        })
    }

    pub fn push_aur(&mut self, pkg: aur_depends::AurPackage) {
        for base in &mut self.bases {
            if base.package_base() == pkg.pkg.package_base {
                base.pkgs.push(pkg);
                return;
            }
        }

        self.bases.push(AurBase {
            pkgs: vec![pkg],
            build: true,
        })
    }

    pub fn extend_aur<I: IntoIterator<Item = aur_depends::AurPackage>>(&mut self, iter: I) {
        iter.into_iter().for_each(|p| self.push_aur(p))
    }

    pub fn extend<I: IntoIterator<Item = Package>>(&mut self, iter: I) {
        iter.into_iter().for_each(|p| self.push(p))
    }
}

#[derive(Debug, Default)]
pub struct Warnings<'a> {
    pub pkgs: Vec<Package>,
    pub missing: Vec<&'a str>,
    pub ood: Vec<&'a str>,
    pub orphans: Vec<&'a str>,
}

impl<'a> Warnings<'a> {
    pub fn missing(&self, color: Colors, cols: Option<usize>) -> &Self {
        if !self.missing.is_empty() {
            let b = color.bold;
            let e = color.error;
            let msg = tr!("packages not in the AUR: ");
            print!("{} {}", e.paint("::"), b.paint(&msg));
            print_indent(Style::new(), msg.len() + 3, 4, cols, "  ", &self.missing);
        }
        self
    }

    pub fn ood(&self, color: Colors, cols: Option<usize>) -> &Self {
        if !self.ood.is_empty() {
            let b = color.bold;
            let e = color.error;
            let msg = tr!("marked out of date: ");
            print!("{} {}", e.paint("::"), b.paint(&msg));
            print_indent(Style::new(), msg.len() + 3, 4, cols, "  ", &self.ood);
        }
        self
    }

    pub fn orphans(&self, color: Colors, cols: Option<usize>) -> &Self {
        if !self.orphans.is_empty() {
            let b = color.bold;
            let e = color.error;
            let msg = tr!("orphans: ");
            print!("{} {}", e.paint("::"), b.paint(&msg));
            print_indent(Style::new(), msg.len() + 3, 4, cols, "  ", &self.orphans);
        }
        self
    }

    pub fn all(&self, color: Colors, cols: Option<usize>) {
        self.missing(color, cols);
        self.ood(color, cols);
        self.orphans(color, cols);
    }
}

pub async fn cache_info_with_warnings<'a, S: AsRef<str> + Send + Sync>(
    raur: &RaurHandle,
    cache: &'a mut raur::Cache,
    pkgs: &'a [S],
    ignore: &[String],
    no_warn: &GlobSet,
) -> StdResult<Warnings<'a>, raur::Error> {
    let mut missing = Vec::new();
    let mut ood = Vec::new();
    let mut orphaned = Vec::new();
    let mut aur_pkgs = raur.cache_info(cache, pkgs).await?;

    aur_pkgs.retain(|pkg1| pkgs.iter().any(|pkg2| pkg1.name == pkg2.as_ref()));

    for pkg in pkgs {
        if !no_warn.is_match(pkg.as_ref())
            && !ignore.iter().any(|p| p == pkg.as_ref())
            && !cache.contains(pkg.as_ref())
        {
            missing.push(pkg.as_ref())
        }
    }

    for pkg in &aur_pkgs {
        if no_warn.is_match(&pkg.name) && !ignore.iter().any(|p| p.as_str() == pkg.name) {
            if pkg.out_of_date.is_some() {
                ood.push(cache.get(pkg.name.as_str()).unwrap().name.as_str());
            }

            if pkg.maintainer.is_none() {
                orphaned.push(cache.get(pkg.name.as_str()).unwrap().name.as_str());
            }
        }
    }

    let ret = Warnings {
        pkgs: aur_pkgs,
        missing,
        ood,
        orphans: orphaned,
    };

    Ok(ret)
}

pub async fn getpkgbuilds(config: &mut Config) -> Result<i32> {
    let pkgs = config
        .targets
        .iter()
        .map(|t| t.as_str())
        .collect::<Vec<_>>();

    let (repo, aur) = split_repo_aur_pkgbuilds(config, &pkgs);
    let mut ret = 0;

    if !repo.is_empty() {
        ret = repo_pkgbuilds(config, &repo)?;
    }

    if !aur.is_empty() {
        let aur = aur.iter().map(|t| t.pkg).collect::<Vec<_>>();
        let action = config.color.action;
        let bold = config.color.bold;
        println!(
            "{} {}",
            action.paint("::"),
            bold.paint(tr!("Querying AUR..."))
        );
        let warnings = cache_info_with_warnings(
            &config.raur,
            &mut config.cache,
            &aur,
            &config.ignore,
            &GlobSet::empty(),
        )
        .await?;
        ret |= !warnings.missing.is_empty() as i32;
        warnings.missing(config.color, config.cols);
        let aur = warnings.pkgs;

        if !aur.is_empty() {
            let mut bases = Bases::new();
            bases.extend(aur);

            config.fetch.clone_dir = std::env::current_dir()?;

            aur_pkgbuilds(config, &bases).await?;
        }
    }
    Ok(ret)
}

fn repo_pkgbuilds(config: &Config, pkgs: &[Targ<'_>]) -> Result<i32> {
    let pkgctl = &config.pkgctl_bin;

    for (n, targ) in pkgs.iter().enumerate() {
        print_download(config, n + 1, pkgs.len(), targ.pkg);

        let ret = Command::new(pkgctl)
            .arg("repo")
            .arg("clone")
            .arg("--protocol")
            .arg("https")
            .arg(targ.to_string())
            .output()
            .with_context(|| {
                format!(
                    "{} {} export {}",
                    tr!("failed to run:"),
                    pkgctl,
                    targ.to_string()
                )
            })?;

        ensure!(
            ret.status.success(),
            "{}",
            String::from_utf8_lossy(&ret.stderr).trim()
        );
    }

    Ok(0)
}

pub fn print_download(_config: &Config, n: usize, total: usize, pkg: &str) {
    let total = total.to_string();
    println!(
        " ({n:>padding$}/{total}) {}",
        tr!("downloading: {pkg}", pkg),
        padding = total.len(),
        n = n,
        total = total,
    );
}

async fn aur_pkgbuilds(config: &Config, bases: &Bases) -> Result<()> {
    let download = bases
        .bases
        .iter()
        .map(|p| p.package_base())
        .collect::<Vec<_>>();

    let cols = config.cols.unwrap_or(0);

    let action = config.color.action;
    let bold = config.color.bold;

    println!(
        "\n{} {}",
        action.paint("::"),
        bold.paint(tr!("Downloading PKGBUILDs..."))
    );

    if bases.bases.is_empty() {
        printtr!(" PKGBUILDs up to date");
        return Ok(());
    }

    if cols < 80 {
        config.fetch.download_cb(&download, |cb| {
            let base = bases
                .bases
                .iter()
                .find(|b| b.package_base() == cb.pkg)
                .unwrap();

            print_download(config, cb.n, download.len(), &base.to_string());
        })?;
    } else {
        let total = download.len().to_string();
        let template = format!(
            " ({{pos:>{}}}/{{len}}) {{prefix:45!}} [{{wide_bar}}]",
            total.len()
        );
        let pb = ProgressBar::new(download.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template(&template)?
                .progress_chars("-> "),
        );

        config.fetch.download_cb(&download, |cb| {
            let base = bases
                .bases
                .iter()
                .find(|b| b.package_base() == cb.pkg)
                .unwrap();

            pb.inc(1);
            pb.set_prefix(base.to_string());
        })?;

        pb.finish();
    }

    Ok(())
}

pub async fn new_aur_pkgbuilds(
    config: &Config,
    bases: &Bases,
    srcinfos: &HashMap<String, Srcinfo>,
) -> Result<()> {
    let mut pkgs = Vec::new();

    if bases.bases.is_empty() {
        return Ok(());
    }

    let all_pkgs = bases
        .bases
        .iter()
        .map(|b| b.package_base())
        .collect::<Vec<_>>();

    if config.redownload == YesNoAll::All {
        aur_pkgbuilds(config, bases).await?;
        config.fetch.merge(&all_pkgs)?;
        return Ok(());
    }

    for base in &bases.bases {
        if config.redownload == YesNoAll::Yes && base.pkgs.iter().any(|p| p.target) {
            pkgs.push(base.clone());
            continue;
        }
        if let Some(pkg) = srcinfos.get(base.package_base()) {
            let upstream_ver = base.version();
            if Version::new(pkg.version()) < Version::new(&*upstream_ver) {
                pkgs.push(base.clone());
            }
        } else {
            pkgs.push(base.clone());
        }
    }

    let new_bases = Bases { bases: pkgs };
    aur_pkgbuilds(config, &new_bases).await?;
    config.fetch.merge(&all_pkgs)?;

    Ok(())
}

pub async fn show_comments(config: &mut Config) -> Result<i32> {
    let client = config.raur.client();

    let warnings = cache_info_with_warnings(
        &config.raur,
        &mut config.cache,
        &config.targets,
        &[],
        &GlobSet::empty(),
    )
    .await?;
    warnings.missing(config.color, config.cols);
    let ret = !warnings.missing.is_empty() as i32;
    let bases = Bases::from_iter(warnings.pkgs);
    let c = config.color;

    for base in &bases.bases {
        let url = config
            .aur_url
            .join(&format!("packages/{}", base.package_base()))?;

        let response = client
            .get(url.clone())
            .send()
            .await
            .with_context(|| format!("{}: {}", base, url))?;
        if !response.status().is_success() {
            bail!("{}: {}: {}", base, url, response.status());
        }

        let document = scraper::Html::parse_document(&response.text().await?);
        let titles_selector = scraper::Selector::parse("div.comments h4.comment-header").unwrap();
        let comments_selector =
            scraper::Selector::parse("div.comments div.article-content").unwrap();

        let titles = document
            .select(&titles_selector)
            .map(|node| node.text().collect::<String>());

        let comments = document
            .select(&comments_selector)
            .map(|node| node.text().collect::<String>());

        let iter = titles.zip(comments).collect::<Vec<_>>();

        if config.sort_mode == SortMode::TopDown {
            for (title, comment) in iter.into_iter() {
                print_indent(c.bold, 0, 0, config.cols, " ", title.split_whitespace());

                for line in comment.trim().split('\n') {
                    let line = line.split_whitespace();
                    print!("    ");
                    print_indent(Style::new(), 4, 4, config.cols, " ", line);
                }
                println!();
            }
        } else {
            for (title, comment) in iter.into_iter().rev() {
                print_indent(c.bold, 0, 0, config.cols, " ", title.split_whitespace());

                for line in comment.trim().split('\n') {
                    let line = line.split_whitespace();
                    print!("    ");
                    print_indent(Style::new(), 4, 4, config.cols, " ", line);
                }
                println!();
            }
        }
    }

    Ok(ret)
}

fn split_repo_aur_pkgbuilds<'a, T: AsTarg>(
    config: &Config,
    targets: &'a [T],
) -> (Vec<Targ<'a>>, Vec<Targ<'a>>) {
    let mut local = Vec::new();
    let mut aur = Vec::new();
    let db = config.alpm.syncdbs();

    for targ in targets {
        let targ = targ.as_targ();
        if !config.mode.repo() {
            aur.push(targ);
        } else if !config.mode.aur() && !config.mode.pkgbuild() {
            local.push(targ);
        } else if let Some(repo) = targ.repo {
            if matches!(
                repo,
                "testing" | "community-testing" | "core" | "extra" | "community" | "multilib"
            ) {
                local.push(targ);
            } else {
                aur.push(targ);
            }
        } else if let Ok(pkg) = db.pkg(targ.pkg) {
            if matches!(
                pkg.db().unwrap().name(),
                "testing" | "community-testing" | "core" | "extra" | "community" | "multilib"
            ) {
                local.push(targ);
            } else {
                aur.push(targ);
            }
        } else {
            aur.push(targ);
        }
    }

    (local, aur)
}

pub async fn show_pkgbuilds(config: &mut Config) -> Result<i32> {
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    let bat = config.color.enabled && Command::new(&config.bat_bin).arg("-V").output().is_ok();
    let client = config.raur.client();

    let (repo, aur) = split_repo_aur_pkgbuilds(config, &config.targets);

    if !repo.is_empty() {
        for pkg in &repo {
            let url = Url::parse(&format!(
                "https://gitlab.archlinux.org/archlinux/packaging/packages/{}/-/raw/HEAD/PKGBUILD",
                pkg.pkg
            ))?;

            let response = client
                .get(url.clone())
                .send()
                .await
                .with_context(|| format!("{}: {}", pkg, url))?;
            if !response.status().is_success() {
                bail!("{}: {}: {}", pkg, url, response.status());
            }

            if bat {
                pipe_bat(config, &response.bytes().await?)?;
            } else {
                let _ = stdout.write_all(&response.bytes().await?);
            }

            let _ = stdout.write_all(b"\n");
        }
    }

    if !aur.is_empty() {
        let aur = aur.iter().map(|t| t.pkg).collect::<Vec<_>>();

        let warnings = cache_info_with_warnings(
            &config.raur,
            &mut config.cache,
            &aur,
            &[],
            &GlobSet::empty(),
        )
        .await?;
        warnings.missing(config.color, config.cols);
        let ret = !warnings.missing.is_empty() as i32;
        let bases = Bases::from_iter(warnings.pkgs);

        for base in &bases.bases {
            let base = base.package_base();
            let url = config.aur_url.join("cgit/aur.git/plain/PKGBUILD").unwrap();
            let url = Url::parse_with_params(url.as_str(), &[("h", base)]).unwrap();

            let response = client
                .get(url.clone())
                .send()
                .await
                .with_context(|| format!("{}: {}", base, url))?;
            if !response.status().is_success() {
                bail!("{}: {}: {}", base, url, response.status());
            }

            if bat {
                pipe_bat(config, &response.bytes().await?)?;
            } else {
                let _ = stdout.write_all(&response.bytes().await?);
            }

            let _ = stdout.write_all(b"\n");
        }

        return Ok(ret);
    }

    Ok(0)
}

fn pipe_bat(config: &Config, pkgbuild: &[u8]) -> Result<()> {
    let mut command = Command::new(&config.bat_bin)
        .arg("-pp")
        .arg("--color=always")
        .arg("-lPKGBUILD")
        .args(&config.bat_flags)
        .stdin(Stdio::piped())
        .spawn()?;

    let _ = command.stdin.as_mut().unwrap().write_all(pkgbuild);
    command.wait()?;
    Ok(())
}
