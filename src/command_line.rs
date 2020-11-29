use crate::args::{PACMAN_FLAGS, PACMAN_GLOBALS};
use crate::config::{Colors, Config, LocalRepos};

use std::fmt;

use anyhow::{anyhow, bail, ensure, Context, Result};
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
                    self.handle_arg(arg, Some(chars.as_str()), op_count, false)?;
                    return Ok(true);
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
        let yes_no_ask = &["yes", "no", "ask"];
        let yes_no_all = &["yes", "no", "all"];
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

        match takes_value(arg) {
            TakesValue::Required if value.is_none() => bail!("option {} expects a value", arg),
            TakesValue::No if forced => bail!("option {} does not allow a value", arg),
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

        let mut set_op = |op: &str| {
            self.op = op.into();
            *op_count += 1;
        };

        let value = value.with_context(|| format!("option {} does not allow a value", arg));

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
            Arg::Long("config") => self.pacman_conf = Some(value?.to_string()),

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
            Arg::Long("sortby") => self.sort_by = validate(value?, sort_by)?,
            Arg::Long("searchby") => self.search_by = validate(value?, search_by)?,
            Arg::Long("news") | Arg::Short('w') => self.news += 1,
            Arg::Long("removemake") => {
                self.remove_make = validate(value.unwrap_or("yes"), yes_no_ask)?
            }
            Arg::Long("upgrademenu") => self.upgrade_menu = true,
            Arg::Long("noupgrademenu") => self.upgrade_menu = false,
            Arg::Long("noremovemake") => self.remove_make = "no".to_string(),
            Arg::Long("cleanafter") => self.clean_after = true,
            Arg::Long("nocleanafter") => self.clean_after = false,
            Arg::Long("redownload") => {
                self.redownload = validate(value.unwrap_or("yes"), yes_no_all)?
            }
            Arg::Long("noredownload") => self.redownload = "no".to_string(),
            Arg::Long("rebuild") => self.rebuild = validate(value.unwrap_or("yes"), yes_no_all)?,
            Arg::Long("norebuild") => self.rebuild = "no".into(),
            Arg::Long("topdown") => self.sort_mode = "topdown".to_string(),
            Arg::Long("bottomup") => self.sort_mode = "bottomup".to_string(),
            Arg::Long("aur") | Arg::Short('a') => {
                self.mode = "aur".to_string();
                self.aur_filter = true;
            }
            Arg::Long("repo") => self.mode = "repo".to_string(),
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
            Arg::Long("combinedupgrade") => self.combined_upgrade = true,
            Arg::Long("nocombinedupgrade") => self.combined_upgrade = false,
            Arg::Long("batchinstall") => self.batch_install = true,
            Arg::Long("nobatchinstall") => self.batch_install = false,
            Arg::Long("sudoloop") => self.sudo_loop = true,
            Arg::Long("nosudoloop") => self.sudo_loop = false,
            Arg::Long("clean") => self.clean += 1,
            Arg::Long("complete") => self.complete = true,
            Arg::Short('c') => {
                self.complete = true;
                self.clean += 1;
                self.comments = true;
            }
            Arg::Long("print") | Arg::Short('p') => self.print = true,
            Arg::Long("newsonupgrade") => self.news_on_upgrade = true,
            Arg::Long("comments") => self.comments = true,
            // ops
            Arg::Long("database") | Arg::Short('D') => set_op("database"),
            Arg::Long("files") | Arg::Short('F') => set_op("files"),
            Arg::Long("query") | Arg::Short('Q') => set_op("query"),
            Arg::Long("remove") | Arg::Short('R') => set_op("remove"),
            Arg::Long("sync") | Arg::Short('S') => set_op("sync"),
            Arg::Long("deptest") | Arg::Short('T') => set_op("deptest"),
            Arg::Long("upgrade") | Arg::Short('U') => set_op("upgrade"),
            Arg::Long("show") | Arg::Short('P') => set_op("show"),
            Arg::Long("getpkgbuild") | Arg::Short('G') => set_op("getpkgbuild"),
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
            Arg::Long("arch") => self.arch = Some(value?.to_string()),
            Arg::Long("color") => self.color = Colors::from(value.unwrap_or("always")),
            //TODO
            Arg::Long("localrepo") => self.repos = LocalRepos::new(value.ok()),
            Arg::Long("local") => self.local = true,
            Arg::Long("chroot") => {
                self.chroot = true;
                if self.repos == LocalRepos::None {
                    self.repos = LocalRepos::Default;
                }
            }
            Arg::Long("nochroot") => self.chroot = false,
            Arg::Long(a) if !arg.is_pacman_arg() => bail!("unknown option --{}", a),
            Arg::Short(a) if !arg.is_pacman_arg() => bail!("unknown option -{}", a),
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
        Arg::Long("removemake") => TakesValue::Optional,
        Arg::Long("redownload") => TakesValue::Optional,
        Arg::Long("rebuild") => TakesValue::Optional,
        Arg::Long("develsuffixes") => TakesValue::Required,
        Arg::Long("localrepo") => TakesValue::Optional,
        //pacman
        Arg::Long("dbpath") | Arg::Short('b') => TakesValue::Required,
        Arg::Long("root") | Arg::Short('r') => TakesValue::Required,
        Arg::Long("ask") => TakesValue::Required,
        Arg::Long("arch") => TakesValue::Required,
        Arg::Long("cachedir") => TakesValue::Required,
        Arg::Long("color") => TakesValue::Required,
        Arg::Long("config") => TakesValue::Required,
        Arg::Long("gpgdir") => TakesValue::Required,
        Arg::Long("hookdir") => TakesValue::Required,
        Arg::Long("logfile") => TakesValue::Required,
        Arg::Long("sysroot") => TakesValue::Required,
        Arg::Long("ignore") => TakesValue::Required,
        Arg::Long("ignoregroup") => TakesValue::Required,
        Arg::Long("assumeinstalled") => TakesValue::Required,
        Arg::Long("print-format") => TakesValue::Required,
        Arg::Long("overwrite") => TakesValue::Required,
        _ => TakesValue::No,
    }
}

pub fn validate(key: &str, valid: &[&str]) -> Result<String> {
    ensure!(
        valid.contains(&key),
        "invalid value for '{}', expected: {}",
        key,
        valid.join("|")
    );
    Ok(key.to_string())
}
