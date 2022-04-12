use crate::config::Config;
use crate::repo;

use alpm::Ver;
use aur_depends::Actions;

use ansi_term::Style;
use chrono::{DateTime, NaiveDateTime};
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
    let date = NaiveDateTime::from_timestamp(date, 0);
    let date = DateTime::<chrono::Utc>::from_utc(date, chrono::Utc);
    date.to_rfc2822()
}

pub fn ymd(date: i64) -> String {
    let date = NaiveDateTime::from_timestamp(date, 0);
    let date = DateTime::<chrono::Utc>::from_utc(date, chrono::Utc);
    date.format("%Y-%m-%d").to_string()
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
                pos += word.as_ref().len();
            }

            if iter.peek().is_some() && pos + sep.len() < cols {
                print!("{}", sep);
                pos += sep.len();
            }

            while let Some(word) = iter.next() {
                let word = word.as_ref();

                if pos + word.len() > cols {
                    print!("\n{:>padding$}", "", padding = indent);
                    pos = indent;
                }

                print!("{}", color.paint(word));
                pos += word.len();

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
        col = (b as u32).wrapping_add(((col as u32) << 4).wrapping_add(col as u32));
    }

    col = (col % 6) + 9;
    let col = Style::from(Color::Fixed(col as u8)).bold();
    col.paint(name).to_string()
}

fn to_install(actions: &Actions) -> ToInstall {
    let install = actions
        .install
        .iter()
        .filter(|p| !p.make)
        .map(|p| format!("{}-{}", p.pkg.name(), p.pkg.version()))
        .collect::<Vec<_>>();
    let make_install = actions
        .install
        .iter()
        .filter(|p| p.make)
        .map(|p| format!("{}-{}", p.pkg.name(), p.pkg.version()))
        .collect::<Vec<_>>();

    let mut build = actions.build.clone();
    for base in &mut build {
        base.pkgs.retain(|p| !p.make);
    }
    build.retain(|b| !b.pkgs.is_empty());
    let build = build.iter().map(|p| p.to_string()).collect::<Vec<_>>();

    let mut make_build = actions.build.clone();
    for base in &mut make_build {
        base.pkgs.retain(|p| p.make);
    }
    make_build.retain(|b| !b.pkgs.is_empty());
    let make_build = make_build.iter().map(|p| p.to_string()).collect::<Vec<_>>();

    ToInstall {
        install,
        make_install,
        aur: build,
        make_aur: make_build,
    }
}

pub fn print_install(config: &Config, actions: &Actions) {
    let c = config.color;

    println!();

    let to = to_install(actions);

    if !to.install.is_empty() {
        let fmt = format!("{} ({}) ", tr!("Repo"), to.install.len());
        let start = 17 + to.install.len().to_string().len();
        print!("{}", c.bold.paint(fmt));
        print_indent(Style::new(), start, 4, config.cols, "  ", to.install);
    }

    if !to.make_install.is_empty() {
        let fmt = format!("{} ({}) ", tr!("Repo Make"), to.make_install.len());
        let start = 22 + to.make_install.len().to_string().len();
        print!("{}", c.bold.paint(fmt));
        print_indent(Style::new(), start, 4, config.cols, "  ", to.make_install);
    }

    if !to.aur.is_empty() {
        let fmt = format!("{} ({}) ", "Aur", to.aur.len());
        let start = 16 + to.aur.len().to_string().len();
        print!("{}", c.bold.paint(fmt));
        print_indent(Style::new(), start, 4, config.cols, "  ", to.aur);
    }

    if !to.make_aur.is_empty() {
        let fmt = format!("{} ({}) ", tr!("Aur Make"), to.make_aur.len());
        let start = 16 + to.make_aur.len().to_string().len();
        print!("{}", c.bold.paint(fmt));
        print_indent(Style::new(), start, 4, config.cols, "  ", to.make_aur);
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

pub fn print_install_verbose(config: &Config, actions: &Actions) {
    let c = config.color;
    let bold = c.bold;
    let db = config.alpm.localdb();

    let package = tr!("Repo ({})", actions.install.len());
    let aur = tr!("Aur ({})", actions.iter_build_pkgs().count());
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

    let make_len = yes.width().max(no.width()).max(make.width());

    let aur_len = actions
        .iter_build_pkgs()
        .map(|pkg| repo(config, &pkg.pkg.name).len() + 1 + pkg.pkg.name.len())
        .chain(Some(aur.width()))
        .max()
        .unwrap_or_default();

    let aur_old_len = actions
        .iter_build_pkgs()
        .filter_map(|pkg| old_ver(config, &pkg.pkg.name))
        .map(|v| v.len())
        .chain(Some(old.width()))
        .max()
        .unwrap_or_default();

    let aur_new_len = actions
        .iter_build_pkgs()
        .map(|pkg| pkg.pkg.version.len())
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

            print_install(config, actions);
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

        for pkg in actions.iter_build_pkgs() {
            println!(
                "{:<package_len$}  {:<old_len$}  {:<new_len$}  {}",
                format!("{}/{}", repo(config, &pkg.pkg.name), pkg.pkg.name),
                old_ver(config, &pkg.pkg.name)
                    .map(|v| v.as_str())
                    .unwrap_or_default(),
                pkg.pkg.version,
                if pkg.make { &yes } else { &no }
            );
        }
    }

    println!();
}
