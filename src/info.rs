use crate::config::{Colors, Config};
use crate::download::cache_info_with_warnings;
use crate::exec;
use crate::fmt::{date, opt, print_indent};
use crate::util::split_repo_aur_targets;

use alpm_utils::Targ;
use ansi_term::Style;
use anyhow::Error;
use raur_ext::Package;
use term_size::dimensions_stdout;

pub fn info(conf: &mut Config, verbose: bool) -> Result<i32, Error> {
    let targets = conf.targets.clone();
    let targets = targets.iter().map(Targ::from).collect::<Vec<_>>();

    let (repo, aur) = split_repo_aur_targets(conf, &targets);
    let mut ret = 0;

    let aur = if !aur.is_empty() {
        let color = conf.color;
        let aur = aur.iter().map(|t| t.pkg).collect::<Vec<_>>();
        let warnings = cache_info_with_warnings(&conf.raur, &mut conf.cache, &aur, &conf.ignore)?;
        for pkg in &warnings.missing {
            eprintln!(
                "{} package '{}' was not found",
                color.error.paint("error:"),
                pkg,
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

pub fn print_aur_info(conf: &mut Config, verbose: bool, pkgs: &[Package]) -> Result<(), Error> {
    let color = conf.color;
    let cols = dimensions_stdout().map(|x| x.0);
    let print = |k: &str, v: &str| print(color, 18, cols, k, v);
    let print_list = |k: &str, v: &[_]| print_list(color, 18, cols, k, v);

    for pkg in pkgs {
        print("Repository", "aur");
        print("Name", &pkg.name);
        print("Version", &pkg.version);
        print("Description", opt(&pkg.description));
        print("URL", opt(&pkg.url));
        print(
            "AUR URL",
            conf.aur_url
                .join(&format!("packages/{}", pkg.package_base))?
                .as_str(),
        );
        print_list("Groups", &pkg.groups);
        print_list("Licenses", &pkg.license);
        print_list("Provides", &pkg.provides);
        print_list("Depends On", &pkg.depends);
        print_list("Make Deps", &pkg.make_depends);
        print_list("Check Deps", &pkg.check_depends);
        print_list("Optional Deps", &pkg.opt_depends);
        print_list("Conflicts With", &pkg.conflicts);
        print("Maintainer", opt(&pkg.maintainer));
        print("Votes", &pkg.num_votes.to_string());
        print("Popularity", &pkg.popularity.to_string());
        print("First Submitted", &date(pkg.first_submitted));
        print("Last Modified", &date(pkg.last_modified));
        print(
            "Out Of Date",
            pkg.out_of_date
                .map(date)
                .as_ref()
                .map(String::as_ref)
                .unwrap_or("No"),
        );

        if verbose {
            print("ID", &pkg.id.to_string());
            print("Package Base ID", &pkg.package_base_id.to_string());
            print_list("Keywords", &pkg.keywords);
            print("Snapshot URL", conf.aur_url.join(&pkg.url_path)?.as_str());
        }

        println!();
    }

    Ok(())
}

fn print(color: Colors, indent: usize, cols: Option<usize>, k: &str, v: &str) {
    print_info(color, false, indent, cols, k, v.split_whitespace());
}

fn print_list(color: Colors, indent: usize, cols: Option<usize>, k: &str, v: &[String]) {
    if v.is_empty() {
        print(color, indent, cols, k, "None");
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
    let prefix = format!("{:<padding$}: ", key, padding = indent - 2);
    print!("{}", color.field.paint(&prefix));

    let sep = if list { "  " } else { " " };
    print_indent(Style::new(), prefix.len(), prefix.len(), cols, sep, value)
}
