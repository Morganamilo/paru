use std::{cell::Cell, cmp::Ordering, collections::HashMap};

use alpm::{DepMod, PackageReason, Ver};
use alpm_utils::DbListExt;

use crate::config::Config;

#[derive(Hash, PartialEq, Eq, Default, Copy, Clone)]
enum State {
    #[default]
    Remove,
    /// The same as keep but whose dependencies need to be checked first
    CheckDeps,
    Keep,
}

// A provider of a dependency
#[derive(Debug)]
struct Provider<'a> {
    // Name of the package that provides some dependency
    name: &'a str,

    // Version of the dependency that package provides
    ver: Option<&'a Ver>,
}

pub fn unneeded_pkgs(config: &Config, keep_make: bool, keep_optional: bool) -> Vec<&str> {
    let mut states = HashMap::new();
    let mut remove = Vec::new();
    let mut providers: HashMap<&str, Vec<Provider>> = HashMap::new();
    let db = config.alpm.localdb();

    // iterate over all packages
    for pkg in db.pkgs() {
        // add self as a provider of self
        let provider = Provider {
            name: pkg.name(),
            ver: Some(pkg.version()),
        };
        providers.entry(pkg.name()).or_default().push(provider);

        // add self as a provider of all packages that it "provides"
        for dep in pkg.provides() {
            let provider = Provider {
                name: pkg.name(),
                ver: dep.version(),
            };

            providers.entry(dep.name()).or_default().push(provider);
        }

        // mark the package to be removed if not explicitely installed
        if pkg.reason() == PackageReason::Explicit {
            states.insert(pkg.name(), Cell::new(State::CheckDeps));
        } else {
            states.insert(pkg.name(), Cell::new(State::Remove));
        }
    }

    // now
    // states contains names of pkgs mapped to -> checkdeps if explicit, remove otherwise
    // providers contains a list of all package providers

    let mut again = true;
    while again {
        again = false;

        // marks all providers of the dependency this package requires as "CheckDeps"
        let mut check_deps = |deps: alpm::AlpmList<&alpm::Dep>| {
            for dep in deps {
                // get all providers of the dependency dep
                for provider in providers
                    .get(dep.name())
                    .into_iter()
                    .flatten()
                    .filter(|provider| {
                        let Some(required_version) = dep.version() else {
                            // pass through all providers if the package doesn't depend on a particular version
                            return true;
                        };

                        let Some(provided_version) = provider.ver else {
                            // the provider doesn't specify what version it provides but we depend on a particular version
                            return false;
                        };

                        let ver_cmp = provided_version.vercmp(required_version);
                        let ver_requirement = dep.depmod();
                        match (ver_cmp, ver_requirement) {
                            // Note: I don't believe this should ever be hit because a version requirement ~does~ exist
                            // (because dep.version() is Some)
                            (_, DepMod::Any) => true,
                            (Ordering::Less, DepMod::Lt | DepMod::Le) => true,
                            (Ordering::Equal, DepMod::Eq | DepMod::Le | DepMod::Ge) => true,
                            (Ordering::Greater, DepMod::Gt | DepMod::Ge) => true,
                            _ => false,
                        }
                    })
                {
                    let state = states.get(provider.name).unwrap();

                    // if the dependency isn't marked to keep (probably marked to remove?), then mark to check its dependencies instead
                    // this means this package is needed
                    if state.get() != State::Keep {
                        state.set(State::CheckDeps);
                        again = true;
                    }
                }
            }
        };

        // iterate over all packages that need their dependencies to be checked, and mark them to keep
        // at the start these are only those that have been explicitely installed but later can also be the dependencies of them and their dependencies
        // this also means that packages that were installed as dependencies but then never reaced through .depends() and friends are left to be removed
        for (&pkg, state) in &states {
            if state.get() != State::CheckDeps {
                continue;
            }

            if let Ok(pkg) = db.pkg(pkg) {
                state.set(State::Keep);
                check_deps(pkg.depends());

                if keep_optional {
                    check_deps(pkg.optdepends());
                }

                if keep_make {
                    continue;
                }

                if config.alpm.syncdbs().pkg(pkg.name()).is_err() {
                    check_deps(pkg.makedepends());
                    check_deps(pkg.checkdepends());
                }
            }
        }
    }

    for pkg in db.pkgs() {
        if states.get(pkg.name()).unwrap().get() == State::Remove {
            remove.push(pkg.name());
        }
    }

    remove
}
