use crate::args::Args;
use crate::fmt::color_repo;
use crate::util::get_provider;
use crate::{sprint, sprintln};

use std::env::var;
use std::fs::File;
use std::io::{stdin, BufRead};
use std::path::PathBuf;

use alpm::{set_questioncb, Question, SigLevel, Usage};
use ansi_term::Color::{Blue, Cyan, Green, Purple, Red, Yellow};
use ansi_term::Style;
use anyhow::{anyhow, bail, ensure, Context, Error, Result};
use atty::Stream::Stdout;
use cini::{Callback, CallbackKind, Ini};
use nix::unistd::dup2;
use once_cell::sync::OnceCell;
use std::os::unix::io::AsRawFd;
use url::Url;

static COLORS: OnceCell<Colors> = OnceCell::new();
pub static NO_CONFIRM: OnceCell<bool> = OnceCell::new();

#[derive(Debug, Default)]
pub struct Alpm {
    alpm: Option<alpm::Alpm>,
}

impl std::ops::Deref for Alpm {
    type Target = alpm::Alpm;
    fn deref(&self) -> &Self::Target {
        self.alpm.as_ref().unwrap()
    }
}

impl Alpm {
    fn new(alpm: alpm::Alpm) -> Self {
        Self { alpm: Some(alpm) }
    }
}

#[derive(Debug, Default, Copy, Clone)]
pub struct Colors {
    pub enabled: bool,
    pub field: Style,
    pub error: Style,
    pub warning: Style,
    pub bold: Style,
    pub upgrade: Style,
    pub base: Style,
    pub action: Style,
    pub sl_repo: Style,
    pub sl_pkg: Style,
    pub sl_version: Style,
    pub sl_installed: Style,
    pub ss_repo: Style,
    pub ss_name: Style,
    pub ss_ver: Style,
    pub ss_stats: Style,
    pub ss_orphaned: Style,
    pub ss_installed: Style,
    pub code: Style,
    pub news_date: Style,
    pub old_version: Style,
    pub new_version: Style,
    pub number_menu: Style,
    pub group: Style,
}

impl From<&str> for Colors {
    fn from(s: &str) -> Self {
        match s {
            "auto" if atty::is(Stdout) => Colors::new(),
            "always" => Colors::new(),
            _ => Colors::default(),
        }
    }
}

impl Colors {
    pub fn new() -> Colors {
        Colors {
            enabled: true,
            field: Style::new().bold(),
            error: Style::new().fg(Red),
            warning: Style::new().fg(Yellow),
            bold: Style::new().bold(),
            upgrade: Style::new().fg(Green),
            base: Style::new().fg(Blue),
            action: Style::new().fg(Blue).bold(),
            sl_repo: Style::new().fg(Purple).bold(),
            sl_pkg: Style::new().bold(),
            sl_version: Style::new().fg(Green).bold(),
            sl_installed: Style::new().fg(Cyan).bold(),
            ss_repo: Style::new().fg(Blue).bold(),
            ss_name: Style::new().bold(),
            ss_ver: Style::new().fg(Green).bold(),
            ss_stats: Style::new().bold(),
            ss_orphaned: Style::new().fg(Red).bold(),
            ss_installed: Style::new().fg(Cyan).bold(),
            code: Style::new().fg(Cyan),
            news_date: Style::new().fg(Cyan).bold(),
            old_version: Style::new().fg(Red),
            new_version: Style::new().fg(Green),
            number_menu: Style::new().fg(Purple),
            group: Style::new().fg(Blue).bold(),
        }
    }
}

#[derive(SmartDefault, Debug)]
pub struct Config {
    section: Option<String>,
    pub args: Args<String>,
    pub globals: Args<String>,

    pub cols: Option<usize>,

    #[default = "yay"]
    pub op: String,
    pub raur: raur::Handle,
    #[default(aur_fetch::Handle::with_cache_dir(""))]
    pub fetch: aur_fetch::Handle,
    pub cache: raur_ext::Cache,
    pub need_root: bool,

    pub pacman: pacmanconf::Config,
    pub alpm: Alpm,
    pub color: Colors,
    pub targets: Vec<String>,

    #[default(Url::parse("https://aur.archlinux.org").unwrap())]
    pub aur_url: Url,
    #[default(Url::parse("https://archlinux.org").unwrap())]
    pub arch_url: Url,
    pub build_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub devel_path: PathBuf,
    pub config_path: Option<PathBuf>,

    pub news: u32,
    pub gendb: bool,

    #[default = "no"]
    pub redownload: String,
    #[default = "no"]
    pub rebuild: String,
    #[default = "no"]
    pub remove_make: String,
    #[default = "votes"]
    pub sort_by: String,
    #[default = "name-desc"]
    pub search_by: String,
    #[default = "topdown"]
    pub sort_mode: String,
    #[default = "any"]
    pub mode: String,

    #[default = 150]
    pub request_split: u64,
    #[default = 7]
    pub completion_interval: u64,

    pub help: bool,
    pub version: bool,

    pub no_confirm: bool,
    pub sudo_loop: bool,
    pub devel: bool,
    pub clean_after: bool,
    pub provides: bool,
    pub pgp_fetch: bool,
    pub combined_upgrade: bool,
    pub batch_install: bool,
    pub use_ask: bool,
    pub clean: usize,
    pub complete: bool,
    pub print: bool,

    #[default = "makepkg"]
    pub makepkg_bin: String,
    #[default = "pacman"]
    pub pacman_bin: String,
    #[default = "git"]
    pub git_bin: String,
    #[default = "gpg"]
    pub gpg_bin: String,
    #[default = "sudo"]
    pub sudo_bin: String,
    #[default = "asp"]
    pub asp_bin: String,
    pub fm: Option<String>,

    pub mflags: Vec<String>,
    pub git_flags: Vec<String>,
    pub gpg_flags: Vec<String>,
    pub sudo_flags: Vec<String>,
    pub fm_flags: Vec<String>,

    pub upgrade_menu: bool,
    pub answer_upgrade: Option<String>,

    pub makepkg_conf: Option<String>,
    pub pacman_conf: Option<String>,

    //pacman
    pub db_path: Option<String>,
    pub root: Option<String>,
    pub verbose: bool,
    pub ask: usize,
    pub arch: Option<String>,

    pub ignore: Vec<String>,
    pub ignore_group: Vec<String>,
}

impl Ini for Config {
    type Err = Error;

    fn callback(&mut self, cb: Callback) -> Result<(), Self::Err> {
        let err = match cb.kind {
            CallbackKind::Section(section) => self.parse_section(section),
            CallbackKind::Directive(_, key, value) => self.parse_directive(key, value),
        };

        let filename = cb.filename.unwrap_or("paru.conf");
        err.map_err(|e| anyhow!("{}:{}: {}", filename, cb.line_number, e))
    }
}

impl Config {
    pub fn new() -> Result<Self> {
        let cache = dirs::cache_dir().ok_or_else(|| anyhow!("failed to find cache directory"))?;
        let cache = cache.join("paru");
        let config =
            dirs::config_dir().ok_or_else(|| anyhow!("failed to find config directory"))?;
        let config = config.join("paru");

        let build_dir = cache.join("clone");
        let config_path = config.join("paru.conf");
        let devel_path = cache.join("devel.json");
        let cache_dir = cache;

        let color = Colors::from("never");
        let cols = term_size::dimensions_stdout().map(|v| v.0);

        let mut config = Self {
            devel_path,
            cols,
            cache_dir,
            color,
            build_dir,
            ..Self::default()
        };

        if config_path.exists() {
            config.config_path = Some(config_path);
        } else {
            let config_path = PathBuf::from("/etc/paru.conf");

            if config_path.exists() {
                config.config_path = Some(config_path);
            }
        }

        Ok(config)
    }

    pub fn pacman_args(&self) -> Args<&str> {
        self.args.as_str()
    }

    pub fn pacman_globals(&self) -> Args<&str> {
        self.globals.as_str()
    }

    pub fn parse_args<S: AsRef<str>, I: Iterator<Item = S>>(&mut self, iter: I) -> Result<()> {
        let mut iter = iter.peekable();
        let mut op_count = 0;
        let mut end_of_ops = false;

        if let Ok(aurdest) = var("AURDEST") {
            self.build_dir = aurdest.into();
        }

        while let Some(arg) = iter.next() {
            let value = iter.peek().map(|s| s.as_ref());
            let arg = arg.as_ref();
            if self.parse_arg(arg, value, &mut op_count, &mut end_of_ops)? {
                iter.next();
            }

            ensure!(op_count <= 1, "only one operation may be used at a time");
        }

        if let Some((i, _)) = self.targets.iter().enumerate().find(|t| t.1 == "-") {
            self.targets.remove(i);
            self.parse_stdin()?;
            reopen_stdin()?;
        }

        self.args.op = self.op.clone();
        self.args.targets = self.targets.clone();
        self.args.bin = self.pacman_bin.clone();

        self.globals.op = self.op.clone();
        self.globals.bin = self.pacman_bin.clone();

        if self.help {
            help();
            std::process::exit(0);
        }

        if self.version {
            version();
            std::process::exit(0);
        }

        self.init_pacmanconf()?;
        self.init_alpm()?;

        if self.pacman.color && !self.globals.has_arg("color", "color") {
            self.color = Colors::from("auto");
        }

        ensure!(COLORS.set(self.color).is_ok(), "failed to initalise colors");
        ensure!(
            NO_CONFIRM.set(self.no_confirm).is_ok(),
            "failed to initalise noconfirm"
        );

        self.raur = raur::Handle::default().with_url(self.aur_url.join("rpc")?.as_str())?;

        self.fetch = aur_fetch::Handle {
            git: self.git_bin.clone().into(),
            git_flags: self.git_flags.clone(),
            clone_dir: self.build_dir.clone(),
            diff_dir: self.cache_dir.join("diff"),
            aur_url: self.aur_url.clone(),
        };

        self.need_root = self.need_root();

        Ok(())
    }

    fn init_pacmanconf(&mut self) -> Result<()> {
        self.pacman =
            pacmanconf::Config::with_opts(None, self.pacman_conf.as_deref(), self.root.as_deref())?;

        if let Some(ref dbpath) = self.db_path {
            self.pacman.db_path = dbpath.clone();
        }

        self.pacman.ignore_pkg = self.ignore.clone();
        self.pacman.ignore_group = self.ignore_group.clone();

        Ok(())
    }

    pub fn init_alpm(&mut self) -> Result<()> {
        let mut alpm =
            alpm::Alpm::new(&self.pacman.root_dir, &self.pacman.db_path).with_context(|| {
                format!(
                    "failed to initialise alpm: root={} dbpath={}",
                    self.pacman.root_dir, self.pacman.db_path
                )
            })?;

        set_questioncb!(alpm, question);

        for repo in &self.pacman.repos {
            alpm.register_syncdb_mut(&repo.name, SigLevel::NONE)?;

            let mut usage = Usage::NONE;

            for v in &repo.usage {
                match v.as_str() {
                    "Sync" => usage |= Usage::SYNC,
                    "Search" => usage |= Usage::SEARCH,
                    "Install" => usage |= Usage::INSTALL,
                    "Upgrade" => usage |= Usage::UPGRADE,
                    _ => {}
                }

                if usage == Usage::NONE {
                    usage = Usage::ALL
                }
            }
        }

        alpm.set_ignorepkgs(&self.pacman.ignore_pkg)?;
        alpm.set_ignoregroups(&self.pacman.ignore_pkg)?;

        alpm.set_logfile(&self.pacman.log_file)?;
        alpm.set_arch(&self.pacman.architecture);
        alpm.set_noupgrades(&self.pacman.no_upgrade)?;
        alpm.set_use_syslog(self.pacman.use_syslog);

        self.alpm = Alpm::new(alpm);
        Ok(())
    }

    fn parse_stdin(&mut self) -> Result<()> {
        for line in stdin().lock().lines() {
            self.targets.push(line?);
        }

        Ok(())
    }

    fn need_root(&self) -> bool {
        let args = &self.args;

        if self.op == "database" {
            return !args.has_arg("k", "check");
        } else if self.op == "files" {
            return args.has_arg("y", "refresh");
        } else if self.op == "query" {
            return args.has_arg("k", "check");
        } else if self.op == "remove" {
            return !(args.has_arg("p", "print") || args.has_arg("p", "print-format"));
        } else if self.op == "sync" {
            if args.has_arg("y", "refresh") {
                return true;
            }

            return !(args.has_arg("p", "print")
                || args.has_arg("p", "print-format")
                || args.has_arg("s", "search")
                || args.has_arg("l", "list")
                || args.has_arg("g", "groups")
                || args.has_arg("i", "info")
                || (args.has_arg("c", "clean") && self.mode == "aur"));
        } else if self.op == "upgrade" {
            return true;
        }

        false
    }

    fn parse_section(&mut self, section: &str) -> Result<()> {
        self.section = Some(section.to_string());
        Ok(())
    }

    fn parse_directive(&mut self, key: &str, value: Option<&str>) -> Result<()> {
        if key == "Include" {
            let value = match value {
                Some(value) => value,
                None => bail!("value can not be empty for value '{}'", key),
            };

            let ini = std::fs::read_to_string(value)?;

            let section = self.section.clone();
            let section = section.as_deref();
            let section = self
                .parse_with_section(section, Some(value), &ini)?
                .map(|s| s.to_string());
            self.section = section;
            return Ok(());
        }

        let section = match &self.section {
            Some(section) => section.as_str(),
            None => bail!("key '{}' does not belong to a section", key),
        };

        match section {
            "options" => self.parse_option(key, value),
            "bin" => self.parse_bin(key, value),
            _ => bail!("unkown section '{}', section"),
        }
    }

    fn parse_bin(&mut self, key: &str, value: Option<&str>) -> Result<()> {
        let value = value
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("key can not be empty"))?;

        let split = value.split_whitespace().map(|s| s.to_string());

        match key {
            "Makepkg" => self.makepkg_bin = value,
            "Pacman" => self.pacman_bin = value,
            "Git" => self.git_bin = value,
            "Asp" => self.asp_bin = value,
            "Gpg" => self.gpg_bin = value,
            "Sudo" => self.sudo_bin = value,
            "FileManager" => self.fm = Some(value),
            "MFlags" => self.mflags.extend(split),
            "GitFlags" => self.git_flags.extend(split),
            "GpgFlags" => self.gpg_flags.extend(split),
            "SudoFlags" => self.sudo_flags.extend(split),
            "FileManagerFlags" => self.fm_flags.extend(split),
            _ => bail!("unkown option '{}'", key),
        };

        Ok(())
    }

    fn parse_option(&mut self, key: &str, value: Option<&str>) -> Result<()> {
        let no_all = &["no", "all"];
        let yes_no_ask = &["yes", "no", "ask"];
        let sort_by = &[
            "votes",
            "popularity",
            "name",
            "base",
            "submitted",
            "modified",
            "id",
            "baseid",
        ];
        let search_by = &[
            "name",
            "name-desc",
            "maintainer",
            "depends",
            "checkdepends",
            "makedepends",
            "optdepends",
        ];

        let mut ok1 = true;
        let mut ok2 = true;

        match key {
            "BottomUp" => self.sort_mode = "bottomup".into(),
            "AurOnly" => self.mode = "aur".into(),
            "RepoOnly" => self.mode = "repo".into(),
            "SudoLoop" => self.sudo_loop = true,
            "Devel" => self.devel = true,
            "CleanAfter" => self.clean_after = true,
            "Provides" => self.provides = true,
            "PgpFetch" => self.pgp_fetch = true,
            "CombinedUpgrade" => self.combined_upgrade = true,
            "BatchInstall" => self.batch_install = true,
            "UseAsk" => self.use_ask = true,
            "Redownload" => {
                let value = value.unwrap_or("all").into();
                self.redownload = validate(value, no_all)?;
            }
            "Rebuild" => {
                let value = value.unwrap_or("all").into();
                self.rebuild = validate(value, no_all)?;
            }
            "RemoveMake" => {
                let value = value.unwrap_or("yes").into();
                self.remove_make = validate(value, yes_no_ask)?;
            }
            "UpgradeMenu" => {
                self.upgrade_menu = true;
                if let Some(value) = value {
                    self.answer_upgrade = Some(value.to_string());
                }
            }
            _ => ok1 = false,
        }

        if ok1 {
            return Ok(());
        }

        let has_value = value.is_some();
        let value = value
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("value can not be empty for value '{}'", key));

        match key {
            "AurUrl" => self.aur_url = value?.parse()?,
            "BuildDir" => self.build_dir = PathBuf::from(value?),
            "Redownload" => self.redownload = validate(value?, no_all)?,
            "Rebuild" => self.rebuild = validate(value?, no_all)?,
            "RemoveMake" => self.remove_make = validate(value?, yes_no_ask)?,
            "SortBy" => self.sort_by = validate(value?, sort_by)?,
            "SearchBy" => self.search_by = validate(value?, search_by)?,
            "RequestSplit" => self.request_split = value?.parse()?,
            "CompletionInterval" => self.completion_interval = value?.parse()?,
            "PacmanConf" => self.pacman_conf = Some(value?.to_string()),
            _ => ok2 = false,
        };

        ensure!(ok1 || ok2, "unkown option '{}'", key);
        ensure!(ok1 || has_value, "option '{}' does not take a value", key);
        Ok(())
    }
}

fn help() {
    let help = include_str!("../help");
    sprint!("{}", help);
}

fn version() {
    let ver = option_env!("PARU_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
    sprint!("paru v{}", ver);
    #[cfg(feature = "git")]
    sprint!(" +git");
    #[cfg(feature = "backtrace")]
    sprint!(" +backtrace");
    sprintln!(" - libalpm v{}", alpm::version());
}

fn validate(key: String, valid: &[&str]) -> Result<String> {
    if !valid.iter().cloned().any(|v| v == key) {
        bail!("invalid value for '{}', expected: {}", key, valid.join("|"))
    }
    Ok(key)
}

fn reopen_stdin() -> Result<()> {
    let stdin_fd = 0;
    let file = File::open("/dev/tty")?;

    dup2(file.as_raw_fd(), stdin_fd)?;

    Ok(())
}

fn question(question: &mut Question) {
    let c = COLORS.get().unwrap();

    match question {
        Question::SelectProvider(question) => {
            let providers = question.providers();
            let len = providers.len();

            sprintln!();
            let prompt = format!(
                "There are {} providers avaliable for {}:",
                len,
                question.depend()
            );
            sprintln!("{} {}", c.action.paint("::"), c.bold.paint(prompt));

            let mut db = String::new();
            for (n, pkg) in providers.enumerate() {
                let pkg_db = pkg.db().unwrap();
                if pkg_db.name() != db {
                    db = pkg_db.name().to_string();
                    sprintln!(
                        "{} {} {}:",
                        c.action.paint("::"),
                        c.bold.paint("Repository"),
                        color_repo(pkg_db.name())
                    );
                    sprint!("    ");
                }
                sprint!("{}) {}  ", n + 1, pkg.name());
            }

            let index = get_provider(len);
            question.set_index(index as i32);
        }
        _ => (),
    }
}
