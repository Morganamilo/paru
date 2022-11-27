use std::collections::HashMap;
use std::env::var;
use std::path::Path;
use std::result::Result as StdResult;

use anyhow::Result;
use async_trait::async_trait;
use raur::{Error, Package, Raur, SearchBy};
use srcinfo::Srcinfo;

#[derive(Debug, Default, Clone)]
pub struct Mock {
    pkgs: HashMap<String, Package>,
}

impl Mock {
    pub fn new() -> Result<Self> {
        let mut mock = Mock {
            pkgs: HashMap::new(),
        };

        let clone = Path::new(&var("CARGO_MANIFEST_DIR").unwrap()).join("testdata/clone");

        for dir in std::fs::read_dir(clone)? {
            let dir = dir?;

            let srcinfo = Srcinfo::parse_file(dir.path().join(".SRCINFO"))?;
            let base = srcinfo.base;

            for pkg in srcinfo.pkgs {
                let name = pkg.pkgname;
                let depends = pkg
                    .depends
                    .into_iter()
                    .find(|v| v.arch.is_none())
                    .map(|v| v.vec)
                    .unwrap_or_default();
                let make_depends = base
                    .makedepends
                    .iter()
                    .find(|v| v.arch.is_none())
                    .map(|v| v.vec.clone())
                    .unwrap_or_default();
                let check_depends = base
                    .checkdepends
                    .iter()
                    .find(|v| v.arch.is_none())
                    .map(|v| v.vec.clone())
                    .unwrap_or_default();

                let pkg = Package {
                    id: 0,
                    name: name.clone(),
                    package_base_id: 0,
                    package_base: base.pkgbase.clone(),
                    version: format!("{}-{}", base.pkgver, base.pkgrel),
                    description: None,
                    url: None,
                    num_votes: 0,
                    popularity: 0.0,
                    out_of_date: None,
                    maintainer: None,
                    first_submitted: 0,
                    last_modified: 0,
                    url_path: "".into(),
                    groups: vec![],
                    depends,
                    make_depends,
                    opt_depends: vec![],
                    check_depends,
                    conflicts: vec![],
                    replaces: vec![],
                    provides: vec![],
                    license: vec![],
                    keywords: vec![],
                    co_maintainers: vec![],
                    submitter: None,
                };

                mock.pkgs.insert(name, pkg);
            }
        }
        Ok(mock)
    }

    pub fn client(&self) -> reqwest::Client {
        reqwest::Client::new()
    }
}

#[async_trait]
impl Raur for Mock {
    type Err = raur::Error;

    async fn raw_info<S: AsRef<str> + Send + Sync>(
        &self,
        pkgs: &[S],
    ) -> StdResult<Vec<Package>, Error> {
        let mut ret = Vec::new();

        for pkg in pkgs {
            if let Some(pkg) = self.pkgs.get(pkg.as_ref()) {
                ret.push(pkg.clone());
            }
        }

        Ok(ret)
    }

    async fn search_by<S: AsRef<str> + Send + Sync>(
        &self,
        _pkg: S,
        _by: SearchBy,
    ) -> StdResult<Vec<Package>, Error> {
        unimplemented!()
    }
}
