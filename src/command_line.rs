use crate::args::{PACMAN_FLAGS, PACMAN_GLOBALS};
use crate::config::{
    Colors, Config, ConfigEnum, LocalRepos, Mode, Op, Sign, SortMode, YesNoAll, YesNoAsk,
};

use std::fmt;

use anyhow::{anyhow, bail, Context, Result};
use tr::tr;
use url::Url;

#[derive(Debug, Copy, Clone)]
enum Arg<'a> {
    Short(char),
    Long(&'a str),
}

impl<'a> fmt::Display for Arg<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Arg::Short(c) => write!(f, "-{}", c),
            Arg::Long(l) => write!(f, "--{}", l),
        }
    }
}

impl<'a> Arg<'a> {
    fn arg(self) -> String {
        match self {
            Arg::Long(arg) => arg.to_string(),
            Arg::Short(arg) => arg.to_string(),
        }
    }

    fn is_pacman_arg(self) -> bool {
        match self {
            Arg::Long(arg) => PACMAN_FLAGS.contains(&arg),
            Arg::Short(arg) => {
                let mut buff = [0, 0, 0, 0];
                let arg = arg.encode_utf8(&mut buff);
                let arg: &str = arg;
                PACMAN_FLAGS.contains(&arg)
            }
        }
    }

    fn is_pacman_global(self) -> bool {
        match self {
            Arg::Long(arg) => PACMAN_GLOBALS.contains(&arg),
            Arg::Short(arg) => {
                let mut buff = [0, 0, 0, 0];
                let arg = arg.encode_utf8(&mut buff);
                let arg: &str = arg;
                PACMAN_GLOBALS.contains(&arg)
            }
        }
    }
}

#[derive(PartialEq)]
enum TakesValue {
    Required,
    No,
    Optional,
}

impl Config {
    pub fn parse_arg(
        &mut self,
        arg: &str,
        value: Option<&str>,
        op_count: &mut u8,
        end_of_ops: &mut bool,
    ) -> Result<bool> {
        let mut forced = false;

        if arg == "-" || *end_of_ops {
            self.targets.push(arg.to_string());
            return Ok(false);
        }
        if arg == "--" {
            *end_of_ops = true;
            return Ok(false);
        }

        if arg.starts_with("--") {
            let mut value = value;
            let mut split = arg.splitn(2, '=');
            let arg = split.next().unwrap();
            let arg = Arg::Long(arg.trim_start_matches("--"));
            let mut used_next = takes_value(arg) == TakesValue::Required;
            if let Some(val) = split.next() {
                value = Some(val);
                used_next = false;
                forced = true;
            }

            self.handle_arg(arg, value, op_count, forced)?;
            Ok(used_next)
        } else if arg.starts_with('-') {
            let mut chars = arg.chars();
            chars.next().unwrap();

            while let Some(c) = chars.next() {
                let arg = Arg::Short(c);
                if takes_value(arg) == TakesValue::Required {
                    if chars.as_str().is_empty() {
                        self.handle_arg(arg, value, op_count, false)?;
                        return Ok(true);
                    } else {
                        self.handle_arg(arg, Some(chars.as_str()), op_count, false)?;
                        return Ok(false);
                    }
                }
                self.handle_arg(arg, None, op_count, false)?;
            }
            Ok(false)
        } else {
            self.targets.push(arg.to_string());
            Ok(false)
        }
    }

    fn handle_arg(
        &mut self,
        arg: Arg,
        mut value: Option<&str>,
        op_count: &mut u8,
        forced: bool,
    ) -> Result<()> {
        match takes_value(arg) {
            TakesValue::Required if value.is_none() => bail!(tr!("option {} expects a value", arg)),
            _ => (),
        }

        if takes_value(arg) != TakesValue::Required && !forced {
            value = None;
        }

        if arg.is_pacman_global() {
            self.globals.args.push(crate::args::Arg {
                key: arg.arg(),
                value: value.map(|s| s.to_string()),
            });
            self.args.args.push(crate::args::Arg {
                key: arg.arg(),
                value: value.map(|s| s.to_string()),
            });
        }

        if arg.is_pacman_arg() {
            self.args.args.push(crate::args::Arg {
                key: arg.arg(),
                value: value.map(|s| s.to_string()),
            });
        }

        let mut set_op = |op: Op| {
            self.op = op;
            *op_count += 1;
        };

        let value = value.with_context(|| tr!("option {} does not allow a value", arg));
        let argkey = match arg {
            Arg::Long(n) => n,
            _ => "<impossible_key_of_short_arg>",
        };

        match arg {
            Arg::Long("help") | Arg::Short('h') => self.help = true,
            Arg::Long("version") | Arg::Short('V') => self.version = true,
            Arg::Long("aururl") => self.aur_url = Url::parse(value?)?,
            Arg::Long("makepkg") => self.makepkg_bin = value?.to_string(),
            Arg::Long("pacman") => self.pacman_bin = value?.to_string(),
            Arg::Long("git") => self.git_bin = value?.to_string(),
            Arg::Long("gpg") => self.gpg_bin = value?.to_string(),
            Arg::Long("sudo") => self.sudo_bin = value?.to_string(),
            Arg::Long("asp") => self.asp_bin = value?.to_string(),
            Arg::Long("bat") => self.bat_bin = value?.to_string(),
            Arg::Long("fm") => self.fm = Some(value?.to_string()),
            Arg::Long("pager") => self.pager_cmd = Some(value?.to_string()),
            Arg::Long("config") => self.pacman_conf = Some(value?.to_string()),

            Arg::Long("builddir") | Arg::Long("clonedir") => self.build_dir = value?.into(),
            Arg::Long("makepkgconf") => self.makepkg_conf = Some(value?.to_string()),
            Arg::Long("mflags") => self.mflags.extend(split_whitespace(value?)),
            Arg::Long("gitflags") => self.git_flags.extend(split_whitespace(value?)),
            Arg::Long("gpgflags") => self.gpg_flags.extend(split_whitespace(value?)),
            Arg::Long("sudoflags") => self.sudo_flags.extend(split_whitespace(value?)),
            Arg::Long("batflags") => self.bat_flags.extend(split_whitespace(value?)),
            Arg::Long("fmflags") => self.fm_flags.extend(split_whitespace(value?)),

            Arg::Long("develsuffixes") => self.devel_suffixes = split_whitespace(value?),
            Arg::Long("installdebug") => self.install_debug = true,
            Arg::Long("noinstalldebug") => self.install_debug = false,

            Arg::Long("completioninterval") => {
                self.completion_interval = value?
                    .parse()
                    .map_err(|_| anyhow!("option {} must be a number", arg))?
            }
            Arg::Long("sortby") => self.sort_by = ConfigEnum::from_str(argkey, value?)?,
            Arg::Long("searchby") => self.search_by = ConfigEnum::from_str(argkey, value?)?,
            Arg::Long("limit") => self.limit = value?.parse()?,
            Arg::Long("news") | Arg::Short('w') => self.news += 1,
            Arg::Long("stats") => self.stats = true,
            Arg::Short('s') => {
                self.stats = true;
                self.ssh = true;
            }
            Arg::Long("order") | Arg::Short('o') => self.order = true,
            Arg::Long("removemake") => {
                self.remove_make = YesNoAsk::Yes.default_or(argkey, value.ok())?
            }
            Arg::Long("upgrademenu") => self.upgrade_menu = true,
            Arg::Long("noupgrademenu") => self.upgrade_menu = false,
            Arg::Long("noremovemake") => self.remove_make = YesNoAsk::No,
            Arg::Long("cleanafter") => self.clean_after = true,
            Arg::Long("nocleanafter") => self.clean_after = false,
            Arg::Long("redownload") => {
                self.redownload = YesNoAll::Yes.default_or(argkey, value.ok())?
            }
            Arg::Long("noredownload") => self.redownload = YesNoAll::No,
            Arg::Long("rebuild") => self.rebuild = YesNoAll::Yes.default_or(argkey, value.ok())?,
            Arg::Long("norebuild") => self.rebuild = YesNoAll::No,
            Arg::Long("topdown") => self.sort_mode = SortMode::TopDown,
            Arg::Long("bottomup") => self.sort_mode = SortMode::BottomUp,
            Arg::Long("aur") | Arg::Short('a') => {
                self.mode = Mode::Aur;
                self.aur_filter = true;
            }
            Arg::Long("repo") => self.mode = Mode::Repo,
            Arg::Long("skipreview") => self.skip_review = true,
            Arg::Long("review") => self.skip_review = false,
            Arg::Long("gendb") => self.gendb = true,
            Arg::Long("nocheck") => self.no_check = true,
            Arg::Long("devel") => self.devel = true,
            Arg::Long("nodevel") => self.devel = false,
            Arg::Long("provides") => self.provides = true,
            Arg::Long("noprovides") => self.provides = false,
            Arg::Long("pgpfetch") => self.pgp_fetch = true,
            Arg::Long("nopgpfetch") => self.pgp_fetch = false,
            Arg::Long("useask") => self.use_ask = true,
            Arg::Long("nouseask") => self.use_ask = false,
            Arg::Long("savechanges") => self.save_changes = true,
            Arg::Long("nosavechanges") => self.save_changes = false,
            Arg::Long("combinedupgrade") => self.combined_upgrade = true,
            Arg::Long("nocombinedupgrade") => self.combined_upgrade = false,
            Arg::Long("batchinstall") => self.batch_install = true,
            Arg::Long("nobatchinstall") => self.batch_install = false,
            Arg::Long("sudoloop") => {
                self.sudo_loop = value
                    .unwrap_or("-v")
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect()
            }
            Arg::Long("nosudoloop") => self.sudo_loop.clear(),
            Arg::Long("clean") => self.clean += 1,
            Arg::Long("complete") => self.complete = true,
            Arg::Short('c') => {
                self.complete = true;
                self.clean += 1;
                self.comments = true;
            }
            Arg::Long("install") | Arg::Short('i') => self.install = true,
            Arg::Long("sysupgrade") | Arg::Short('u') => self.sysupgrade = true,
            Arg::Long("refresh") | Arg::Short('y') => self.refresh = true,
            Arg::Long("quiet") | Arg::Short('q') => self.quiet = true,
            Arg::Long("list") | Arg::Short('l') => self.list = true,
            Arg::Long("delete") | Arg::Short('d') => self.delete += 1,

            Arg::Long("print") | Arg::Short('p') => self.print = true,
            Arg::Long("newsonupgrade") => self.news_on_upgrade = true,
            Arg::Long("nonewsonupgrade") => self.news_on_upgrade = false,
            Arg::Long("comments") => self.comments = true,
            Arg::Long("ssh") => self.ssh = true,
            Arg::Long("failfast") => self.fail_fast = true,
            Arg::Long("nofailfast") => self.fail_fast = false,
            Arg::Long("keepsrc") => self.keep_src = true,
            Arg::Long("nokeepsrc") => self.keep_src = false,
            // ops
            Arg::Long("database") | Arg::Short('D') => set_op(Op::Database),
            Arg::Long("files") | Arg::Short('F') => set_op(Op::Files),
            Arg::Long("query") | Arg::Short('Q') => set_op(Op::Query),
            Arg::Long("remove") | Arg::Short('R') => set_op(Op::Remove),
            Arg::Long("sync") | Arg::Short('S') => set_op(Op::Sync),
            Arg::Long("deptest") | Arg::Short('T') => set_op(Op::DepTest),
            Arg::Long("upgrade") | Arg::Short('U') => set_op(Op::Upgrade),
            Arg::Long("show") | Arg::Short('P') => set_op(Op::Show),
            Arg::Long("getpkgbuild") | Arg::Short('G') => set_op(Op::GetPkgBuild),
            Arg::Long("repoctl") | Arg::Short('L') => set_op(Op::RepoCtl),
            Arg::Long("chrootctl") | Arg::Short('C') => set_op(Op::ChrootCtl),
            // globals
            Arg::Long("noconfirm") => self.no_confirm = true,
            Arg::Long("confirm") => self.no_confirm = false,
            Arg::Long("dbpath") | Arg::Short('b') => self.db_path = Some(value?.to_string()),
            Arg::Long("root") | Arg::Short('r') => self.root = Some(value?.to_string()),
            Arg::Long("verbose") | Arg::Short('v') => self.verbose = true,
            Arg::Long("ask") => {
                if let Ok(n) = value?.to_string().parse() {
                    self.ask = n
                }
            }
            Arg::Long("ignore") => self.ignore.extend(value?.split(',').map(|s| s.to_string())),
            Arg::Long("ignoregroup") => self
                .ignore_group
                .extend(value?.split(',').map(|s| s.to_string())),
            Arg::Long("assume-installed") => self.assume_installed.push(value?.to_string()),
            Arg::Long("arch") => self.arch = Some(value?.to_string()),
            Arg::Long("color") => self.color = Colors::from(value.unwrap_or("always")),
            Arg::Long("localrepo") => self.repos = LocalRepos::new(value.ok()),
            Arg::Long("chroot") => {
                self.chroot = true;
                if self.repos == LocalRepos::None {
                    self.repos = LocalRepos::Default;
                }
                if let Ok(p) = value {
                    self.chroot_dir = p.into();
                }
            }
            Arg::Long("nochroot") => self.chroot = false,
            Arg::Long("sign") => {
                self.sign = match value {
                    Ok(k) => Sign::Key(k.to_string()),
                    Err(_) => Sign::Yes,
                }
            }
            Arg::Long("nokeeprepocache") => self.keep_repo_cache = false,
            Arg::Long("keeprepocache") => self.keep_repo_cache = true,
            Arg::Long("signdb") => {
                self.sign_db = match value {
                    Ok(k) => Sign::Key(k.to_string()),
                    Err(_) => Sign::Yes,
                }
            }
            Arg::Long("nosign") => self.sign = Sign::No,
            Arg::Long("nosigndb") => self.sign_db = Sign::No,
            Arg::Long(a) if !arg.is_pacman_arg() && !arg.is_pacman_global() => {
                bail!(tr!("unknown option --{}", a))
            }
            Arg::Short(a) if !arg.is_pacman_arg() && !arg.is_pacman_global() => {
                bail!(tr!("unknown option -{}", a))
            }
            _ => (),
        }

        match takes_value(arg) {
            TakesValue::No if forced => bail!(tr!("option {} does not allow a value", arg)),
            _ => (),
        }

        Ok(())
    }
}

fn split_whitespace(s: &str) -> Vec<String> {
    s.split_whitespace().map(|s| s.to_string()).collect()
}

fn takes_value(arg: Arg) -> TakesValue {
    match arg {
        Arg::Long("aururl") => TakesValue::Required,
        Arg::Long("editor") => TakesValue::Required,
        Arg::Long("makepkg") => TakesValue::Required,
        Arg::Long("pacman") => TakesValue::Required,
        Arg::Long("git") => TakesValue::Required,
        Arg::Long("gpg") => TakesValue::Required,
        Arg::Long("sudo") => TakesValue::Required,
        Arg::Long("asp") => TakesValue::Required,
        Arg::Long("fm") => TakesValue::Required,
        Arg::Long("bat") => TakesValue::Required,
        Arg::Long("makepkgconf") => TakesValue::Required,
        Arg::Long("editorflags") => TakesValue::Required,
        Arg::Long("mflags") => TakesValue::Required,
        Arg::Long("gitflags") => TakesValue::Required,
        Arg::Long("gpgflags") => TakesValue::Required,
        Arg::Long("sudoflags") => TakesValue::Required,
        Arg::Long("batflags") => TakesValue::Required,
        Arg::Long("fmflags") => TakesValue::Required,
        Arg::Long("completioninterval") => TakesValue::Required,
        Arg::Long("sortby") => TakesValue::Required,
        Arg::Long("searchby") => TakesValue::Required,
        Arg::Long("limit") => TakesValue::Required,
        Arg::Long("removemake") => TakesValue::Optional,
        Arg::Long("redownload") => TakesValue::Optional,
        Arg::Long("rebuild") => TakesValue::Optional,
        Arg::Long("sudoloop") => TakesValue::Optional,
        Arg::Long("develsuffixes") => TakesValue::Required,
        Arg::Long("localrepo") => TakesValue::Optional,
        Arg::Long("chroot") => TakesValue::Optional,
        Arg::Long("builddir") => TakesValue::Required,
        Arg::Long("clonedir") => TakesValue::Required,
        //pacman
        Arg::Long("dbpath") | Arg::Short('b') => TakesValue::Required,
        Arg::Long("root") | Arg::Short('r') => TakesValue::Required,
        Arg::Long("ask") => TakesValue::Required,
        Arg::Long("cachedir") => TakesValue::Required,
        Arg::Long("arch") => TakesValue::Required,
        Arg::Long("color") => TakesValue::Required,
        Arg::Long("config") => TakesValue::Required,
        Arg::Long("gpgdir") => TakesValue::Required,
        Arg::Long("hookdir") => TakesValue::Required,
        Arg::Long("logfile") => TakesValue::Required,
        Arg::Long("sysroot") => TakesValue::Required,
        Arg::Long("ignore") => TakesValue::Required,
        Arg::Long("ignoregroup") => TakesValue::Required,
        Arg::Long("assume-installed") => TakesValue::Required,
        Arg::Long("print-format") => TakesValue::Required,
        Arg::Long("overwrite") => TakesValue::Required,
        Arg::Long("sign") => TakesValue::Optional,
        Arg::Long("signdb") => TakesValue::Optional,
        _ => TakesValue::No,
    }
}
