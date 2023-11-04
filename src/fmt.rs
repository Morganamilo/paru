use std::fmt::Write;

use std::collections::HashSet;

use crate::config::Config;
use crate::repo;

use alpm::Ver;
use aur_depends::{Actions, Base};

use ansi_term::Style;
use chrono::{Local, TimeZone, Utc};
use tr::tr;
use unicode_width::UnicodeWidthStr;

struct ToInstall {
    install: Vec<String>,
    make_install: Vec<String>,
    aur: Vec<String>,
    make_aur: Vec<String>,
}

pub fn opt(opt: &Option<String>) -> String {
    opt.clone().unwrap_or_else(|| tr!("None"))
}

pub fn date(date: i64) -> String {
    let date = Utc.timestamp_opt(date, 0).unwrap().with_timezone(&Local);
    date.format("%a, %e %b %Y %T").to_string()
}

pub fn ymd(date: i64) -> String {
    let date = Utc.timestamp_opt(date, 0).unwrap().with_timezone(&Local);
    date.format("%Y-%m-%d").to_string()
}

fn word_len(s: &str) -> usize {
    let mut len = 0;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.by_ref().take_while(|c| c != &'m').count();
        } else {
            len += 1;
        }
    }

    len
}

pub fn print_indent<S: AsRef<str>>(
    color: Style,
    start: usize,
    indent: usize,
    cols: Option<usize>,
    sep: &str,
    value: impl IntoIterator<Item = S>,
) {
    let v = value.into_iter();

    match cols {
        Some(cols) if cols > indent + 2 => {
            let mut pos = start;

            let mut iter = v.peekable();

            if let Some(word) = iter.next() {
                print!("{}", color.paint(word.as_ref()));
                pos += word_len(word.as_ref());
            }

            if iter.peek().is_some() && pos + sep.len() < cols {
                print!("{}", sep);
                pos += sep.len();
            }

            while let Some(word) = iter.next() {
                let word = word.as_ref();
                let len = word_len(word);

                if pos + len > cols {
                    print!("\n{:>padding$}", "", padding = indent);
                    pos = indent;
                }

                print!("{}", color.paint(word));
                pos += len;

                if iter.peek().is_some() && pos + sep.len() < cols {
                    print!("{}", sep);
                    pos += sep.len();
                }
            }
        }
        _ => {
            let mut iter = v;
            if let Some(word) = iter.next() {
                print!("{}", color.paint(word.as_ref()));
            }

            for word in iter {
                print!("{}{}", sep, color.paint(word.as_ref()));
            }
        }
    }
    println!();
}

use ansi_term::Color;

pub fn color_repo(enabled: bool, name: &str) -> String {
    if !enabled {
        return name.to_string();
    }

    let mut col: u32 = 5;

    for &b in name.as_bytes() {
        col = (b as u32).wrapping_add((col << 4).wrapping_add(col));
    }

    col = (col % 6) + 9;
    let col = Style::from(Color::Fixed(col as u8)).bold();
    col.paint(name).to_string()
}

pub fn print_target(targ: &str, quiet: bool) {
    if quiet {
        println!("{}", targ.split_once('/').unwrap().1);
    } else {
        println!("{}", targ);
    }
}

fn base_string(config: &Config, base: &Base, devel: &HashSet<String>) -> String {
    let c = config.color;
    let mut s = String::new();
    write!(
        &mut s,
        "{}{}",
        base.package_base(),
        c.install_version.paint("-"),
    )
    .unwrap();
    if base.packages().any(|p| devel.contains(p)) {
        write!(&mut s, "{}", c.install_version.paint("latest-commit")).unwrap();
    } else {
        write!(&mut s, "{}", c.install_version.paint(base.version())).unwrap();
    }

    if !Base::base_is_pkg(base.package_base(), base.packages()) {
        write!(&mut s, " (").unwrap();
        let mut pkgs = base.packages();
        write!(&mut s, "{}", pkgs.next().unwrap()).unwrap();
        for pkg in pkgs {
            write!(&mut s, " {}", pkg).unwrap();
        }
        write!(&mut s, ")").unwrap();
    }
    s
}

fn to_install(config: &Config, actions: &Actions, devel: &HashSet<String>) -> ToInstall {
    let c = config.color;
    let dash = c.install_version.paint("-");

    let install = actions
        .install
        .iter()
        .filter(|p| !p.make)
        .map(|p| {
            format!(
                "{}{}{}",
                p.pkg.name(),
                dash,
                c.install_version.paint(p.pkg.version().to_string())
            )
        })
        .collect::<Vec<_>>();
    let make_install = actions
        .install
        .iter()
        .filter(|p| p.make)
        .map(|p| {
            format!(
                "{}{}{}",
                p.pkg.name(),
                dash,
                c.install_version.paint(p.pkg.version().to_string())
            )
        })
        .collect::<Vec<_>>();

    let mut build = actions.build.clone();
    for base in &mut build {
        match base {
            Base::Aur(base) => base.pkgs.retain(|p| !p.make),
            Base::Pkgbuild(base) => base.pkgs.retain(|p| !p.make),
        }
    }
    build.retain(|b| b.package_count() != 0);
    let build = build
        .iter()
        .map(|p| base_string(config, p, devel))
        .collect::<Vec<_>>();

    let mut make_build = actions.build.clone();
    for base in &mut make_build {
        match base {
            Base::Aur(base) => base.pkgs.retain(|p| p.make),
            Base::Pkgbuild(base) => base.pkgs.retain(|p| p.make),
        }
    }
    make_build.retain(|b| b.package_count() != 0);
    let make_build = make_build
        .iter()
        .map(|p| base_string(config, p, devel))
        .collect::<Vec<_>>();

    ToInstall {
        install,
        make_install,
        aur: build,
        make_aur: make_build,
    }
}

pub fn print_install(config: &Config, actions: &Actions, devel: &HashSet<String>) {
    let c = config.color;

    println!();

    let to = to_install(config, actions, devel);

    if !to.install.is_empty() {
        let fmt = format!("{} ({}) ", tr!("Repo"), to.install.len());
        let start = 17 + to.install.len().to_string().len();
        print!("{}", c.bold.paint(fmt));
        print_indent(Style::new(), start, 8, config.cols, "  ", to.install);
    }

    if !to.make_install.is_empty() {
        let fmt = format!("{} ({}) ", tr!("Repo Make"), to.make_install.len());
        let start = 22 + to.make_install.len().to_string().len();
        print!("{}", c.bold.paint(fmt));
        print_indent(Style::new(), start, 8, config.cols, "  ", to.make_install);
    }

    if !to.aur.is_empty() {
        let aur = if actions.iter_pkgbuilds().next().is_some() {
            "Pkgbuilds"
        } else {
            "Aur"
        };
        let fmt = format!("{} ({}) ", aur, to.aur.len());
        let start = 16 + to.aur.len().to_string().len();
        print!("{}", c.bold.paint(fmt));
        print_indent(Style::new(), start, 8, config.cols, "  ", to.aur);
    }

    if !to.make_aur.is_empty() {
        let aur = if actions.iter_pkgbuilds().next().is_some() {
            tr!("Pkgbuilds Make")
        } else {
            tr!("Aur Make")
        };

        let fmt = format!("{} ({}) ", aur, to.make_aur.len());
        let start = 16 + to.make_aur.len().to_string().len();
        print!("{}", c.bold.paint(fmt));
        print_indent(Style::new(), start, 8, config.cols, "  ", to.make_aur);
    }

    println!();
}

fn repo<'a>(config: &'a Config, pkg: &str) -> &'a str {
    let (_, dbs) = repo::repo_aur_dbs(config);

    if dbs.is_empty() {
        return "aur";
    }

    let db = dbs
        .iter()
        .find(|db| db.pkg(pkg).is_ok())
        .map(|db| db.name())
        .unwrap_or_else(|| dbs.first().unwrap().name());

    db
}

fn old_ver<'a>(config: &'a Config, pkg: &str) -> Option<&'a Ver> {
    let (_, dbs) = repo::repo_aur_dbs(config);

    if dbs.is_empty() {
        return config.alpm.localdb().pkg(pkg).ok().map(|p| p.version());
    }

    dbs.iter()
        .find_map(|db| db.pkg(pkg).ok())
        .map(|p| p.version())
}

pub fn print_install_verbose(config: &Config, actions: &Actions, devel: &HashSet<String>) {
    let c = config.color;
    let bold = c.bold;
    let db = config.alpm.localdb();

    let package = tr!("Repo ({})", actions.install.len());
    let aur = match (
        actions.iter_aur_pkgs().count(),
        actions.iter_pkgbuilds().count(),
    ) {
        (a, 0) => tr!("Aur ({})", a),
        (a, c) => tr!("Pkgbuilds ({})", a + c),
    };
    let old = tr!("Old Version");
    let new = tr!("New Version");
    let make = tr!("Make Only");
    let yes = tr!("Yes");
    let no = tr!("No");

    let package_len = actions
        .install
        .iter()
        .map(|pkg| pkg.pkg.db().unwrap().name().len() + 1 + pkg.pkg.name().len())
        .chain(Some(package.width()))
        .max()
        .unwrap_or_default();

    let old_len = actions
        .install
        .iter()
        .filter_map(|pkg| db.pkg(pkg.pkg.name()).ok())
        .map(|pkg| pkg.version().len())
        .chain(Some(old.width()))
        .max()
        .unwrap_or_default();

    let new_len = actions
        .install
        .iter()
        .map(|pkg| pkg.pkg.version().len())
        .chain(Some(new.width()))
        .max()
        .unwrap_or_default();
    let new_len = new_len.max("latest-commit".len());

    let make_len = yes.width().max(no.width()).max(make.width());

    let aur_len = actions
        .build
        .iter()
        .filter_map(|pkg| match pkg {
            Base::Aur(base) => base
                .pkgs
                .iter()
                .map(|pkg| repo(config, &pkg.pkg.name).len() + 1 + pkg.pkg.name.len())
                .max(),
            Base::Pkgbuild(base) => base
                .pkgs
                .iter()
                .map(|pkg| base.repo.len() + 1 + pkg.pkg.pkgname.len())
                .max(),
        })
        .chain(Some(aur.width()))
        .max()
        .unwrap_or_default();

    let aur_old_len = actions
        .build
        .iter()
        .filter_map(|pkg| match pkg {
            Base::Aur(base) => base
                .pkgs
                .iter()
                .filter_map(|pkg| old_ver(config, &pkg.pkg.name))
                .map(|v| v.as_str())
                .max(),
            Base::Pkgbuild(base) => base
                .pkgs
                .iter()
                .filter_map(|pkg| old_ver(config, &pkg.pkg.pkgname))
                .map(|v| v.as_str())
                .max(),
        })
        .map(|v| v.len())
        .chain(Some(old.width()))
        .max()
        .unwrap_or_default();

    let aur_new_len = actions
        .build
        .iter()
        .map(|base| base.version().len())
        .chain(Some(new.width()))
        .max()
        .unwrap_or_default();

    let package_len = package_len.max(aur_len);
    let old_len = old_len.max(aur_old_len);
    let new_len = new_len.max(aur_new_len);

    if let Some(cols) = config.cols {
        if package_len + 2 + old_len + 2 + new_len + 2 + make_len > cols {
            eprintln!(
                "{} {}",
                c.warning.paint("::"),
                tr!("insufficient columns available for table display")
            );

            print_install(config, actions, devel);
            return;
        }
    }

    if !actions.install.is_empty() {
        println!();
        println!(
            "{}{:<package_len$}  {}{:<old_len$}  {}{:<new_len$}  {}",
            bold.paint(&package),
            "",
            bold.paint(&old),
            "",
            bold.paint(&new),
            "",
            bold.paint(&make),
            package_len = package_len - package.width(),
            old_len = old_len - old.width(),
            new_len = new_len - new.width(),
        );

        let mut install = actions.install.clone();
        install.sort_by(|a, b| {
            a.pkg
                .db()
                .unwrap()
                .name()
                .cmp(b.pkg.db().unwrap().name())
                .then(a.pkg.name().cmp(b.pkg.name()))
        });

        for pkg in &install {
            println!(
                "{:<package_len$}  {:<old_len$}  {:<new_len$}  {}",
                format!("{}/{}", pkg.pkg.db().unwrap().name(), pkg.pkg.name()),
                db.pkg(pkg.pkg.name())
                    .map(|pkg| pkg.version().as_str())
                    .unwrap_or(""),
                pkg.pkg.version().as_str(),
                if pkg.make { &yes } else { &no }
            );
        }
    }

    if !actions.build.is_empty() {
        println!();
        println!(
            "{}{:<package_len$}  {}{:<old_len$}  {}{:<new_len$}  {}",
            bold.paint(&aur),
            "",
            bold.paint(&old),
            "",
            bold.paint(&new),
            "",
            bold.paint(&make),
            package_len = package_len - aur.width(),
            old_len = old_len - old.width(),
            new_len = new_len - new.width(),
        );

        for pkg in actions.build.iter() {
            match pkg {
                Base::Aur(base) => {
                    for pkg in &base.pkgs {
                        let ver = if devel.contains(&pkg.pkg.name) {
                            "latest-commit"
                        } else {
                            &pkg.pkg.version
                        };
                        println!(
                            "{:<package_len$}  {:<old_len$}  {:<new_len$}  {}",
                            format!("{}/{}", repo(config, &pkg.pkg.name), pkg.pkg.name),
                            old_ver(config, &pkg.pkg.name)
                                .map(|v| v.as_str())
                                .unwrap_or_default(),
                            ver,
                            if pkg.make { &yes } else { &no }
                        );
                    }
                }
                Base::Pkgbuild(base) => {
                    for pkg in &base.pkgs {
                        let ver = base.srcinfo.version();
                        let ver = if devel.contains(&pkg.pkg.pkgname) {
                            "latest-commit"
                        } else {
                            &ver
                        };
                        println!(
                            "{:<package_len$}  {:<old_len$}  {:<new_len$}  {}",
                            format!("{}/{}", base.repo, pkg.pkg.pkgname),
                            old_ver(config, &pkg.pkg.pkgname)
                                .map(|v| v.as_str())
                                .unwrap_or_default(),
                            ver,
                            if pkg.make { &yes } else { &no }
                        );
                    }
                }
            }
        }
    }

    println!();
}
