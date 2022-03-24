use crate::config::{Colors, Config};
use crate::download::cache_info_with_warnings;
use crate::exec;
use crate::fmt::{date, opt, print_indent};
use crate::util::split_repo_aur_info;

use alpm_utils::Targ;
use ansi_term::Style;
use anyhow::Error;
use raur::ArcPackage as Package;
use term_size::dimensions_stdout;
use tr::tr;
use unicode_width::UnicodeWidthStr;

pub async fn info(conf: &mut Config, verbose: bool) -> Result<i32, Error> {
    let targets = conf.targets.clone();
    let targets = targets.iter().map(Targ::from).collect::<Vec<_>>();

    let (repo, aur) = split_repo_aur_info(conf, &targets)?;
    let mut ret = 0;

    let aur = if !aur.is_empty() {
        let color = conf.color;
        let aur = aur.iter().map(|t| t.pkg).collect::<Vec<_>>();
        let warnings =
            cache_info_with_warnings(&conf.raur, &mut conf.cache, &aur, &conf.ignore).await?;
        for pkg in &warnings.missing {
            eprintln!(
                "{} {}",
                color.error.paint("error:"),
                tr!("package '{}' was not found", pkg,),
            );
        }
        ret = !warnings.missing.is_empty() as i32;
        warnings.pkgs
    } else {
        Vec::new()
    };

    if !repo.is_empty() {
        let targets = repo.into_iter().map(|t| t.to_string()).collect::<Vec<_>>();
        let mut args = conf.pacman_args();
        args.targets.clear();
        args.targets(targets.iter().map(|t| t.as_str()));
        ret |= exec::pacman(conf, &args)?.code();
    }

    if !aur.is_empty() {
        print_aur_info(conf, verbose, &aur)?;
    }

    Ok(ret)
}

fn longest() -> usize {
    [
        tr!("Repository"),
        tr!("Name"),
        tr!("Version"),
        tr!("Description"),
        tr!("Groups"),
        tr!("Licenses"),
        tr!("Provides"),
        tr!("Depends On"),
        tr!("Make Deps"),
        tr!("Check Deps"),
        tr!("Optional Deps"),
        tr!("Conflicts With"),
        tr!("Maintainer"),
        tr!("Votes"),
        tr!("Popularity"),
        tr!("First Submitted"),
        tr!("Last Modified"),
        tr!("Out Of Date"),
        tr!("ID"),
        tr!("Package Base ID"),
        tr!("Keywords"),
        tr!("Snapshot URL"),
        "URL".to_string(),
        "AUR URL".to_string(),
    ]
    .iter()
    .map(|s| s.width())
    .max()
    .unwrap()
}

pub fn print_aur_info(conf: &mut Config, verbose: bool, pkgs: &[Package]) -> Result<(), Error> {
    let len = longest() + 3;
    let color = conf.color;
    let cols = dimensions_stdout().map(|x| x.0);
    let print = |k: &str, v: &str| print(color, len, cols, k, v);
    let print_list = |k: &str, v: &[_]| print_list(color, len, cols, k, v);
    let no = tr!("No");

    for pkg in pkgs {
        print(&tr!("Repository"), "aur");
        print(&tr!("Name"), &pkg.name);
        print(&tr!("Version"), &pkg.version);
        print(&tr!("Description"), &opt(&pkg.description));
        print("URL", &opt(&pkg.url));
        print(
            "AUR URL",
            conf.aur_url
                .join(&format!("packages/{}", pkg.package_base))?
                .as_str(),
        );
        print_list(&tr!("Groups"), &pkg.groups);
        print_list(&tr!("Licenses"), &pkg.license);
        print_list(&tr!("Provides"), &pkg.provides);
        print_list(&tr!("Depends On"), &pkg.depends);
        print_list(&tr!("Make Deps"), &pkg.make_depends);
        print_list(&tr!("Check Deps"), &pkg.check_depends);
        print_list(&tr!("Optional Deps"), &pkg.opt_depends);
        print_list(&tr!("Conflicts With"), &pkg.conflicts);
        print(&tr!("Maintainer"), &opt(&pkg.maintainer));
        print(&tr!("Votes"), &pkg.num_votes.to_string());
        print(&tr!("Popularity"), &pkg.popularity.to_string());
        print(&tr!("First Submitted"), &date(pkg.first_submitted));
        print(&tr!("Last Modified"), &date(pkg.last_modified));
        print(
            &tr!("Out Of Date"),
            pkg.out_of_date.map(date).as_deref().unwrap_or(no.as_str()),
        );

        if verbose {
            print(&tr!("ID"), &pkg.id.to_string());
            print(&tr!("Package Base ID"), &pkg.package_base_id.to_string());
            print_list(&tr!("Keywords"), &pkg.keywords);
            print(
                &tr!("Snapshot URL"),
                conf.aur_url.join(&pkg.url_path)?.as_str(),
            );
        }

        println!();
    }

    Ok(())
}

pub fn print(color: Colors, indent: usize, cols: Option<usize>, k: &str, v: &str) {
    print_info(color, false, indent, cols, k, v.split_whitespace());
}

fn print_list(color: Colors, indent: usize, cols: Option<usize>, k: &str, v: &[String]) {
    if v.is_empty() {
        print(color, indent, cols, k, &tr!("None"));
    } else {
        print_info(color, true, indent, cols, k, v.iter().map(|s| s.as_str()));
    }
}

fn print_info<'a>(
    color: Colors,
    list: bool,
    indent: usize,
    cols: Option<usize>,
    key: &str,
    value: impl IntoIterator<Item = &'a str>,
) {
    let mut prefix = key.to_string();
    for _ in 0..indent - prefix.width() - 2 {
        prefix.push(' ');
    }
    prefix.push_str(": ");
    print!("{}", color.field.paint(&prefix));

    let sep = if list { "  " } else { " " };
    print_indent(Style::new(), indent, indent, cols, sep, value)
}
