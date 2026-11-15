//! Support for an ordered list of AUR-compatible sources.
//!
//! Every source speaks the aurweb `/rpc` API and serves package git repos at
//! `<url>/<pkgbase>.git`. Lookups query all sources and merge the results,
//! the earlier source winning when two provide a package with the same name.
//! The source each package was resolved from is remembered so it can be
//! downloaded from there and labelled with the source's name.

// The mock build replaces the RPC client with crate::mock::Mock.
#![cfg_attr(feature = "mock", allow(dead_code))]

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ansiterm::Style;
use anyhow::{Context, Result, anyhow, bail, ensure};
use async_trait::async_trait;
use raur::{Package, Raur, SearchBy};
use tr::tr;
use url::Url;

type Mutation = Box<dyn Fn(&mut Vec<AurSource>) -> Result<()>>;

/// One AUR-compatible source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AurSource {
    pub name: String,
    pub url: Url,
    pub rpc_url: Option<Url>,
}

impl AurSource {
    pub fn new(name: &str, url: &str, rpc_url: Option<&str>) -> Result<Self> {
        validate_name(name)?;
        let url = parse_base_url(url)?;
        let rpc_url = match rpc_url {
            Some(rpc) if !rpc.is_empty() => {
                Some(Url::parse(rpc).with_context(|| tr!("invalid rpc url '{}'", rpc))?)
            }
            _ => None,
        };
        Ok(Self {
            name: name.to_string(),
            url,
            rpc_url,
        })
    }

    /// Parses the value of an `AurSource = <name> <url> [rpcurl]` directive.
    pub fn from_directive(value: &str) -> Result<Self> {
        let words = value.split_whitespace().collect::<Vec<_>>();
        match words.as_slice() {
            [name, url] => Self::new(name, url, None),
            [name, url, rpc] => Self::new(name, url, Some(rpc)),
            _ => bail!(tr!("expected 'AurSource = <name> <url> [rpcurl]'")),
        }
    }

    /// The value of the `AurSource` directive describing this source.
    pub fn to_directive(&self) -> String {
        match &self.rpc_url {
            Some(rpc) => format!("{} {} {}", self.name, self.url, rpc),
            None => format!("{} {}", self.name, self.url),
        }
    }

    /// The URL of the source's /rpc endpoint.
    pub fn rpc(&self) -> Result<Url> {
        match &self.rpc_url {
            Some(rpc) => Ok(rpc.clone()),
            None => Ok(self.url.join("rpc")?),
        }
    }

    /// The git URL of `pkgbase` on this source. When `ssh_host` matches the
    /// source's host the URL is rewritten to use ssh (`--ssh`).
    pub fn clone_url(&self, pkgbase: &str, ssh_host: Option<&str>) -> Result<Url> {
        let url = self.url.join(&format!("{}.git", pkgbase))?;
        match ssh_host {
            Some(host) if url.scheme() == "https" && url.host_str() == Some(host) => {
                Ok(url.as_str().replacen("https://", "ssh://aur@", 1).parse()?)
            }
            _ => Ok(url),
        }
    }
}

fn validate_name(name: &str) -> Result<()> {
    ensure!(!name.is_empty(), tr!("source name can not be empty"));
    ensure!(
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')),
        tr!(
            "invalid source name '{}' (use letters, digits, '-', '_' or '.')",
            name
        )
    );
    ensure!(
        !name.starts_with(['-', '.']),
        tr!("invalid source name '{}'", name)
    );
    Ok(())
}

/// Parses a source base URL, making sure the path ends in '/' so relative
/// joins (`rpc`, `packages.gz`, `<pkgbase>.git`) stay below it.
pub fn parse_base_url(url: &str) -> Result<Url> {
    let mut url = Url::parse(url).with_context(|| tr!("invalid url '{}'", url))?;
    ensure!(
        !url.cannot_be_a_base(),
        tr!("invalid url '{}'", url.as_str())
    );
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url)
}

/// Builds sources for a list of bare URLs (the `--aururls` flag), naming each
/// after its host. The official AUR is named `aur`.
pub fn sources_from_urls<S: AsRef<str>>(urls: &[S]) -> Result<Vec<AurSource>> {
    let mut sources: Vec<AurSource> = Vec::new();

    for url in urls {
        let url = parse_base_url(url.as_ref())?;
        let host = url.host_str().unwrap_or("source");
        let base = if host == "aur.archlinux.org" {
            "aur".to_string()
        } else {
            host.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                        c
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
                .trim_start_matches(['-', '.'])
                .to_string()
        };
        let base = if base.is_empty() {
            "source".to_string()
        } else {
            base
        };
        let mut name = base.clone();
        let mut n = 2;
        while sources.iter().any(|s| s.name == name) {
            name = format!("{}-{}", base, n);
            n += 1;
        }
        sources.push(AurSource {
            name,
            url,
            rpc_url: None,
        });
    }

    Ok(sources)
}

/// Adds `src` to `list`. A later definition with the same name replaces the
/// earlier one and moves to the end, so a config that includes another file
/// can redefine its sources.
pub fn push_source(list: &mut Vec<AurSource>, src: AurSource) {
    list.retain(|s| s.name != src.name);
    list.push(src);
}

/// Shared map from package name / package base to the index of the source it
/// was resolved from.
pub type Origins = Arc<Mutex<HashMap<String, usize>>>;

/// The label shown before a package from source `idx` of `sources`, e.g.
/// `aur` or `mirror`. The first (default) source and single-source setups are
/// always labelled `aur`.
pub fn label(sources: &[AurSource], idx: Option<usize>) -> &str {
    match idx {
        Some(i) if i > 0 && i < sources.len() => &sources[i].name,
        _ => "aur",
    }
}

pub fn add_source(list: &mut Vec<AurSource>, src: AurSource) -> Result<()> {
    ensure!(
        !list.iter().any(|s| s.name == src.name),
        tr!("an AUR source named '{}' already exists", src.name)
    );
    list.push(src);
    Ok(())
}

/// Applies `key=value` changes (name, url, rpcurl) to a source. Nothing is
/// changed if any of them is invalid.
pub fn edit_source(list: &mut [AurSource], name: &str, changes: &[(String, String)]) -> Result<()> {
    let idx = find(list, name)?;
    let mut src = list[idx].clone();

    for (key, value) in changes {
        match key.as_str() {
            "name" => {
                validate_name(value)?;
                ensure!(
                    value == name || !list.iter().any(|s| &s.name == value),
                    tr!("an AUR source named '{}' already exists", value)
                );
                src.name = value.clone();
            }
            "url" => src.url = parse_base_url(value)?,
            "rpcurl" if value.is_empty() => src.rpc_url = None,
            "rpcurl" => {
                src.rpc_url =
                    Some(Url::parse(value).with_context(|| tr!("invalid rpc url '{}'", value))?)
            }
            _ => bail!(tr!(
                "unknown field '{}' (expected name, url or rpcurl)",
                key
            )),
        }
    }

    list[idx] = src;
    Ok(())
}

pub fn delete_source(list: &mut Vec<AurSource>, name: &str) -> Result<AurSource> {
    let idx = find(list, name)?;
    Ok(list.remove(idx))
}

/// Moves a source to a 1-based position.
pub fn move_source(list: &mut Vec<AurSource>, name: &str, position: usize) -> Result<()> {
    let idx = find(list, name)?;
    ensure!(
        position >= 1 && position <= list.len(),
        tr!("position must be between 1 and {}", list.len())
    );
    let src = list.remove(idx);
    list.insert(position - 1, src);
    Ok(())
}

fn find(list: &[AurSource], name: &str) -> Result<usize> {
    list.iter()
        .position(|s| s.name == name)
        .ok_or_else(|| anyhow!(tr!("no AUR source named '{}'", name)))
}

/// A [`Raur`] implementation that queries several sources and merges the
/// results. When two sources provide a package with the same name the earlier
/// source wins. A failing source is reported and skipped; an error is only
/// returned when every source fails.
#[derive(Debug, Clone)]
pub struct MultiRaur<H> {
    names: Vec<String>,
    handles: Vec<H>,
    origins: Origins,
    warning: Style,
}

impl<H: Default> Default for MultiRaur<H> {
    fn default() -> Self {
        Self::single(H::default())
    }
}

impl<H> MultiRaur<H> {
    /// A handle over a single source; behaves exactly like `handle`.
    pub fn single(handle: H) -> Self {
        Self {
            names: vec!["aur".into()],
            handles: vec![handle],
            origins: Origins::default(),
            warning: Style::new(),
        }
    }

    /// A handle over several sources. `names[i]` names the source queried by
    /// `handles[i]`; `origins` records which source each package came from.
    pub fn new(names: Vec<String>, handles: Vec<H>, origins: Origins, warning: Style) -> Self {
        assert_eq!(names.len(), handles.len());
        assert!(!handles.is_empty());
        Self {
            names,
            handles,
            origins,
            warning,
        }
    }

    #[cfg(test)]
    pub fn origins(&self) -> &Origins {
        &self.origins
    }

    fn merge<E: std::fmt::Display>(
        &self,
        results: Vec<std::result::Result<Vec<Package>, E>>,
    ) -> std::result::Result<Vec<Package>, E> {
        let mut merged = Vec::new();
        let mut seen = HashSet::new();
        let mut first_err = None;
        let mut failed = 0;
        let mut origins = self.origins.lock().unwrap();

        for (i, res) in results.into_iter().enumerate() {
            match res {
                Ok(pkgs) => {
                    for pkg in pkgs {
                        if !seen.insert(pkg.name.clone()) {
                            continue;
                        }
                        for key in [&pkg.name, &pkg.package_base] {
                            let idx = origins.entry(key.clone()).or_insert(i);
                            *idx = (*idx).min(i);
                        }
                        merged.push(pkg);
                    }
                }
                Err(e) => {
                    failed += 1;
                    if self.handles.len() > 1 {
                        eprintln!(
                            "{} {}",
                            self.warning.paint(tr!("warning:")),
                            tr!("AUR source '{}' failed: {}", self.names[i], e)
                        );
                    }
                    first_err.get_or_insert(e);
                }
            }
        }

        if failed == self.handles.len() {
            return Err(first_err.unwrap());
        }

        Ok(merged)
    }
}

impl MultiRaur<raur::Handle> {
    pub fn client(&self) -> &reqwest::Client {
        self.handles[0].client()
    }
}

#[async_trait]
impl<H, E> Raur for MultiRaur<H>
where
    H: Raur<Err = E> + Send + Sync,
    E: std::fmt::Display + Send,
{
    type Err = E;

    async fn raw_info<S: AsRef<str> + Send + Sync>(
        &self,
        pkg_names: &[S],
    ) -> std::result::Result<Vec<Package>, E> {
        if self.handles.len() == 1 {
            let pkgs = self.handles[0].raw_info(pkg_names).await;
            return self.merge(vec![pkgs]);
        }
        let futs = self.handles.iter().map(|h| h.raw_info(pkg_names));
        let results = futures::future::join_all(futs).await;
        self.merge(results)
    }

    async fn search_by<S: AsRef<str> + Send + Sync>(
        &self,
        query: S,
        strategy: SearchBy,
    ) -> std::result::Result<Vec<Package>, E> {
        let query = query.as_ref();
        let futs = self.handles.iter().map(|h| h.search_by(query, strategy));
        let results = futures::future::join_all(futs).await;
        self.merge(results)
    }
}

/// Where `paru --aursource` writes: `$PARU_CONF` if set, otherwise the user
/// config file.
pub fn user_config_path() -> Result<PathBuf> {
    if let Ok(conf) = std::env::var("PARU_CONF") {
        return Ok(PathBuf::from(conf));
    }
    let config =
        dirs::config_dir().ok_or_else(|| anyhow!(tr!("failed to find config directory")))?;
    Ok(config.join("paru").join("paru.conf"))
}

fn is_aursource_line(line: &str) -> bool {
    let line = line.trim();
    if line.starts_with('#') || line.starts_with('[') {
        return false;
    }
    line.split('=').next().map(str::trim) == Some("AurSource")
}

fn section_of(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.starts_with('[') && line.ends_with(']') && line.len() >= 2 {
        Some(&line[1..line.len() - 1])
    } else {
        None
    }
}

/// Returns `contents` with its `AurSource` lines replaced by `sources`.
///
/// Every other line, including comments, is kept as is. The new lines go
/// where the first existing `AurSource` line was, otherwise after the last
/// setting of the first `[options]` section, otherwise in a new `[options]`
/// section at the end.
pub fn rewrite_sources(contents: &str, sources: &[AurSource]) -> String {
    let new_lines = sources
        .iter()
        .map(|s| format!("AurSource = {}", s.to_directive()))
        .collect::<Vec<_>>();

    let mut section: Option<&str> = None;
    let mut insert_at = None;
    let mut options_end = None;
    let mut in_first_options = false;
    let mut out: Vec<String> = Vec::new();

    for line in contents.lines() {
        if let Some(s) = section_of(line) {
            in_first_options = s == "options" && options_end.is_none();
            if in_first_options {
                options_end = Some(out.len() + 1);
            }
            section = Some(s);
            out.push(line.to_string());
            continue;
        }

        if section == Some("options") && is_aursource_line(line) {
            insert_at.get_or_insert(out.len());
            continue;
        }

        out.push(line.to_string());
        if in_first_options && !line.trim().is_empty() {
            options_end = Some(out.len());
        }
    }

    if let Some(idx) = insert_at.or(options_end) {
        out.splice(idx..idx, new_lines);
    } else if !new_lines.is_empty() {
        if out.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        out.push("[options]".into());
        out.extend(new_lines);
    }

    let mut s = out.join("\n");
    if !s.is_empty() {
        s.push('\n');
    }
    s
}

/// Reads the `AurSource` directives in the `[options]` sections of `contents`
/// (without following `Include`s).
#[cfg(test)]
pub fn parse_sources(contents: &str) -> Result<Vec<AurSource>> {
    let mut section = None;
    let mut list = Vec::new();
    for line in contents.lines() {
        if let Some(s) = section_of(line) {
            section = Some(s);
        } else if section == Some("options") && is_aursource_line(line) {
            let value = line.split_once('=').map(|v| v.1.trim()).unwrap_or("");
            push_source(&mut list, AurSource::from_directive(value)?);
        }
    }
    Ok(list)
}

/// Writes `contents` to `path` atomically: the data goes to a temporary file
/// in the same directory which then replaces `path`. On failure `path` is left
/// untouched.
pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    // Write through symlinks rather than replacing them.
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!(tr!("invalid config path '{}'", path.display())))?;
    fs::create_dir_all(dir)
        .with_context(|| tr!("failed to create directory '{}'", dir.display()))?;

    let mut tmp = tempfile::Builder::new()
        .prefix(".paru.conf.")
        .tempfile_in(dir)
        .with_context(|| tr!("failed to create a temporary file in '{}'", dir.display()))?;
    tmp.write_all(contents.as_bytes())?;
    tmp.as_file().sync_all()?;
    if let Ok(meta) = fs::metadata(&path) {
        let _ = fs::set_permissions(tmp.path(), meta.permissions());
    }
    tmp.persist(&path)
        .map_err(|e| e.error)
        .with_context(|| tr!("failed to write '{}'", path.display()))?;
    Ok(())
}

/// Parses a paru config file the way paru would and returns its AUR settings.
fn parse_config(name: &str, contents: &str) -> Result<crate::config::Config> {
    use cini::Ini;
    let mut config = crate::config::Config::default();
    config.parse(Some(name), contents)?;
    Ok(config)
}

fn print_sources(config: &crate::config::Config, sources: &[AurSource], configured: bool) {
    let bold = config.color.bold;
    if !configured {
        println!(
            "{}",
            tr!("No AurSource configured; using AurUrl/AurRpcUrl:")
        );
    }
    for (i, src) in sources.iter().enumerate() {
        print!("{} {} {}", i + 1, bold.paint(&src.name), src.url);
        if let Some(rpc) = &src.rpc_url {
            print!(" {}", tr!("(rpc: {})", rpc));
        }
        println!();
    }
}

const USAGE: &str = "usage: paru --aursource <list|add|edit|delete|move> [args]
    paru --aursource list
    paru --aursource add <name> <url> [rpcurl]
    paru --aursource edit <name> [name=<new>] [url=<url>] [rpcurl=<url>]
    paru --aursource delete <name>
    paru --aursource move <name> <position>";

/// `paru --aursource <action> [args]`: list or edit the AurSource entries of
/// the user's paru.conf.
pub fn command(config: &crate::config::Config) -> Result<i32> {
    let action = config.aur_source_action.as_deref().unwrap_or("list");
    let args = &config.targets;

    let mutate: Mutation = match action {
        "list" | "ls" => {
            ensure!(args.is_empty(), "{}", USAGE);
            print_sources(
                config,
                &config.effective_aur_sources(),
                !config.aur_sources.is_empty(),
            );
            return Ok(0);
        }
        "add" => {
            ensure!(args.len() == 2 || args.len() == 3, "{}", USAGE);
            let src = AurSource::new(&args[0], &args[1], args.get(2).map(|s| s.as_str()))?;
            Box::new(move |list| add_source(list, src.clone()))
        }
        "edit" => {
            ensure!(args.len() >= 2, "{}", USAGE);
            let mut changes = Vec::new();
            for kv in &args[1..] {
                let (k, v) = kv
                    .split_once('=')
                    .ok_or_else(|| anyhow!(tr!("expected key=value, got '{}'", kv)))?;
                changes.push((k.to_string(), v.to_string()));
            }
            let name = args[0].clone();
            Box::new(move |list| edit_source(list, &name, &changes))
        }
        "delete" | "remove" | "rm" => {
            ensure!(args.len() == 1, "{}", USAGE);
            let name = args[0].clone();
            Box::new(move |list| delete_source(list, &name).map(|_| ()))
        }
        "move" | "mv" => {
            ensure!(args.len() == 2, "{}", USAGE);
            let pos = args[1]
                .parse()
                .map_err(|_| anyhow!(tr!("invalid position '{}'", args[1])))?;
            let name = args[0].clone();
            Box::new(move |list| move_source(list, &name, pos))
        }
        _ => bail!(
            "{}\n{}",
            tr!("unknown --aursource action '{}'", action),
            USAGE
        ),
    };

    let path = user_config_path()?;
    let sources = update_config_file(&path, Path::new("/etc/paru.conf"), mutate)?;
    println!("{}", tr!("Saved AUR sources to {}", path.display()));
    let configured = !sources.is_empty();
    let sources = if configured {
        sources
    } else {
        let saved = fs::read_to_string(&path)?;
        parse_config(&path.display().to_string(), &saved)?.effective_aur_sources()
    };
    print_sources(config, &sources, configured);
    Ok(0)
}

/// Applies `mutate` to the AurSource list of the config file at `path` and
/// saves it, keeping every other line. If `path` does not exist it is created
/// and, when `system` exists, includes it so its settings still apply.
///
/// When no list is configured yet, the list starts with the current
/// AurUrl/AurRpcUrl named `aur`, so adding a source keeps the AUR. Nothing is
/// written if `mutate` fails or the result does not parse back to the same
/// list. Returns the new list.
pub fn update_config_file(
    path: &Path,
    system: &Path,
    mutate: impl Fn(&mut Vec<AurSource>) -> Result<()>,
) -> Result<Vec<AurSource>> {
    let name = path.display().to_string();
    let contents = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if system.exists() {
                format!(
                    "# {}\nInclude = {}\n",
                    tr!("Created by paru --aursource. The system config is included below."),
                    system.display()
                )
            } else {
                String::new()
            }
        }
        Err(e) => {
            return Err(e).with_context(|| tr!("failed to read '{}'", path.display()));
        }
    };

    let current = parse_config(&name, &contents)?;
    let mut list = current.aur_sources.clone();
    if list.is_empty() {
        list = current.effective_aur_sources();
        list[0].name = "aur".into();
    }

    mutate(&mut list)?;

    let new_contents = rewrite_sources(&contents, &list);
    let check = parse_config(&name, &new_contents)?;
    ensure!(
        check.aur_sources == list,
        tr!(
            "can not save AUR sources: some are defined in a file included by '{}'; edit that file instead",
            path.display()
        )
    );

    write_atomic(path, &new_contents)?;
    Ok(list)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn src(name: &str, url: &str) -> AurSource {
        AurSource::new(name, url, None).unwrap()
    }

    pub(crate) fn pkg(name: &str, base: &str, version: &str) -> Package {
        Package {
            id: 0,
            name: name.into(),
            package_base_id: 0,
            package_base: base.into(),
            version: version.into(),
            description: None,
            url: None,
            num_votes: 0,
            popularity: 0.0,
            out_of_date: None,
            maintainer: None,
            first_submitted: 0,
            last_modified: 0,
            url_path: String::new(),
            groups: vec![],
            depends: vec![],
            make_depends: vec![],
            opt_depends: vec![],
            check_depends: vec![],
            conflicts: vec![],
            replaces: vec![],
            provides: vec![],
            license: vec![],
            keywords: vec![],
            co_maintainers: vec![],
            submitter: None,
        }
    }

    #[derive(Debug, Clone)]
    pub(crate) struct Fake(pub Option<Vec<Package>>);

    #[async_trait]
    impl Raur for Fake {
        type Err = raur::Error;

        async fn raw_info<S: AsRef<str> + Send + Sync>(
            &self,
            names: &[S],
        ) -> std::result::Result<Vec<Package>, raur::Error> {
            let pkgs = self.0.clone().ok_or(raur::Error::Aur("down".into()))?;
            Ok(pkgs
                .into_iter()
                .filter(|p| names.iter().any(|n| n.as_ref() == p.name))
                .collect())
        }

        async fn search_by<S: AsRef<str> + Send + Sync>(
            &self,
            query: S,
            _: SearchBy,
        ) -> std::result::Result<Vec<Package>, raur::Error> {
            let pkgs = self.0.clone().ok_or(raur::Error::Aur("down".into()))?;
            Ok(pkgs
                .into_iter()
                .filter(|p| p.name.contains(query.as_ref()))
                .collect())
        }
    }

    fn multi(handles: Vec<Fake>) -> MultiRaur<Fake> {
        let names = (0..handles.len()).map(|i| format!("s{}", i)).collect();
        MultiRaur::new(names, handles, Origins::default(), Style::new())
    }

    #[test]
    fn directive_round_trip() {
        let s = AurSource::from_directive("taur https://taur.example.org").unwrap();
        assert_eq!(s.name, "taur");
        assert_eq!(s.url.as_str(), "https://taur.example.org/");
        assert_eq!(s.rpc().unwrap().as_str(), "https://taur.example.org/rpc");
        assert_eq!(
            s.clone_url("foo", None).unwrap().as_str(),
            "https://taur.example.org/foo.git"
        );

        let s = AurSource::from_directive("m https://h/aur https://h/api/rpc").unwrap();
        assert_eq!(s.url.as_str(), "https://h/aur/");
        assert_eq!(s.rpc().unwrap().as_str(), "https://h/api/rpc");
        assert_eq!(
            s.clone_url("foo", None).unwrap().as_str(),
            "https://h/aur/foo.git"
        );
        assert_eq!(AurSource::from_directive(&s.to_directive()).unwrap(), s);

        assert!(AurSource::from_directive("onlyname").is_err());
        assert!(AurSource::from_directive("a b c d").is_err());
        assert!(AurSource::from_directive("bad/name https://x").is_err());
        assert!(AurSource::from_directive("ok not-a-url").is_err());
    }

    #[test]
    fn ssh_only_for_matching_host() {
        let aur = src("aur", "https://aur.archlinux.org");
        let other = src("t", "https://t.example");
        let host = Some("aur.archlinux.org");
        assert_eq!(
            aur.clone_url("foo", host).unwrap().as_str(),
            "ssh://aur@aur.archlinux.org/foo.git"
        );
        assert_eq!(
            other.clone_url("foo", host).unwrap().as_str(),
            "https://t.example/foo.git"
        );
    }

    #[test]
    fn names_from_urls() {
        let s = sources_from_urls(&[
            "https://aur.archlinux.org",
            "http://127.0.0.1:8765",
            "https://taur.example/a",
            "https://taur.example/b",
        ])
        .unwrap();
        let names = s.iter().map(|s| s.name.as_str()).collect::<Vec<_>>();
        assert_eq!(
            names,
            ["aur", "127.0.0.1", "taur.example", "taur.example-2"]
        );
        assert!(sources_from_urls(&["nope"]).is_err());
    }

    #[test]
    fn labels() {
        let list = vec![src("aur", "https://a"), src("mirror", "https://m")];
        assert_eq!(label(&list, None), "aur");
        assert_eq!(label(&list, Some(0)), "aur");
        assert_eq!(label(&list, Some(1)), "mirror");
        assert_eq!(label(&list, Some(7)), "aur");
        // The first source is the default and is shown as aur whatever its name.
        let list = vec![src("main", "https://a"), src("mirror", "https://m")];
        assert_eq!(label(&list, Some(0)), "aur");
        assert_eq!(label(&[], Some(1)), "aur");
    }

    #[test]
    fn list_management() {
        let mut list = vec![src("aur", "https://aur.archlinux.org")];
        add_source(&mut list, src("taur", "https://taur.example")).unwrap();
        assert!(add_source(&mut list, src("taur", "https://x.example")).is_err());
        add_source(&mut list, src("third", "https://third.example")).unwrap();

        move_source(&mut list, "third", 1).unwrap();
        let names = |l: &[AurSource]| l.iter().map(|s| s.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&list), ["third", "aur", "taur"]);
        assert!(move_source(&mut list, "third", 0).is_err());
        assert!(move_source(&mut list, "third", 4).is_err());
        assert!(move_source(&mut list, "missing", 1).is_err());

        edit_source(
            &mut list,
            "taur",
            &[
                ("name".into(), "t".into()),
                ("url".into(), "https://new.example".into()),
                ("rpcurl".into(), "https://new.example/api/rpc".into()),
            ],
        )
        .unwrap();
        assert_eq!(list[2].name, "t");
        assert_eq!(list[2].url.as_str(), "https://new.example/");
        assert_eq!(
            list[2].rpc().unwrap().as_str(),
            "https://new.example/api/rpc"
        );
        edit_source(&mut list, "t", &[("rpcurl".into(), "".into())]).unwrap();
        assert_eq!(list[2].rpc_url, None);
        assert!(edit_source(&mut list, "t", &[("name".into(), "aur".into())]).is_err());
        assert!(edit_source(&mut list, "t", &[("bogus".into(), "x".into())]).is_err());
        // A failed edit changes nothing.
        assert!(
            edit_source(
                &mut list,
                "t",
                &[("name".into(), "u".into()), ("url".into(), "bad".into())]
            )
            .is_err()
        );
        assert_eq!(list[2].name, "t");

        delete_source(&mut list, "aur").unwrap();
        assert_eq!(names(&list), ["third", "t"]);
        assert!(delete_source(&mut list, "aur").is_err());
    }

    #[test]
    fn redefinition_replaces() {
        let mut list = Vec::new();
        push_source(&mut list, src("a", "https://a"));
        push_source(&mut list, src("b", "https://b"));
        push_source(&mut list, src("a", "https://a2"));
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].url.as_str(), "https://a2/");
    }

    #[tokio::test]
    async fn merge_prefers_earlier_source() {
        let raur = multi(vec![
            Fake(Some(vec![pkg("yay", "yay", "12-1")])),
            Fake(Some(vec![
                pkg("yay", "yay", "0.1-1"),
                pkg("only-t", "t-base", "1-1"),
            ])),
        ]);
        let pkgs = raur.info(&["yay", "only-t"]).await.unwrap();
        assert_eq!(pkgs.len(), 2);
        assert_eq!(
            pkgs.iter().find(|p| p.name == "yay").unwrap().version,
            "12-1"
        );

        let origins = raur.origins().lock().unwrap();
        assert_eq!(origins["yay"], 0);
        assert_eq!(origins["only-t"], 1);
        assert_eq!(origins["t-base"], 1);
    }

    #[tokio::test]
    async fn failed_source_is_skipped() {
        let raur = multi(vec![Fake(None), Fake(Some(vec![pkg("foo", "foo", "1-1")]))]);
        let pkgs = raur.search("foo").await.unwrap();
        assert_eq!(pkgs.len(), 1);
        assert_eq!(raur.origins().lock().unwrap()["foo"], 1);

        let raur = multi(vec![Fake(None), Fake(None)]);
        assert!(raur.search("foo").await.is_err());
    }

    #[test]
    fn rewrite_keeps_comments_and_order() {
        let conf = "# my config\n[options]\nBottomUp\n# keep me\nAurSource = old https://old\nDevel\nAurSource = old2 https://old2\n\n[bin]\nSudo = doas\n";
        let new = vec![
            src("aur", "https://aur.archlinux.org"),
            src("t", "https://t"),
        ];
        let out = rewrite_sources(conf, &new);
        assert_eq!(
            out,
            "# my config\n[options]\nBottomUp\n# keep me\nAurSource = aur https://aur.archlinux.org/\nAurSource = t https://t/\nDevel\n\n[bin]\nSudo = doas\n"
        );
        assert_eq!(parse_sources(&out).unwrap(), new);

        // No AurSource lines yet: after the last setting of [options].
        let conf = "[options]\nBottomUp\n\n[bin]\nSudo = doas\n";
        let out = rewrite_sources(conf, &new[1..]);
        assert_eq!(
            out,
            "[options]\nBottomUp\nAurSource = t https://t/\n\n[bin]\nSudo = doas\n"
        );

        // No [options] section.
        let out = rewrite_sources("Include = /etc/paru.conf\n", &new[1..]);
        assert_eq!(
            out,
            "Include = /etc/paru.conf\n\n[options]\nAurSource = t https://t/\n"
        );

        // Removing every source leaves everything else alone.
        let conf = "[options]\n#AurSource = commented https://c\nAurSource = t https://t/\nDevel\n";
        assert_eq!(
            rewrite_sources(conf, &[]),
            "[options]\n#AurSource = commented https://c\nDevel\n"
        );
        // AurSource in another section is not touched.
        assert_eq!(
            parse_sources("[repo]\nAurSource = a https://a\n").unwrap(),
            vec![]
        );
    }

    #[test]
    fn atomic_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/paru.conf");
        write_atomic(&path, "a\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "a\n");
        write_atomic(&path, "b\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "b\n");
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    fn conf_sources(c: &crate::config::Config) -> Vec<String> {
        c.effective_aur_sources()
            .iter()
            .map(|s| format!("{} {}", s.name, s.url))
            .collect()
    }

    #[test]
    fn config_parsing_and_backward_compat() {
        // Without AurSource lines AurUrl/AurRpcUrl are the single source.
        let c = parse_config("t", "[options]\nAurUrl = https://mirror.example\n").unwrap();
        assert!(c.aur_sources.is_empty());
        assert_eq!(c.aur_url.as_str(), "https://mirror.example/");
        assert_eq!(conf_sources(&c), ["aur https://mirror.example/"]);
        assert_eq!(c.aur_label("anything"), "aur");

        let c = parse_config(
            "t",
            "[options]\nAurSource = aur https://aur.archlinux.org\nAurSource = mirror https://m.example/aur\n",
        )
        .unwrap();
        assert_eq!(
            conf_sources(&c),
            [
                "aur https://aur.archlinux.org/",
                "mirror https://m.example/aur/"
            ]
        );
        assert!(parse_config("t", "[options]\nAurSource = broken\n").is_err());
    }

    #[test]
    fn cli_flags() {
        let mut c = parse_config(
            "t",
            "[options]\nAurSource = aur https://aur.archlinux.org\nAurSource = mirror https://m.example\n",
        )
        .unwrap();
        c.parse_args([
            "-Ss",
            "--aururls",
            "https://a.example,https://b.example",
            "--aururls=https://c.example",
            "foo",
        ])
        .unwrap();
        assert_eq!(c.aur_urls_arg.len(), 3);

        let mut c = parse_config(
            "t",
            "[options]\nAurSource = aur https://aur.archlinux.org\nAurSource = mirror https://m.example\n",
        )
        .unwrap();
        c.parse_args(["-Ss", "--aururl", "https://only.example", "foo"])
            .unwrap();
        assert!(c.aur_sources.is_empty());
        assert_eq!(conf_sources(&c), ["aur https://only.example/"]);

        let mut c = crate::config::Config::default();
        c.parse_args(["--aursource", "add", "taur", "https://taur.example"])
            .unwrap();
        assert_eq!(c.aur_source_action.as_deref(), Some("add"));
        assert_eq!(c.targets, ["taur", "https://taur.example"]);
    }

    #[test]
    fn config_labels_and_urls() {
        let mut c = parse_config(
            "t",
            "[options]\nAurSource = aur https://aur.archlinux.org\nAurSource = mirror https://m.example/aur\n",
        )
        .unwrap();
        c.aur_ssh_host = Some("aur.archlinux.org".into());
        {
            let mut o = c.aur_origins.lock().unwrap();
            o.insert("foo".into(), 1);
            o.insert("foo-base".into(), 1);
            o.insert("bar".into(), 0);
        }
        assert_eq!(c.aur_label("foo"), "mirror");
        assert_eq!(c.aur_label("bar"), "aur");
        assert_eq!(c.aur_label("unknown"), "aur");
        assert!(c.is_aur_source_label("mirror"));
        assert!(!c.is_aur_source_label("aur"));
        assert!(!c.is_aur_source_label("core"));
        assert_eq!(
            c.aur_package_url("foo", "foo").unwrap().as_str(),
            "https://m.example/aur/packages/foo"
        );
        let urls = |c: &crate::config::Config, p| {
            c.aur_clone_urls(p)
                .unwrap()
                .iter()
                .map(|u| u.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            urls(&c, "foo-base"),
            [
                "https://m.example/aur/foo-base.git",
                "https://aur.archlinux.org/foo-base.git"
            ]
        );
        assert_eq!(
            urls(&c, "bar"),
            [
                "https://aur.archlinux.org/bar.git",
                "https://m.example/aur/bar.git"
            ]
        );
        c.ssh = true;
        assert_eq!(
            urls(&c, "bar"),
            [
                "ssh://aur@aur.archlinux.org/bar.git",
                "https://m.example/aur/bar.git"
            ]
        );
    }

    #[test]
    fn update_file() {
        let dir = tempfile::tempdir().unwrap();
        let system = dir.path().join("etc-paru.conf");
        fs::write(&system, "[options]\nBottomUp\n").unwrap();
        let path = dir.path().join("user/paru.conf");

        // Creating the user config keeps the system one via Include and the
        // AUR as the first source.
        let list = update_config_file(&path, &system, |l| {
            add_source(l, src("taur", "https://taur.example"))
        })
        .unwrap();
        assert_eq!(list.len(), 2);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(&format!("Include = {}", system.display())));
        assert!(text.contains(
            "AurSource = aur https://aur.archlinux.org/\nAurSource = taur https://taur.example/\n"
        ));
        let c = parse_config("t", &text).unwrap();
        assert!(!c.sort_mode.is_top_down(false));

        // Edits keep other lines.
        let custom = "# mine\n[options]\nDevel\nAurUrl = https://aur.archlinux.org\nAurSource = aur https://aur.archlinux.org/\nAurSource = taur https://taur.example/\n# trailing\n[bin]\nSudo = doas\n";
        fs::write(&path, custom).unwrap();
        update_config_file(&path, &system, |l| move_source(l, "taur", 1)).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            custom.replace(
                "AurSource = aur https://aur.archlinux.org/\nAurSource = taur https://taur.example/",
                "AurSource = taur https://taur.example/\nAurSource = aur https://aur.archlinux.org/"
            )
        );

        // A failing change leaves the file untouched.
        let before = fs::read_to_string(&path).unwrap();
        assert!(
            update_config_file(&path, &system, |l| delete_source(l, "nope").map(|_| ())).is_err()
        );
        assert!(
            update_config_file(&path, &system, |l| add_source(l, src("taur", "https://x")))
                .is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), before);

        // Deleting everything falls back to AurUrl.
        update_config_file(&path, &system, |l| {
            l.clear();
            Ok(())
        })
        .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("AurSource ="));
        assert!(text.contains("# trailing") && text.contains("Sudo = doas"));

        // Sources defined in an included file can not be removed from here.
        fs::write(
            &system,
            "[options]\nAurSource = aur https://aur.archlinux.org\nAurSource = sys https://sys.example\n",
        )
        .unwrap();
        fs::write(&path, format!("Include = {}\n", system.display())).unwrap();
        let before = fs::read_to_string(&path).unwrap();
        assert!(
            update_config_file(&path, &system, |l| delete_source(l, "sys").map(|_| ())).is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }
}
