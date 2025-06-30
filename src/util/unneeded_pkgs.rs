use std::{cell::Cell, cmp::Ordering, collections::HashMap};

use alpm::{AlpmList, Dep, DepMod, PackageReason, Ver};
use alpm_utils::DbListExt;

use crate::config::Config;

type PkgName<'a> = &'a str;

/// Removal state of a package
#[derive(Hash, PartialEq, Eq, Default, Copy, Clone)]
enum State {
    /// This package should be kept (probably explicitely installed or used by an explicitely installed package)
    Keep { deps: DepState },

    /// This package should be removed
    #[default]
    Remove,
}

/// Traversal state of a package's dependencies.
#[derive(Hash, PartialEq, Eq, Copy, Clone)]
enum DepState {
    /// All dependencies have already been traversed and marked [`State::Keep`]
    AlreadyMarked,

    /// The package has already been marked [`State::Keep`] but its dependencies haven't been marked yet
    NotMarkedYet,
}

// A provider of a dependency
#[derive(Debug)]
struct Provider<'a> {
    // Name of the package that provides some dependency
    pkg_name: PkgName<'a>,

    // Version of the dependency that package provides
    ver: Option<&'a Ver>,
}

pub fn unneeded_pkgs(config: &Config, keep_make: bool, keep_optional: bool) -> Vec<PkgName<'_>> {
    // Removal state of each package on the system
    let mut states: HashMap<PkgName, Cell<State>> = HashMap::new();

    // A list of every provided dependency.
    // Maps from provided dependency name -> provided dependency version and the name of the package that provides it.
    let mut providers: HashMap<PkgName, Vec<Provider>> = HashMap::new();
    let db = config.alpm.localdb();

    // Iterate over all packages and populate `states` and `providers`
    for pkg in db.pkgs() {
        // Add pkg as a provider of pkg
        let provider = Provider {
            pkg_name: pkg.name(),
            ver: Some(pkg.version()),
        };
        providers.entry(pkg.name()).or_default().push(provider);

        // Add pkg as a provider of all dependencies that pkg provides
        for dep in pkg.provides() {
            let provider = Provider {
                pkg_name: pkg.name(),
                ver: dep.version(),
            };

            providers.entry(dep.name()).or_default().push(provider);
        }

        // By default, mark every explicitely installed package to be kept, and every dependency to be removed
        if pkg.reason() == PackageReason::Explicit {
            states.insert(
                pkg.name(),
                Cell::new(State::Keep {
                    deps: DepState::NotMarkedYet,
                }),
            );
        } else {
            states.insert(pkg.name(), Cell::new(State::Remove));
        }
    }

    // Go through all packages that are marked to be kept and mark their dependencies as also needed to be kept
    let mut every_dependency_checked = false;
    while !every_dependency_checked {
        // assume we checked every dependency
        // this is set to false if we found some that haven't been checked yet
        every_dependency_checked = true;

        // Iterate over all packages that need their dependencies to be marked to keep, and mark them to keep.
        // At the start these are only those that have been explicitely installed but later can also be the dependencies of them and their dependencies.
        // This also means that packages that were installed as dependencies but then never reaced through .depends() and friends are left to be removed
        for (&pkg, state) in states.iter().filter(|(_, state)| {
            state.get()
                == State::Keep {
                    deps: DepState::NotMarkedYet,
                }
        }) {
            let pkg = db.pkg(pkg).unwrap();

            state.set(State::Keep {
                deps: DepState::AlreadyMarked,
            });

            mark_deps_as_kept(
                pkg.depends(),
                &providers,
                &states,
                &mut every_dependency_checked,
            );

            if keep_optional {
                mark_deps_as_kept(
                    pkg.optdepends(),
                    &providers,
                    &states,
                    &mut every_dependency_checked,
                );
            }

            if keep_make {
                continue;
            }

            if config.alpm.syncdbs().pkg(pkg.name()).is_err() {
                mark_deps_as_kept(
                    pkg.makedepends(),
                    &providers,
                    &states,
                    &mut every_dependency_checked,
                );
                mark_deps_as_kept(
                    pkg.checkdepends(),
                    &providers,
                    &states,
                    &mut every_dependency_checked,
                );
            }
        }
    }

    states
        .into_iter()
        .filter_map(|(pkg, state)| {
            if state.get() == State::Remove {
                Some(pkg)
            } else {
                None
            }
        })
        .collect()
}

/// Marks all dependency providers that satisfy the dependencies of this package requires with [`State::Keep`]
fn mark_deps_as_kept(
    dependencies: AlpmList<&Dep>,
    providers: &HashMap<PkgName, Vec<Provider>>,
    states: &HashMap<PkgName, Cell<State>>,
    every_dependency_marked: &mut bool,
) {
    for dep in dependencies {
        // mark all providers that satisfy the dependency as to be kept
        for provider in providers
            .get(dep.name())
            .into_iter()
            .flatten()
            .filter(|provider| provider_satisfies_dependency(dep, provider))
        {
            let state = states
                .get(provider.pkg_name)
                .expect("state should have all packages");

            // if the dependency hasn't already been recursively marked as to kept, mark it, and edit the marking loop to go through this package's dependencies, too.
            if state.get()
                != (State::Keep {
                    deps: DepState::AlreadyMarked,
                })
            {
                state.set(State::Keep {
                    deps: DepState::NotMarkedYet,
                });
                *every_dependency_marked = false;
            }
        }
    }
}

/// Checks if the provider of the dependency satisfies its version requirements
fn provider_satisfies_dependency(dep: &Dep, provider: &Provider) -> bool {
    let Some(required_version) = dep.version() else {
        // dependency doesn't depend on any particular version
        return true;
    };

    let Some(provided_version) = provider.ver else {
        // provider doesn't specify what version it provides but we depend on a particular version
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
}
