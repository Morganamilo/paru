use crate::args::Args;
use crate::exec::{self, Status};
use crate::fmt::color_repo;
use crate::util::get_provider;
use crate::{alpm_debug_enabled, help, printtr, repo};

use std::env::consts::ARCH;
use std::env::{remove_var, set_var, var};
use std::fmt;
use std::fs::File;
use std::io::{stdin, stdout, BufRead};
use std::path::{Path, PathBuf};

use alpm::{
    AnyDownloadEvent, AnyQuestion, Depend, DownloadEvent, DownloadResult, LogLevel, Question,
};
use ansi_term::Color::{Blue, Cyan, Green, Purple, Red, Yellow};
use ansi_term::Style;
use anyhow::{anyhow, bail, ensure, Context, Error, Result};
use cini::{Callback, CallbackKind, Ini};
use globset::{Glob, GlobSet, GlobSetBuilder};
use nix::unistd::{dup2, isatty};
use std::os::unix::io::AsRawFd;
use tr::tr;
use url::Url;

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

impl std::ops::DerefMut for Alpm {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.alpm.as_mut().unwrap()
    }
}

impl Alpm {
    fn new(alpm: alpm::Alpm) -> Self {
        Self { alpm: Some(alpm) }
    }
}

#[derive(Debug, SmartDefault, Clone, PartialEq, Eq)]
pub enum LocalRepos {
    #[default]
    None,
    Default,
    Repo(Vec<String>),
}

impl LocalRepos {
    pub fn new(repo: Option<&str>) -> Self {
        match repo {
            Some(s) => LocalRepos::Repo(s.split_whitespace().map(|s| s.to_string()).collect()),
            None => LocalRepos::Default,
        }
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
    pub ss_ood: Style,
    pub code: Style,
    pub news_date: Style,
    pub old_version: Style,
    pub new_version: Style,
    pub number_menu: Style,
    pub group: Style,
    pub stats_line_separator: Style,
    pub stats_value: Style,
}

impl From<&str> for Colors {
    fn from(s: &str) -> Self {
        match s {
            "auto" if isatty(stdout().as_raw_fd()).unwrap_or(false) => Colors::new(),
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
            upgrade: Style::new().fg(Green).bold(),
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
            ss_ood: Style::new().fg(Red).bold(),
            code: Style::new().fg(Cyan),
            news_date: Style::new().fg(Cyan).bold(),
            old_version: Style::new().fg(Red),
            new_version: Style::new().fg(Green),
            number_menu: Style::new().fg(Purple),
            group: Style::new().fg(Blue).bold(),
            stats_line_separator: Style::new().fg(Blue).bold(),
            stats_value: Style::new().fg(Cyan),
        }
    }
}

pub trait ConfigEnum: Sized + PartialEq + Copy + Clone + fmt::Debug + 'static {
    const VALUE_LOOKUP: &'static [(&'static str, Self)];

    fn as_str(&self) -> &'static str {
        Self::VALUE_LOOKUP
            .iter()
            .find(|(_, v)| self == v)
            .map(|(k, _)| k)
            .unwrap()
    }

    fn default_or(self, key: &str, value: Option<&str>) -> Result<Self> {
        value.map_or(Ok(self), |value| ConfigEnum::from_str(key, value))
    }

    fn from_str(key: &str, value: &str) -> Result<Self> {
        let val = Self::VALUE_LOOKUP
            .iter()
            .find(|(name, _)| name == &value)
            .map(|(_, res)| *res);

        if let Some(val) = val {
            Ok(val)
        } else {
            let okvalues = Self::VALUE_LOOKUP
                .iter()
                .map(|v| v.0)
                .collect::<Vec<&str>>()
                .join("|");
            bail!(tr!(
                "invalid value '{val}' for key '{key}', expected: {exp}",
                val = value,
                key = key,
                exp = okvalues
            ))
        }
    }
}

type ConfigEnumValues<T> = &'static [(&'static str, T)];

#[derive(Debug, SmartDefault, PartialEq, Eq)]
pub enum Sign {
    #[default]
    No,
    Yes,
    Key(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    ChrootCtl,
    Database,
    DepTest,
    Files,
    GetPkgBuild,
    Query,
    Remove,
    RepoCtl,
    Show,
    Sync,
    Upgrade,
    Yay,
}

impl ConfigEnum for Op {
    const VALUE_LOOKUP: ConfigEnumValues<Self> = &[
        ("chrootctl", Self::ChrootCtl),
        ("database", Self::Database),
        ("deptest", Self::DepTest),
        ("files", Self::Files),
        ("getpkgbuild", Self::GetPkgBuild),
        ("query", Self::Query),
        ("remove", Self::Remove),
        ("repoctl", Self::RepoCtl),
        ("show", Self::Show),
        ("sync", Self::Sync),
        ("upgrade", Self::Upgrade),
        ("yay", Self::Yay),
    ];
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    Base,
    BaseId,
    Id,
    Modified,
    Name,
    Popularity,
    Submitted,
    Votes,
}

impl ConfigEnum for SortBy {
    const VALUE_LOOKUP: ConfigEnumValues<Self> = &[
        ("base", Self::Base),
        ("baseid", Self::BaseId),
        ("id", Self::Id),
        ("modified", Self::Modified),
        ("name", Self::Name),
        ("popularity", Self::Popularity),
        ("submitted", Self::Submitted),
        ("votes", Self::Votes),
    ];
}

impl ConfigEnum for raur::SearchBy {
    const VALUE_LOOKUP: ConfigEnumValues<Self> = &[
        ("checkdepends", Self::CheckDepends),
        ("depends", Self::Depends),
        ("maintainer", Self::Maintainer),
        ("makedepends", Self::MakeDepends),
        ("name-desc", Self::NameDesc),
        ("name", Self::Name),
        ("optdepends", Self::OptDepends),
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    BottomUp,
    TopDown,
}

impl ConfigEnum for SortMode {
    const VALUE_LOOKUP: ConfigEnumValues<Self> =
        &[("bottomup", Self::BottomUp), ("topdown", Self::TopDown)];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Any,
    Aur,
    Repo,
}

impl ConfigEnum for Mode {
    const VALUE_LOOKUP: ConfigEnumValues<Self> =
        &[("any", Self::Any), ("aur", Self::Aur), ("repo", Self::Repo)];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YesNoAll {
    Yes,
    No,
    All,
}

impl ConfigEnum for YesNoAll {
    const VALUE_LOOKUP: ConfigEnumValues<Self> =
        &[("yes", Self::Yes), ("no", Self::No), ("all", Self::All)];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YesNoAsk {
    Yes,
    No,
    Ask,
}

impl ConfigEnum for YesNoAsk {
    const VALUE_LOOKUP: ConfigEnumValues<Self> =
        &[("yes", Self::Yes), ("no", Self::No), ("ask", Self::Ask)];
}

#[derive(SmartDefault, Debug)]
pub struct Config {
    section: Option<String>,
    pub args: Args<String>,
    pub globals: Args<String>,

    pub cols: Option<usize>,

    #[default(Op::Yay)]
    pub op: Op,

    #[cfg(not(feature = "mock"))]
    pub raur: raur::Handle,
    #[cfg(feature = "mock")]
    pub raur: crate::mock::Mock,
    #[default(aur_fetch::Handle::with_cache_dir(""))]
    pub fetch: aur_fetch::Handle,
    pub cache: raur::Cache,
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
    pub stats: bool,
    pub order: bool,
    pub gendb: bool,

    #[default(YesNoAll::No)]
    pub redownload: YesNoAll,
    #[default(YesNoAll::No)]
    pub rebuild: YesNoAll,
    #[default(YesNoAsk::No)]
    pub remove_make: YesNoAsk,
    #[default(SortBy::Votes)]
    pub sort_by: SortBy,
    #[default(raur::SearchBy::NameDesc)]
    pub search_by: raur::SearchBy,
    pub limit: usize,
    #[default(SortMode::TopDown)]
    pub sort_mode: SortMode,
    #[default(Mode::Any)]
    pub mode: Mode,
    pub aur_filter: bool,

    #[default = 7]
    pub completion_interval: u64,

    pub help: bool,
    pub version: bool,

    pub skip_review: bool,
    pub no_check: bool,
    pub no_confirm: bool,
    pub devel: bool,
    pub clean_after: bool,
    pub provides: bool,
    pub pgp_fetch: bool,
    pub combined_upgrade: bool,
    pub batch_install: bool,
    pub use_ask: bool,
    pub save_changes: bool,
    pub clean: usize,
    pub complete: bool,
    pub print: bool,
    pub news_on_upgrade: bool,
    pub comments: bool,
    pub ssh: bool,
    pub keep_repo_cache: bool,
    pub fail_fast: bool,
    pub keep_src: bool,

    pub sign: Sign,
    pub sign_db: Sign,

    pub pre_build_command: Option<String>,

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
    #[default = "bat"]
    pub bat_bin: String,
    pub fm: Option<String>,
    pub sudo_loop: Vec<String>,

    pub mflags: Vec<String>,
    pub git_flags: Vec<String>,
    pub gpg_flags: Vec<String>,
    pub sudo_flags: Vec<String>,
    pub bat_flags: Vec<String>,
    pub fm_flags: Vec<String>,
    pub pager_cmd: Option<String>,

    pub devel_suffixes: Vec<String>,
    #[default(GlobSet::empty())]
    pub no_warn: GlobSet,
    #[default(GlobSetBuilder::new())]
    pub no_warn_builder: GlobSetBuilder,
    pub install_debug: bool,

    pub upgrade_menu: bool,

    pub makepkg_conf: Option<String>,
    pub pacman_conf: Option<String>,

    pub repos: LocalRepos,
    #[default(Path::new("/var/lib/aurbuild/").join(ARCH))]
    pub chroot_dir: PathBuf,
    pub chroot: bool,
    pub install: bool,
    pub uninstall: bool,
    pub sysupgrade: bool,
    pub refresh: bool,
    pub quiet: bool,
    pub list: bool,
    pub delete: u32,

    //pacman
    pub db_path: Option<String>,
    pub root: Option<String>,
    pub verbose: bool,
    pub ask: usize,
    pub arch: Option<String>,

    pub ignore: Vec<String>,
    pub ignore_group: Vec<String>,
    pub assume_installed: Vec<String>,
}

impl Ini for Config {
    type Err = Error;

    fn callback(&mut self, cb: Callback) -> Result<(), Self::Err> {
        let err = match cb.kind {
            CallbackKind::Section(section) => {
                self.section = Some(section.to_string());
                if !matches!(section, "options" | "bin" | "env") {
                    eprintln!("{}", tr!("error: unknown section '{}'", section));
                }
                Ok(())
            }
            CallbackKind::Directive(_, key, value) => self.parse_directive(key, value),
        };

        let filename = cb.filename.unwrap_or("paru.conf");
        err.map_err(|e| anyhow!("{}:{}: {}", filename, cb.line_number, e))
    }
}

impl Config {
    pub fn new() -> Result<Self> {
        let cache =
            dirs::cache_dir().ok_or_else(|| anyhow!(tr!("failed to find cache directory")))?;
        let cache = cache.join("paru");
        let config =
            dirs::config_dir().ok_or_else(|| anyhow!(tr!("failed to find config directory")))?;
        let config = config.join("paru");

        let build_dir = cache.join("clone");
        let config_path = config.join("paru.conf");
        let devel_path = cache.join("devel.json");
        let cache_dir = cache;

        let color = Colors::from("never");
        let cols = term_size::dimensions_stdout().map(|v| v.0);

        let mut config = Self {
            cols,
            color,
            build_dir,
            cache_dir,
            devel_path,
            ..Self::default()
        };

        if let Ok(conf) = var("PARU_CONF") {
            let path = PathBuf::from(conf);
            ensure!(
                path.exists(),
                tr!("config file '{}' does not exist", path.display())
            );
            config.config_path = Some(path);
        } else if config_path.exists() {
            config.config_path = Some(config_path);
        } else {
            let config_path = PathBuf::from("/etc/paru.conf");

            if config_path.exists() {
                config.config_path = Some(config_path);
            }
        }

        Ok(config)
    }

    pub fn set_op_args_globals(&mut self, op: Op) {
        self.op = op;
        self.args.op = op.as_str().to_string();
        self.globals.op = op.as_str().to_string();
    }

    pub fn pacman_args(&self) -> Args<&str> {
        self.args.as_str()
    }

    pub fn pacman_globals(&self) -> Args<&str> {
        self.globals.as_str()
    }

    pub fn parse_args<S: AsRef<str>, I: IntoIterator<Item = S>>(&mut self, iter: I) -> Result<()> {
        let iter = iter.into_iter();
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

            ensure!(
                op_count <= 1,
                tr!("only one operation may be used at a time")
            );
        }

        if let Some((i, _)) = self.targets.iter().enumerate().find(|t| t.1 == "-") {
            self.targets.remove(i);
            self.parse_stdin()?;
            reopen_stdin()?;
        }

        self.args.op = self.op.as_str().to_string();
        self.args.targets = self.targets.clone();
        self.args.bin = self.pacman_bin.clone();

        self.globals.op = self.op.as_str().to_string();
        self.globals.bin = self.pacman_bin.clone();

        if self.help {
            match self.op {
                Op::GetPkgBuild | Op::Show | Op::Yay => {
                    help::help();
                    std::process::exit(0);
                }
                _ => {
                    let status = exec::pacman(self, &self.args).unwrap_or(Status(1));
                    std::process::exit(status.code());
                }
            }
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

        #[cfg(not(feature = "mock"))]
        {
            use std::time::Duration;

            let ver = option_env!("PARU_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
            let client = reqwest::Client::builder()
                .tcp_keepalive(Duration::new(15, 0))
                .user_agent(format!("paru/{}", ver))
                .build()?;

            self.raur = raur::Handle::new_with_settings(client, self.aur_url.join("rpc")?.as_str());
        }

        #[cfg(feature = "mock")]
        {
            self.raur = crate::mock::Mock::new()?;
        }

        let aur_url = if self.ssh {
            self.aur_url
                .to_string()
                .replacen("https://", "ssh://aur@", 1)
                .parse()
                .expect("change AUR URL schema from HTTPS to SSH")
        } else {
            self.aur_url.clone()
        };

        self.fetch = aur_fetch::Handle {
            git: self.git_bin.clone().into(),
            git_flags: self.git_flags.clone(),
            clone_dir: self.build_dir.clone(),
            diff_dir: self.cache_dir.join("diff"),
            aur_url,
        };

        self.need_root = self.need_root();

        if self.repos != LocalRepos::None {
            let repos = repo::repo_aur_dbs(self).1;

            if repos.is_empty() {
                bail!(
                    "no local repos configured, add one to your pacman.conf:
    [options]
    CacheDir = /var/lib/repo/aur

    [aur]
    SigLevel = PackageOptional DatabaseOptional
    Server = file:///var/lib/repo/aur"
                );
            }

            for repo in repos {
                if !self.pacman.repos.iter().any(|r| r.name == repo.name()) {
                    bail!(tr!(
                        "can not find local repo '{}' in pacman.conf",
                        repo.name()
                    ));
                }
            }
        }
        self.no_warn = self.no_warn_builder.build()?;

        if !self.assume_installed.is_empty() && !self.chroot {
            self.mflags.push("-d".to_string());
        }

        if self.chroot {
            remove_var("PKGEXT");
        }

        Ok(())
    }

    fn init_pacmanconf(&mut self) -> Result<()> {
        self.pacman =
            pacmanconf::Config::with_opts(None, self.pacman_conf.as_deref(), self.root.as_deref())?;

        if let Some(ref dbpath) = self.db_path {
            self.pacman.db_path = dbpath.clone();
        }

        self.ignore.extend(self.pacman.ignore_pkg.clone());
        self.ignore_group.extend(self.pacman.ignore_group.clone());

        Ok(())
    }

    pub fn new_alpm(&self) -> Result<alpm::Alpm> {
        let mut alpm = alpm_utils::alpm_with_conf(&self.pacman).with_context(|| {
            tr!(
                "failed to initialize alpm: root={} dbpath={}",
                self.pacman.root_dir,
                self.pacman.db_path
            )
        })?;

        alpm.set_question_cb((self.no_confirm, self.color), question);
        alpm.set_dl_cb((), download);
        alpm.set_log_cb(self.color, log);

        if !self.chroot {
            for dep in &self.assume_installed {
                alpm.add_assume_installed(&Depend::new(dep.as_str()))?;
            }
        }

        for pkg in &self.ignore {
            alpm.add_ignorepkg(pkg.as_str())?;
        }

        for group in &self.ignore_group {
            alpm.add_ignoregroup(group.as_str())?;
        }

        Ok(alpm)
    }

    pub fn init_alpm(&mut self) -> Result<()> {
        self.alpm = Alpm::new(self.new_alpm()?);
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

        if self.op == Op::Database {
            return !args.has_arg("k", "check");
        } else if self.op == Op::Files {
            return args.has_arg("y", "refresh");
        } else if self.op == Op::Query {
            return args.has_arg("k", "check");
        } else if self.op == Op::Remove {
            return !(args.has_arg("p", "print") || args.has_arg("p", "print-format"));
        } else if self.op == Op::Sync {
            if args.has_arg("y", "refresh") {
                return true;
            }

            return !(args.has_arg("p", "print")
                || args.has_arg("p", "print-format")
                || args.has_arg("s", "search")
                || args.has_arg("l", "list")
                || args.has_arg("g", "groups")
                || args.has_arg("i", "info")
                || (args.has_arg("c", "clean") && self.mode == Mode::Aur));
        } else if self.op == Op::Upgrade {
            return true;
        }

        false
    }

    fn parse_directive(&mut self, key: &str, value: Option<&str>) -> Result<()> {
        if key == "Include" {
            let value = match value {
                Some(value) => value,
                None => bail!(tr!("value can not be empty for key '{}'", key)),
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
            None => bail!(tr!("key '{}' does not belong to a section", key)),
        };

        match section {
            "options" => self.parse_option(key, value),
            "bin" => self.parse_bin(key, value),
            "env" => self.parse_env(key, value),
            _ => Ok(()),
        }
    }

    fn parse_env(&mut self, key: &str, value: Option<&str>) -> Result<()> {
        let value = value.context(tr!("key can not be empty"))?;

        ensure!(!key.is_empty(), tr!("key can not be empty"));
        ensure!(!key.contains('\0'), tr!("key can not contain null bytes"));
        ensure!(
            !value.contains('\0'),
            tr!("value can not contain null bytes")
        );

        set_var(key, value);
        Ok(())
    }

    fn parse_bin(&mut self, key: &str, value: Option<&str>) -> Result<()> {
        let value = value
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!(tr!("key can not be empty")))?;

        let split = value.split_whitespace().map(|s| s.to_string());

        match key {
            "Makepkg" => self.makepkg_bin = value,
            "Pacman" => self.pacman_bin = value,
            "Git" => self.git_bin = value,
            "Asp" => self.asp_bin = value,
            "Gpg" => self.gpg_bin = value,
            "Sudo" => self.sudo_bin = value,
            "Pager" => self.pager_cmd = Some(value),
            "Bat" => self.bat_bin = value,
            "FileManager" => self.fm = Some(value),
            "MFlags" => self.mflags.extend(split),
            "GitFlags" => self.git_flags.extend(split),
            "GpgFlags" => self.gpg_flags.extend(split),
            "SudoFlags" => self.sudo_flags.extend(split),
            "BatFlags" => self.bat_flags.extend(split),
            "FileManagerFlags" => self.fm_flags.extend(split),
            "PreBuildCommand" => self.pre_build_command = Some(value),
            _ => eprintln!(
                "{}",
                tr!("error: unknown option '{}' in section [bin]", key)
            ),
        };

        Ok(())
    }

    fn parse_option(&mut self, key: &str, value: Option<&str>) -> Result<()> {
        let mut ok1 = true;
        let mut ok2 = true;

        match key {
            "SkipReview" => self.skip_review = true,
            "BottomUp" => self.sort_mode = SortMode::BottomUp,
            "AurOnly" => self.mode = Mode::Aur,
            "RepoOnly" => self.mode = Mode::Repo,
            "SudoLoop" => {
                self.sudo_loop = value
                    .unwrap_or("-v")
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect()
            }
            "Devel" => self.devel = true,
            "NoCheck" => self.no_check = true,
            "CleanAfter" => self.clean_after = true,
            "Provides" => self.provides = true,
            "PgpFetch" => self.pgp_fetch = true,
            "CombinedUpgrade" => self.combined_upgrade = true,
            "BatchInstall" => self.batch_install = true,
            "UseAsk" => self.use_ask = true,
            "SaveChanges" => self.save_changes = true,
            "NewsOnUpgrade" => self.news_on_upgrade = true,
            "InstallDebug" => self.install_debug = true,
            "Redownload" => self.redownload = YesNoAll::All.default_or(key, value)?,
            "Rebuild" => self.rebuild = YesNoAll::All.default_or(key, value)?,
            "RemoveMake" => self.remove_make = YesNoAsk::Yes.default_or(key, value)?,
            "UpgradeMenu" => self.upgrade_menu = true,
            "LocalRepo" => self.repos = LocalRepos::new(value),
            "Chroot" => {
                if self.repos == LocalRepos::None {
                    self.repos = LocalRepos::Default;
                }
                self.chroot = true;
                if let Some(p) = value {
                    self.chroot_dir = p.into();
                }
            }
            "Sign" => {
                self.sign = match value {
                    Some(v) => Sign::Key(v.to_string()),
                    None => Sign::Yes,
                }
            }
            "KeepRepoCache" => self.keep_repo_cache = true,
            "FailFast" => self.fail_fast = true,
            "KeepSrc" => self.keep_src = true,
            "SignDb" => {
                self.sign_db = match value {
                    Some(v) => Sign::Key(v.to_string()),
                    None => Sign::Yes,
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
            .ok_or_else(|| anyhow!(tr!("value can not be empty for key '{}'", key)));

        match key {
            "AurUrl" => self.aur_url = value?.parse()?,
            "BuildDir" | "CloneDir" => self.build_dir = PathBuf::from(value?),
            "Redownload" => self.redownload = ConfigEnum::from_str(key, value?.as_str())?,
            "Rebuild" => self.rebuild = ConfigEnum::from_str(key, value?.as_str())?,
            "RemoveMake" => self.remove_make = ConfigEnum::from_str(key, value?.as_str())?,
            "SortBy" => self.sort_by = ConfigEnum::from_str(key, value?.as_str())?,
            "SearchBy" => self.search_by = ConfigEnum::from_str(key, value?.as_str())?,
            "Limit" => self.limit = value?.parse()?,
            "CompletionInterval" => self.completion_interval = value?.parse()?,
            "PacmanConf" => self.pacman_conf = Some(value?),
            "MakepkgConf" => self.makepkg_conf = Some(value?),
            "DevelSuffixes" => {
                self.devel_suffixes
                    .extend(value?.split_whitespace().map(|s| s.to_string()));
            }
            "NoWarn" => {
                for word in value?.split_whitespace() {
                    self.no_warn_builder.add(Glob::new(word)?);
                }
            }
            _ => ok2 = false,
        };

        if !(ok1 || ok2) {
            eprintln!(
                "{}",
                tr!("error: unknown option '{}' in section [options]", key)
            )
        } else {
            ensure!(
                ok1 || has_value,
                tr!("option '{}' does not take a value", key)
            );
        }
        Ok(())
    }

    pub fn aur_namespace(&self) -> &str {
        if self.pacman.repos.iter().any(|r| r.name == "aur") {
            // hack for search install
            "__aur__"
        } else {
            "aur"
        }
    }
}

pub fn version() {
    let ver = option_env!("PARU_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
    print!("paru v{}", ver);
    #[cfg(feature = "git")]
    print!(" +git");
    #[cfg(feature = "backtrace")]
    print!(" +backtrace");
    println!(" - libalpm v{}", alpm::version());
}

fn reopen_stdin() -> Result<()> {
    let stdin_fd = 0;
    let file = File::open("/dev/tty")?;

    dup2(file.as_raw_fd(), stdin_fd)?;

    Ok(())
}

fn question(question: AnyQuestion, data: &mut (bool, Colors)) {
    let no_confirm = data.0;
    let c = data.1;

    match question.question() {
        Question::SelectProvider(mut question) => {
            let providers = question.providers();
            let len = providers.len();

            println!();
            let prompt = tr!(
                "There are {n} providers available for {pkg}:",
                n = len,
                pkg = question.depend()
            );
            print!("{} {}", c.action.paint("::"), c.bold.paint(prompt));

            let mut db = String::new();
            for (n, pkg) in providers.iter().enumerate() {
                let pkg_db = pkg.db().unwrap();
                if pkg_db.name() != db {
                    db = pkg_db.name().to_string();
                    println!(
                        "\n{} {} {}:",
                        c.action.paint("::"),
                        c.bold.paint(tr!("Repository")),
                        color_repo(c.enabled, pkg_db.name())
                    );
                    print!("    ");
                }
                print!("{}) {}  ", n + 1, pkg.name());
            }

            let index = get_provider(len, no_confirm);
            question.set_index(index as i32);
        }
        Question::InstallIgnorepkg(mut question) => {
            question.set_install(true);
        }
        _ => (),
    }
}

fn download(filename: &str, event: AnyDownloadEvent, _: &mut ()) {
    match event.event() {
        DownloadEvent::Init(_) => println!("  syncing {}...", filename),
        DownloadEvent::Completed(c) if c.result == DownloadResult::Failed => {
            printtr!("  failed to sync {}", filename);
        }
        _ => (),
    }
}

fn log(level: LogLevel, msg: &str, color: &mut Colors) {
    let err = color.error;
    let warn = color.warning;

    match level {
        LogLevel::WARNING => eprint!("{} {}", warn.paint("::"), msg),
        LogLevel::ERROR => eprint!("{} {}", err.paint("error:"), msg),
        LogLevel::DEBUG if alpm_debug_enabled() => eprint!("debug: <alpm> {}", msg),
        _ => (),
    }
}
