use alpm_utils::Targ;
use std::fmt::{Display, Formatter, Result};

pub static PACMAN_FLAGS: &[&str] = &[
    "disable-download-timeout",
    "sysroot",
    "d",
    "nodeps",
    "assume-installed",
    "dbonly",
    "absdir",
    "noprogressbar",
    "noscriptlet",
    "p",
    "print",
    "print-format",
    "asdeps",
    "asdep",
    "asexplicit",
    "asexp",
    "ignore",
    "ignoregroup",
    "needed",
    "overwrite",
    "f",
    "force",
    "c",
    "changelog",
    "deps",
    "e",
    "explicit",
    "g",
    "groups",
    "i",
    "info",
    "k",
    "check",
    "l",
    "list",
    "m",
    "foreign",
    "n",
    "native",
    "o",
    "owns",
    "file",
    "q",
    "quiet",
    "s",
    "search",
    "t",
    "unrequired",
    "u",
    "upgrades",
    "cascade",
    "nosave",
    "recursive",
    "unneeded",
    "clean",
    "optional",
    "sysupgrade",
    "w",
    "downloadonly",
    "y",
    "refresh",
    "x",
    "regex",
    "machinereadable",
];

pub static PACMAN_GLOBALS: &[&str] = &[
    "b",
    "dbpath",
    "r",
    "root",
    "v",
    "verbose",
    "ask",
    "arch",
    "cachedir",
    "color",
    "config",
    "debug",
    "gpgdir",
    "hookdir",
    "logfile",
    "disable-download-timeout",
    "sysroot",
    "noconfirm",
    "confirm",
    "h",
    "help",
];

#[derive(Default, Debug, Clone)]
pub struct Arg<S> {
    pub key: S,
    pub value: Option<S>,
}

#[derive(Default, Debug, Clone)]
pub struct Args<S> {
    pub bin: S,
    pub op: S,
    pub args: Vec<Arg<S>>,
    pub targets: Vec<S>,
}

impl<S: AsRef<str>> Display for Arg<S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        if self.key.as_ref().len() == 1 {
            f.write_str("-")?;
        } else {
            f.write_str("--")?;
        }

        f.write_str(self.key.as_ref())?;

        if let Some(ref value) = self.value {
            if self.key.as_ref().len() != 1 {
                f.write_str("=")?;
            }
            f.write_str(value.as_ref())?;
        }

        Ok(())
    }
}

impl<S: AsRef<str>> Arg<S> {
    pub fn as_str(&self) -> Arg<&str> {
        let value = self.value.as_ref().map(|v| v.as_ref());
        Arg {
            key: self.key.as_ref(),
            value,
        }
    }
}

impl<S: AsRef<str>> Args<S> {
    pub fn args(&self) -> Vec<String> {
        let op = format!("--{}", self.op.as_ref());
        let mut args = vec![op];
        args.extend(self.args.iter().map(|a| a.to_string()));
        args.push("--".into());
        args.extend(self.targets.iter().map(|s| s.as_ref().to_string()));
        args
    }

    pub fn has_arg(&self, s1: &str, s2: &str) -> bool {
        self.args
            .iter()
            .any(|a| a.key.as_ref() == s1 || a.key.as_ref() == s2)
    }

    pub fn count(&self, s1: &str, s2: &str) -> usize {
        self.args
            .iter()
            .filter(|a| a.key.as_ref() == s1 || a.key.as_ref() == s2)
            .count()
    }

    pub fn op(&mut self, op: S) -> &mut Self {
        self.op = op;
        self
    }

    pub fn remove<T: AsRef<str>>(&mut self, arg: T) -> &mut Self {
        self.args.retain(|v| v.key.as_ref() != arg.as_ref());
        self
    }

    pub fn target(&mut self, target: S) {
        self.targets.push(target);
    }

    pub fn targets(&mut self, targets: impl IntoIterator<Item = S>) {
        targets.into_iter().for_each(|target| self.target(target));
    }

    pub fn arg(&mut self, arg: S) -> &mut Self {
        let arg = Arg {
            key: arg,
            value: None,
        };
        self.args.push(arg);
        self
    }

    pub fn push_value(&mut self, arg: S, value: S) {
        self.push(arg, Some(value));
    }

    pub fn push(&mut self, arg: S, value: Option<S>) {
        let arg = Arg { key: arg, value };
        self.args.push(arg);
    }

    pub fn as_str(&self) -> Args<&str> {
        Args {
            bin: self.bin.as_ref(),
            op: self.op.as_ref(),
            args: self.args.iter().map(|s| s.as_str()).collect(),
            targets: self.targets.iter().map(|s| s.as_ref()).collect(),
        }
    }
}

pub fn parse_targets(targets: &[String]) -> Vec<Targ<'_>> {
    targets.iter().map(|t| Targ::from(t.as_str())).collect()
}
