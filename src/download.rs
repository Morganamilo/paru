use crate::config::{Colors, Config};
use crate::fmt::print_indent;
use crate::util::split_repo_aur_targets;

use std::collections::{HashMap, HashSet};
use std::fs::read_dir;
use std::io::Write;
use std::iter::FromIterator;
use std::process::{Command, Stdio};
use std::result::Result as StdResult;

use alpm::Version;
use alpm_utils::{DbListExt, Targ};
use ansi_term::Style;
use anyhow::{bail, ensure, Context, Result};
use aur_depends::Base;
use indicatif::{ProgressBar, ProgressStyle};
use raur_ext::{Package, RaurExt};
use srcinfo::Srcinfo;
use url::Url;

#[derive(Debug, Clone)]
pub struct Bases {
    pub bases: Vec<Base>,
}

impl FromIterator<Package> for Bases {
    fn from_iter<T: IntoIterator<Item = Package>>(iter: T) -> Self {
        let mut bases = Bases::new();
        bases.extend(iter);
        bases
    }
}

impl Bases {
    pub fn new() -> Self {
        Self { bases: Vec::new() }
    }

    pub fn push(&mut self, pkg: Package) {
        for base in &mut self.bases {
            if base.package_base() == pkg.package_base {
                base.pkgs.push(aur_depends::AurPackage {
                    pkg,
                    make: false,
                    target: false,
                });
                return;
            }
        }

        self.bases.push(Base {
            pkgs: vec![aur_depends::AurPackage {
                pkg,
                make: false,
                target: false,
            }],
        })
    }

    pub fn extend<I: IntoIterator<Item = Package>>(&mut self, iter: I) {
        iter.into_iter().for_each(|p| self.push(p))
    }
}

#[derive(Debug, Default)]
pub struct Warnings<'a> {
    pub pkgs: Vec<raur_ext::Package>,
    pub missing: Vec<&'a str>,
    pub ood: Vec<&'a str>,
    pub orphans: Vec<&'a str>,
}

impl<'a> Warnings<'a> {
    pub fn missing(&self, color: Colors, cols: Option<usize>) -> &Self {
        if !self.missing.is_empty() {
            let b = color.bold;
            let e = color.error;
            let len = ":: packages not in the AUR: ".len();
            print!("{} {}", e.paint("::"), b.paint("Packages not in the AUR: "));
            print_indent(Style::new(), len, 4, cols, "  ", &self.missing);
        }
        self
    }

    pub fn ood(&self, color: Colors, cols: Option<usize>) -> &Self {
        if !self.ood.is_empty() {
            let b = color.bold;
            let e = color.error;
            let len = ":: marked out of date: ".len();
            print!("{} {}", e.paint("::"), b.paint("Marked out of date: "));
            print_indent(Style::new(), len, 4, cols, "  ", &self.ood);
        }
        self
    }

    pub fn orphans(&self, color: Colors, cols: Option<usize>) -> &Self {
        if !self.orphans.is_empty() {
            let b = color.bold;
            let e = color.error;
            let len = ":: orphans: ".len();
            print!("{} {}", e.paint("::"), b.paint("Orphans: "));
            print_indent(Style::new(), len, 4, cols, "  ", &self.orphans);
        }
        self
    }

    pub fn all(&self, color: Colors, cols: Option<usize>) {
        self.missing(color, cols);
        self.ood(color, cols);
        self.orphans(color, cols);
    }
}

pub fn cache_info_with_warnings<'a, S: AsRef<str>>(
    raur: &raur::Handle,
    cache: &'a mut raur_ext::Cache,
    pkgs: &'a [S],
    ignore: &[String],
) -> StdResult<Warnings<'a>, raur::Error> {
    let mut missing = Vec::new();
    let mut ood = Vec::new();
    let mut orphaned = Vec::new();
    let aur_pkgs = raur.cache_info(cache, pkgs)?;

    for pkg in pkgs {
        if !ignore.iter().any(|p| p == pkg.as_ref()) && !cache.contains(pkg.as_ref()) {
            missing.push(pkg.as_ref())
        }
    }

    for pkg in &aur_pkgs {
        if !ignore.iter().any(|p| p.as_str() == pkg.name) {
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

pub fn getpkgbuilds(config: &mut Config) -> Result<i32> {
    let pkgs = config
        .targets
        .iter()
        .map(|t| t.as_str())
        .collect::<Vec<_>>();

    let (repo, aur) = split_repo_aur_targets(config, &pkgs);
    let mut ret = 0;

    if !repo.is_empty() {
        ret = repo_pkgbuilds(config, &repo)?;
    }

    if !aur.is_empty() {
        let aur = aur.iter().map(|t| t.pkg).collect::<Vec<_>>();
        let action = config.color.action;
        let bold = config.color.bold;
        println!("{} {}", action.paint("::"), bold.paint("Querying AUR..."));
        let warnings =
            cache_info_with_warnings(&config.raur, &mut config.cache, &aur, &config.ignore)?;
        if !warnings.missing.is_empty() {
            ret |= ret
        }
        warnings.missing(config.color, config.cols);
        let aur = warnings.pkgs;

        if !aur.is_empty() {
            let mut bases = Bases::new();
            bases.extend(aur);

            config.fetch.clone_dir = std::env::current_dir()?;

            aur_pkgbuilds(config, &bases)?;
        }
    }
    Ok(ret)
}

fn repo_pkgbuilds<'a>(config: &Config, pkgs: &[Targ<'a>]) -> Result<i32> {
    let db = config.alpm.syncdbs();

    let cd = std::env::current_dir().context("could not get current directory")?;
    let asp = &config.asp_bin;

    if Command::new(asp).output().is_err() {
        eprintln!("{} is not installed: can not get repo packages", asp);
        return Ok(1);
    }

    let cd = read_dir(cd)?
        .map(|d| d.map(|d| d.file_name().into_string().unwrap()))
        .collect::<Result<HashSet<_>, _>>()?;

    let mut ok = Vec::new();
    let mut missing = Vec::new();

    for &pkg in pkgs {
        let pkg = pkg.pkg;
        if db.pkg(pkg).is_err() {
            missing.push(pkg);
        } else {
            ok.push(pkg);
        }
    }

    if !missing.is_empty() {
        let len = ":: Missing ABS packages ".len();
        print!("{} Missing ABS packages ", config.color.error.paint("::"));
        print_indent(config.color.base, len, 3, config.cols, "  ", &missing);
    }

    for (n, pkg) in ok.into_iter().enumerate() {
        print_download(config, n + 1, pkgs.len(), pkg);
        let action = if cd.contains(pkg) { "update" } else { "export" };

        let ret = Command::new(asp)
            .arg(action)
            .arg(pkg)
            .output()
            .with_context(|| format!("failed to run: {} {} {}", asp, action, pkg))?;

        ensure!(
            ret.status.success(),
            "{}",
            String::from_utf8_lossy(&ret.stderr).trim()
        );
    }

    Ok(!missing.is_empty() as i32)
}

pub fn print_download(_config: &Config, n: usize, total: usize, pkg: &str) {
    let total = total.to_string();
    println!(
        " ({:>padding$}/{}) {}: {}",
        n,
        total,
        //config.color.action.paint("::"),
        "downloading",
        pkg,
        padding = total.len(),
    );
}

pub fn aur_pkgbuilds(config: &Config, bases: &Bases) -> Result<()> {
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
        bold.paint("Downloading PKGBUILDs...")
    );

    if bases.bases.is_empty() {
        println!(" PKGBUILDs up to date");
        return Ok(());
    }

    let fetched = if cols < 80 {
        config.fetch.download_cb(&download, |cb| {
            let base = bases
                .bases
                .iter()
                .find(|b| b.package_base() == cb.pkg)
                .unwrap();

            print_download(config, cb.n, download.len(), &base.to_string());
        })?
    } else {
        let total = download.len().to_string();
        let truncate = cols - (80 - (total.len() * 2)).min(cols);
        let template = format!(
            " ({{pos:>{}}}/{{len}}) {{prefix:!}} [{{wide_bar}}]",
            total.len()
        );
        let pb = ProgressBar::new(download.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template(&template)
                .progress_chars("-> "),
        );

        let mut prefix = format!("{:<100}", "");
        prefix.truncate(truncate);
        pb.set_prefix(&prefix);

        let fetched = config.fetch.download_cb(&download, |cb| {
            let base = bases
                .bases
                .iter()
                .find(|b| b.package_base() == cb.pkg)
                .unwrap();

            pb.inc(1);
            let mut prefix = format!("{}{:<100}", base, "");
            prefix.truncate(truncate);
            pb.set_prefix(&prefix);
        })?;

        pb.finish();
        fetched
    };

    config.fetch.merge(&fetched)?;

    Ok(())
}

pub fn new_aur_pkgbuilds(
    config: &Config,
    bases: &Bases,
    srcinfos: &HashMap<String, Srcinfo>,
) -> Result<()> {
    let mut pkgs = Vec::new();
    if config.redownload == "all" {
        return aur_pkgbuilds(config, bases);
    }

    for base in &bases.bases {
        if let Some(pkg) = srcinfos.get(base.package_base()) {
            let upstream_ver = pkg.version();
            if Version::new(base.version()) > Version::new(&*upstream_ver) {
                pkgs.push(base.clone());
            }
        } else {
            pkgs.push(base.clone());
        }
    }

    let bases = Bases { bases: pkgs };
    aur_pkgbuilds(config, &bases)
}

pub fn show_pkgbuilds(config: &mut Config) -> Result<i32> {
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    let bat = config.color.enabled && Command::new("bat").arg("-V").output().is_ok();

    let (repo, aur) = split_repo_aur_targets(config, &config.targets);

    if !repo.is_empty() {
        let asp = &config.asp_bin;

        if Command::new(asp).output().is_err() {
            eprintln!("{} is not installed: can not get repo packages", asp);
            return Ok(1);
        }

        for pkg in repo {
            let ret = Command::new(asp)
                .arg("update")
                .arg(&pkg.pkg)
                .output()
                .with_context(|| format!("failed to run: {} update {}", asp, pkg))?;

            ensure!(
                ret.status.success(),
                "{}",
                String::from_utf8_lossy(&ret.stderr).trim()
            );

            if bat {
                let output = Command::new(asp)
                    .arg("show")
                    .arg(&pkg.pkg)
                    .output()
                    .with_context(|| format!("failed to run: {} show {}", asp, pkg))?;

                ensure!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );

                pipe_bat(&output.stdout)?;
            } else {
                let ret = Command::new(asp)
                    .arg("show")
                    .arg(&pkg.pkg)
                    .spawn()?
                    .wait()
                    .with_context(|| format!("failed to run: {} show {}", asp, pkg))?;

                ensure!(ret.success(), "asp returned {}", ret.code().unwrap_or(1));
            }
            let _ = stdout.write_all(b"\n");
        }
    }

    if !aur.is_empty() {
        let client = reqwest::blocking::Client::new();
        let aur = aur.iter().map(|t| t.pkg).collect::<Vec<_>>();

        let warnings = cache_info_with_warnings(&config.raur, &mut config.cache, &aur, &[])?;
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
                .with_context(|| format!("{}: {}", base, url))?;
            if !response.status().is_success() {
                bail!("{}: {}: {}", base, url, response.status());
            }

            if bat {
                pipe_bat(&response.bytes()?)?;
            } else {
                let _ = stdout.write_all(&response.bytes()?);
            }

            let _ = stdout.write_all(b"\n");
        }

        return Ok(ret);
    }

    Ok(0)
}

fn pipe_bat(pkgbuild: &[u8]) -> Result<()> {
    let mut command = Command::new("bat")
        .arg("-pp")
        .arg("--color=always")
        .arg("-lPKGBUILD")
        .stdin(Stdio::piped())
        .spawn()?;

    let _ = command.stdin.as_mut().unwrap().write_all(pkgbuild);
    command.wait()?;
    Ok(())
}
