use alpm::{Dep, Depend, SigLevel, Version};
use alpm_utils::depends::{
    satisfies_dep, satisfies_dep_nover, satisfies_provide, satisfies_provide_nover,
};
use anyhow::{Context, Result};
use aur_depends::{Actions, DependencySatisfier};
use srcinfo::Srcinfo;
use std::collections::{HashMap, HashSet};

/// Provider preferences snapshot each consumer's choices during resolution.
/// They select usable archives, not mandatory successful rebuilds.
#[derive(Debug, Default)]
pub(crate) struct DependencyPlan {
    selections: HashMap<String, Vec<(Depend, DependencySatisfier)>>,
}

enum ArchivePreference<'a> {
    Selected(&'a str),
    Defer,
    Unplanned,
}

impl DependencyPlan {
    pub(crate) fn from_actions(actions: &Actions<'_>) -> Self {
        let mut plan = Self::default();
        for selection in actions.resolved_dependencies() {
            plan.selections
                .entry(selection.consumer().to_owned())
                .or_default()
                .push((
                    selection.dependency().to_depend(),
                    selection.satisfier().clone(),
                ));
        }
        plan
    }

    fn preference(&self, requirement: &Requirement) -> ArchivePreference<'_> {
        let Some(selections) = self.selections.get(&requirement.consumer) else {
            return ArchivePreference::Unplanned;
        };
        if let Some((_, satisfier)) = selections
            .iter()
            .find(|(dependency, _)| dependency.to_string() == requirement.dependency.to_string())
        {
            return match satisfier {
                DependencySatisfier::Build(name) => ArchivePreference::Selected(name),
                _ => ArchivePreference::Defer,
            };
        }
        // Archive metadata can add a version to a dependency declared without one.
        // Preserve a unique preference and check the actual archive against that version.
        let mut matching = selections
            .iter()
            .filter(|(dependency, _)| dependency.name() == requirement.dependency.name());
        if let Some(name) = matching.clone().find_map(|(_, source)| match source {
            DependencySatisfier::Build(name) => Some(name),
            _ => None,
        }) {
            // Selection origins can differ while retaining the same provider identity.
            return if matching.all(|(_, source)| match source {
                DependencySatisfier::Build(other)
                | DependencySatisfier::Repository(other)
                | DependencySatisfier::Installed(other) => other == name,
                DependencySatisfier::Assumed => false,
            }) {
                ArchivePreference::Selected(name)
            } else {
                ArchivePreference::Defer
            };
        }
        let Some((_, satisfier)) = matching.next() else {
            return ArchivePreference::Unplanned;
        };
        if matching.any(|(_, other)| other != satisfier) {
            return ArchivePreference::Defer;
        }
        // External satisfaction was established only for the original declaration.
        ArchivePreference::Unplanned
    }
}

#[derive(Debug)]
pub(crate) struct Requirement {
    consumer: String,
    dependency: Depend,
}

impl Requirement {
    fn new(consumer: &str, dependency: &Dep) -> Self {
        Self {
            consumer: consumer.to_owned(),
            dependency: dependency.to_depend(),
        }
    }
}

/// Archives available for injection into later chroot builds.
#[derive(Debug, Default)]
pub(crate) struct Artifacts {
    packages: Vec<BuiltArtifact>,
}

impl Artifacts {
    pub(crate) fn record<'a>(
        &mut self,
        alpm: &alpm::Alpm,
        paths: impl Iterator<Item = &'a str>,
    ) -> Result<()> {
        let mut seen = HashSet::new();
        let packages = paths
            .filter(|path| seen.insert(*path))
            .map(|path| BuiltArtifact::load(alpm, path))
            .collect::<Result<Vec<_>>>()?;
        for package in packages {
            self.packages.retain(|old| old.name != package.name);
            self.packages.push(package);
        }
        Ok(())
    }

    pub(crate) fn select<'a>(
        &'a self,
        requirements: &[Requirement],
        plan: &DependencyPlan,
        current_packages: &[&str],
        ignore_version: bool,
    ) -> Vec<&'a str> {
        select_chroot_artifacts(
            &self.packages,
            requirements,
            plan,
            current_packages,
            ignore_version,
        )
    }
}

#[derive(Debug)]
struct BuiltArtifact {
    path: String,
    name: String,
    version: Version,
    provides: Vec<Depend>,
    depends: Vec<Depend>,
}

impl BuiltArtifact {
    fn load(alpm: &alpm::Alpm, path: &str) -> Result<Self> {
        let package = alpm
            .pkg_load(path.as_bytes(), false, SigLevel::NONE)
            .with_context(|| format!("load built package {path}"))?;
        Ok(Self {
            path: path.to_owned(),
            name: package.name().to_owned(),
            version: Version::new(package.version().as_str()),
            provides: package
                .provides()
                .iter()
                .map(|dep| dep.to_depend())
                .collect(),
            depends: package
                .depends()
                .iter()
                .map(|dep| dep.to_depend())
                .collect(),
        })
    }

    fn satisfies(&self, dependency: &Dep, ignore_version: bool) -> bool {
        if ignore_version {
            satisfies_dep_nover(dependency, &self.name)
                || self
                    .provides
                    .iter()
                    .any(|provide| satisfies_provide_nover(dependency, provide))
        } else {
            satisfies_dep(dependency, &self.name, &self.version)
                || self
                    .provides
                    .iter()
                    .any(|provide| satisfies_provide(dependency, provide))
        }
    }
}

/// Build all split outputs using the global and package-specific declarations.
pub(crate) fn dependencies(srcinfo: &Srcinfo, arch: &str, check: bool) -> Vec<Requirement> {
    srcinfo
        .pkgs
        .iter()
        .flat_map(|package| {
            srcinfo
                .pkg
                .depends
                .arch(arch)
                .chain(srcinfo.base.makedepends.arch(arch))
                .chain(srcinfo.base.checkdepends.arch(arch).filter(|_| check))
                .chain(package.depends.arch(arch))
                .map(|dependency| Requirement::new(&package.pkgname, &Depend::new(dependency)))
        })
        .collect()
}

fn select_chroot_artifacts<'a>(
    artifacts: &'a [BuiltArtifact],
    requirements: &[Requirement],
    plan: &DependencyPlan,
    current_packages: &[&str],
    ignore_version: bool,
) -> Vec<&'a str> {
    let mut selected = HashSet::new();
    let mut pending = requirements
        .iter()
        .map(|requirement| Requirement::new(&requirement.consumer, &requirement.dependency))
        .collect::<Vec<_>>();
    while let Some(requirement) = pending.pop() {
        let candidate = match plan.preference(&requirement) {
            ArchivePreference::Selected(name) => {
                if current_packages.contains(&name) {
                    continue;
                }
                // A planned build may fail. Available archives supplement normal
                // chroot resolution; a missing archive must not block that resolution.
                let Some(candidate) = artifacts
                    .iter()
                    .enumerate()
                    .find(|(_, artifact)| artifact.name == name)
                else {
                    continue;
                };
                if !candidate
                    .1
                    .satisfies(&requirement.dependency, ignore_version)
                {
                    continue;
                }
                Some(candidate)
            }
            ArchivePreference::Defer => continue,
            ArchivePreference::Unplanned => {
                // Changed SRCINFO and archive declarations may be absent from the
                // plan. Accept a unique satisfier; never make another provider choice.
                let candidates = artifacts
                    .iter()
                    .enumerate()
                    .filter(|(_, artifact)| {
                        artifact.satisfies(&requirement.dependency, ignore_version)
                    })
                    .collect::<Vec<_>>();
                if candidates.len() > 1 {
                    None
                } else {
                    candidates.first().copied()
                }
            }
        };
        let Some((index, artifact)) = candidate else {
            continue;
        };
        if selected.insert(index) {
            pending.extend(
                artifact
                    .depends
                    .iter()
                    .map(|dependency| Requirement::new(&artifact.name, dependency)),
            );
        }
    }
    artifacts
        .iter()
        .enumerate()
        .filter(|(index, _)| selected.contains(index))
        .map(|(_, artifact)| artifact.path.as_str())
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{BuiltArtifact, DependencyPlan, Requirement};
    use std::collections::HashMap;

    fn select(
        artifacts: &[BuiltArtifact],
        requirements: &[Depend],
        choices: &HashMap<String, String>,
        ignore_version: bool,
    ) -> Vec<String> {
        let mut plan = DependencyPlan::default();
        plan.selections.insert(
            "consumer".to_owned(),
            choices
                .iter()
                .map(|(dep, name)| {
                    (
                        Depend::new(dep.as_str()),
                        aur_depends::DependencySatisfier::Build(name.clone()),
                    )
                })
                .collect(),
        );
        let requirements = requirements
            .iter()
            .map(|dep| Requirement::new("consumer", dep))
            .collect::<Vec<_>>();
        super::select_chroot_artifacts(artifacts, &requirements, &plan, &[], ignore_version)
            .into_iter()
            .map(str::to_owned)
            .collect()
    }
    use alpm::{Depend, Version};

    fn artifact(
        path: &str,
        name: &str,
        version: &str,
        provides: &[&str],
        depends: &[&str],
    ) -> BuiltArtifact {
        BuiltArtifact {
            path: path.to_owned(),
            name: name.to_owned(),
            version: Version::new(version),
            provides: provides.iter().map(|dep| Depend::new(*dep)).collect(),
            depends: depends.iter().map(|dep| Depend::new(*dep)).collect(),
        }
    }

    #[test]
    fn selects_only_the_required_artifact_closure() {
        let artifacts = vec![
            artifact("/pkg/foundation.pkg.tar.zst", "foundation", "1-1", &[], &[]),
            artifact(
                "/pkg/dependent.pkg.tar.zst",
                "dependent",
                "1-1",
                &[],
                &["foundation>=1"],
            ),
            artifact("/pkg/unrelated.pkg.tar.zst", "unrelated", "1-1", &[], &[]),
        ];
        let requirements = vec![Depend::new("dependent>=1")];

        assert_eq!(
            select(&artifacts, &requirements, &Default::default(), false),
            vec!["/pkg/foundation.pkg.tar.zst", "/pkg/dependent.pkg.tar.zst",]
        );
    }

    #[test]
    fn uses_the_resolved_package_name_with_multiple_providers() {
        let artifacts = vec![
            artifact("/pkg/foo.pkg.tar.zst", "foo", "1-1", &[], &[]),
            artifact(
                "/pkg/foo-git.pkg.tar.zst",
                "foo-git",
                "1-1",
                &["foo=1"],
                &[],
            ),
        ];
        let requirements = vec![Depend::new("foo>=1")];
        let choices = [("foo>=1".to_owned(), "foo".to_owned())]
            .into_iter()
            .collect();

        assert_eq!(
            select(&artifacts, &requirements, &choices, false),
            vec!["/pkg/foo.pkg.tar.zst"]
        );
    }

    #[test]
    fn selects_a_versioned_provider() {
        let artifacts = vec![artifact(
            "/pkg/provider.pkg.tar.zst",
            "provider",
            "1-1",
            &["virtual-dependency=2"],
            &[],
        )];
        let requirements = vec![Depend::new("virtual-dependency>=2")];

        assert_eq!(
            select(&artifacts, &requirements, &Default::default(), false),
            vec!["/pkg/provider.pkg.tar.zst"]
        );
    }

    #[test]
    fn excludes_artifacts_that_do_not_satisfy_the_required_version() {
        let artifacts = vec![artifact("/pkg/foo.pkg.tar.zst", "foo", "1-1", &[], &[])];
        let requirements = vec![Depend::new("foo>=2")];

        assert!(select(&artifacts, &requirements, &Default::default(), false).is_empty());
    }

    #[test]
    fn preserves_global_build_dependencies_with_package_overrides() {
        let srcinfo: srcinfo::Srcinfo = "pkgbase = example\n pkgver = 1\n pkgrel = 1\n arch = any\n depends = build-library\npkgname = example\n depends = runtime-library\n".parse().unwrap();
        let dependencies = super::dependencies(&srcinfo, "x86_64", true);
        assert!(
            dependencies
                .iter()
                .any(|dep| dep.dependency.name() == "build-library"),
            "global PKGBUILD dependency was omitted: {dependencies:?}"
        );
    }

    #[test]
    fn reuses_a_provider_already_required_by_name() {
        let artifacts = vec![
            artifact("/pkg/p1", "p1", "1", &["virtual"], &[]),
            artifact("/pkg/p2", "p2", "1", &["virtual"], &[]),
        ];
        let requirements = vec![Depend::new("virtual"), Depend::new("p1")];
        assert_eq!(
            select(&artifacts, &requirements, &Default::default(), false),
            vec!["/pkg/p1"]
        );
    }

    #[test]
    fn preserves_automatic_provider_after_a_named_target() {
        let artifacts = vec![
            artifact(
                "/pkg/provider-installed",
                "provider-installed",
                "1-1",
                &["virtual-provider"],
                &[],
            ),
            artifact("/pkg/virtual-provider", "virtual-provider", "1-1", &[], &[]),
        ];
        let choices = [(
            "virtual-provider".to_owned(),
            "provider-installed".to_owned(),
        )]
        .into_iter()
        .collect();
        assert_eq!(
            select(
                &artifacts,
                &[Depend::new("virtual-provider")],
                &choices,
                false
            ),
            vec!["/pkg/provider-installed"]
        );
    }

    #[test]
    fn ignores_versions_when_nodeps_is_requested() {
        let artifacts = vec![artifact("/pkg/foo.pkg.tar.zst", "foo", "1-1", &[], &[])];
        let requirements = vec![Depend::new("foo>=2")];

        assert_eq!(
            select(&artifacts, &requirements, &Default::default(), true),
            vec!["/pkg/foo.pkg.tar.zst"]
        );
    }

    #[test]
    fn filters_architecture_and_check_dependencies() {
        let srcinfo: srcinfo::Srcinfo = "pkgbase = example\n pkgver = 1\n pkgrel = 1\n arch = x86_64\n arch = aarch64\n makedepends = build-tool\n makedepends_x86_64 = native-tool\n makedepends_aarch64 = other-tool\n checkdepends = test-tool\npkgname = example\n depends = library\npkgname = example-extra\n depends = extra-library\n".parse().unwrap();
        let names = |check| {
            super::dependencies(&srcinfo, "x86_64", check)
                .iter()
                .map(|dep| dep.dependency.name().to_owned())
                .collect::<std::collections::HashSet<_>>()
        };
        assert_eq!(
            names(false),
            ["build-tool", "native-tool", "library", "extra-library"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        );
        let mut checked = names(false);
        checked.insert("test-tool".to_owned());
        assert_eq!(names(true), checked);
    }

    #[test]
    fn retains_the_resolvers_provider_choice() {
        let artifacts = vec![
            artifact("/pkg/p1", "p1", "1", &["virtual=2"], &[]),
            artifact("/pkg/p2", "p2", "1", &["virtual=2"], &[]),
        ];
        let choices = [("virtual>=2".to_owned(), "p2".to_owned())]
            .into_iter()
            .collect();
        assert_eq!(
            select(&artifacts, &[Depend::new("virtual>=2")], &choices, false),
            vec!["/pkg/p2"]
        );
    }

    #[test]
    fn leaves_incompatible_selected_archives_to_chroot_resolution() {
        // Arrange
        let artifacts = vec![artifact("/pkg/p2", "p2", "1", &["virtual=1"], &[])];
        let mut plan = DependencyPlan::default();
        plan.selections.insert(
            "consumer".to_owned(),
            vec![(
                Depend::new("virtual>=2"),
                aur_depends::DependencySatisfier::Build("p2".to_owned()),
            )],
        );
        // Act
        let selected = super::select_chroot_artifacts(
            &artifacts,
            &[Requirement::new("consumer", &Depend::new("virtual>=2"))],
            &plan,
            &[],
            false,
        );

        // Assert
        assert!(selected.is_empty());
    }

    #[test]
    fn terminates_for_cyclic_runtime_dependencies() {
        let artifacts = vec![
            artifact("/pkg/a", "a", "1", &[], &["b"]),
            artifact("/pkg/b", "b", "1", &[], &["a"]),
        ];
        assert_eq!(
            select(&artifacts, &[Depend::new("a")], &Default::default(), false),
            vec!["/pkg/a", "/pkg/b"]
        );
    }

    #[test]
    fn keeps_each_consumers_provider_selection() {
        let artifacts = vec![
            artifact("/pkg/p1", "p1", "1", &["virtual"], &[]),
            artifact("/pkg/p2", "p2", "1", &["virtual"], &[]),
            artifact("/pkg/parent", "parent", "1", &[], &["virtual"]),
        ];
        let mut plan = DependencyPlan::default();
        for (consumer, provider) in [("consumer", "p1"), ("parent", "p2")] {
            plan.selections.insert(
                consumer.to_owned(),
                vec![(
                    Depend::new("virtual"),
                    aur_depends::DependencySatisfier::Build(provider.to_owned()),
                )],
            );
        }
        let requirements = [
            Requirement::new("consumer", &Depend::new("virtual")),
            Requirement::new("consumer", &Depend::new("parent")),
        ];
        assert_eq!(
            super::select_chroot_artifacts(&artifacts, &requirements, &plan, &[], false),
            vec!["/pkg/p1", "/pkg/p2", "/pkg/parent"]
        );
    }

    #[test]
    fn excludes_build_archives_for_dependencies_supplied_elsewhere() {
        let artifacts = vec![artifact(
            "/pkg/provider",
            "provider",
            "1",
            &["virtual"],
            &[],
        )];
        for source in [
            aur_depends::DependencySatisfier::Repository("provider".to_owned()),
            aur_depends::DependencySatisfier::Installed("provider".to_owned()),
            aur_depends::DependencySatisfier::Assumed,
        ] {
            let mut plan = DependencyPlan::default();
            plan.selections.insert(
                "consumer".to_owned(),
                vec![(Depend::new("virtual"), source)],
            );
            assert!(super::select_chroot_artifacts(
                &artifacts,
                &[Requirement::new("consumer", &Depend::new("virtual"))],
                &plan,
                &[],
                false
            )
            .is_empty());
        }
    }

    #[test]
    fn uses_available_archives_for_changed_external_requirements() {
        // Arrange: the old external decision does not resolve the archive's new constraint.
        let artifacts = vec![
            artifact("/pkg/provider", "provider", "2", &["virtual=2"], &[]),
            artifact("/pkg/parent", "parent", "1", &[], &["virtual>=2"]),
        ];
        for source in [
            aur_depends::DependencySatisfier::Repository("provider".to_owned()),
            aur_depends::DependencySatisfier::Installed("provider".to_owned()),
            aur_depends::DependencySatisfier::Assumed,
        ] {
            let mut plan = DependencyPlan::default();
            plan.selections.insert(
                "parent".to_owned(),
                vec![(Depend::new("virtual>=1"), source)],
            );
            let requirements = [Requirement::new("consumer", &Depend::new("parent"))];

            // Act
            let selected =
                super::select_chroot_artifacts(&artifacts, &requirements, &plan, &[], false);

            // Assert
            assert_eq!(selected, vec!["/pkg/provider", "/pkg/parent"]);
        }
    }

    #[test]
    fn leaves_unavailable_selected_archives_to_chroot_resolution() {
        // Arrange
        let artifacts = vec![artifact("/pkg/other", "other", "1", &["virtual=2"], &[])];
        let mut plan = DependencyPlan::default();
        plan.selections.insert(
            "consumer".to_owned(),
            vec![(
                Depend::new("virtual>=2"),
                aur_depends::DependencySatisfier::Build("failed-provider".to_owned()),
            )],
        );
        let requirements = [Requirement::new("consumer", &Depend::new("virtual>=2"))];

        // Act
        let selected = super::select_chroot_artifacts(&artifacts, &requirements, &plan, &[], false);

        // Assert
        assert!(selected.is_empty());
    }

    #[test]
    fn leaves_ambiguous_unplanned_providers_to_chroot_resolution() {
        // Arrange
        let artifacts = vec![
            artifact("/pkg/p1", "p1", "1", &["virtual"], &[]),
            artifact("/pkg/p2", "p2", "1", &["virtual"], &[]),
        ];
        let requirements = [Requirement::new("consumer", &Depend::new("virtual"))];

        // Act
        let selected = super::select_chroot_artifacts(
            &artifacts,
            &requirements,
            &DependencyPlan::default(),
            &[],
            false,
        );

        // Assert
        assert!(selected.is_empty());
    }

    #[test]
    fn leaves_ambiguous_changed_declarations_to_chroot_resolution() {
        // Arrange: only an unselected third provider satisfies the changed declaration.
        let artifacts = vec![artifact("/pkg/p3", "p3", "1", &["virtual=3"], &[])];
        let mut plan = DependencyPlan::default();
        plan.selections.insert(
            "consumer".to_owned(),
            vec![
                (
                    Depend::new("virtual=1"),
                    aur_depends::DependencySatisfier::Build("p1".to_owned()),
                ),
                (
                    Depend::new("virtual=2"),
                    aur_depends::DependencySatisfier::Build("p2".to_owned()),
                ),
            ],
        );
        let requirements = [Requirement::new("consumer", &Depend::new("virtual>=3"))];

        // Act
        let selected = super::select_chroot_artifacts(&artifacts, &requirements, &plan, &[], false);

        // Assert
        assert!(selected.is_empty());
    }

    #[test]
    fn retains_build_hints_across_same_provider_origins() {
        // Arrange: different origins name the same provider for earlier constraints.
        let artifacts = vec![
            artifact("/pkg/provider", "provider", "3", &["virtual=3"], &[]),
            artifact("/pkg/parent", "parent", "1", &[], &["virtual>=3"]),
        ];
        for source in [
            aur_depends::DependencySatisfier::Installed("provider".to_owned()),
            aur_depends::DependencySatisfier::Repository("provider".to_owned()),
        ] {
            for reverse in [false, true] {
                let mut hints = vec![
                    (
                        Depend::new("virtual>=1"),
                        aur_depends::DependencySatisfier::Build("provider".to_owned()),
                    ),
                    (Depend::new("virtual>=2"), source.clone()),
                ];
                if reverse {
                    hints.reverse();
                }
                let mut plan = DependencyPlan::default();
                plan.selections.insert("parent".to_owned(), hints);
                let requirements = [Requirement::new("consumer", &Depend::new("parent"))];

                // Act
                let selected =
                    super::select_chroot_artifacts(&artifacts, &requirements, &plan, &[], false);

                // Assert
                assert_eq!(selected, vec!["/pkg/provider", "/pkg/parent"]);
            }
        }
    }

    #[test]
    fn defers_changed_build_hints_with_conflicting_sources() {
        // Arrange: availability must not override a conflicting identity or assumption.
        let artifacts = vec![artifact("/pkg/p1", "p1", "3", &["virtual=3"], &[])];
        for source in [
            aur_depends::DependencySatisfier::Build("p2".to_owned()),
            aur_depends::DependencySatisfier::Repository("p2".to_owned()),
            aur_depends::DependencySatisfier::Installed("p2".to_owned()),
            aur_depends::DependencySatisfier::Assumed,
        ] {
            let mut plan = DependencyPlan::default();
            plan.selections.insert(
                "consumer".to_owned(),
                vec![
                    (
                        Depend::new("virtual>=1"),
                        aur_depends::DependencySatisfier::Build("p1".to_owned()),
                    ),
                    (Depend::new("virtual>=2"), source),
                ],
            );
            let requirements = [Requirement::new("consumer", &Depend::new("virtual>=3"))];

            // Act
            let selected =
                super::select_chroot_artifacts(&artifacts, &requirements, &plan, &[], false);

            // Assert
            assert!(selected.is_empty());
        }
    }

    #[test]
    fn defers_changed_build_hints_without_usable_preferred_archives() {
        // Arrange: an unselected provider remains usable in both failure cases.
        for available in [false, true] {
            let mut artifacts = vec![artifact("/pkg/p3", "p3", "3", &["virtual=3"], &[])];
            if available {
                artifacts.push(artifact("/pkg/p1", "p1", "2", &["virtual=2"], &[]));
            }
            let mut plan = DependencyPlan::default();
            plan.selections.insert(
                "consumer".to_owned(),
                vec![
                    (
                        Depend::new("virtual>=1"),
                        aur_depends::DependencySatisfier::Build("p1".to_owned()),
                    ),
                    (
                        Depend::new("virtual>=2"),
                        aur_depends::DependencySatisfier::Installed("p1".to_owned()),
                    ),
                ],
            );
            let requirements = [Requirement::new("consumer", &Depend::new("virtual>=3"))];

            // Act
            let selected =
                super::select_chroot_artifacts(&artifacts, &requirements, &plan, &[], false);

            // Assert
            assert!(selected.is_empty());
        }
    }

    #[test]
    fn leaves_conflicting_external_preferences_to_chroot_resolution() {
        // Arrange: external identities also matter when a declaration changes.
        let artifacts = vec![artifact("/pkg/p3", "p3", "1", &["virtual=3"], &[])];
        let mut plan = DependencyPlan::default();
        plan.selections.insert(
            "consumer".to_owned(),
            vec![
                (
                    Depend::new("virtual=1"),
                    aur_depends::DependencySatisfier::Repository("p1".to_owned()),
                ),
                (
                    Depend::new("virtual=2"),
                    aur_depends::DependencySatisfier::Installed("p2".to_owned()),
                ),
            ],
        );
        let requirements = [Requirement::new("consumer", &Depend::new("virtual>=3"))];

        // Act
        let selected = super::select_chroot_artifacts(&artifacts, &requirements, &plan, &[], false);

        // Assert
        assert!(selected.is_empty());
    }

    #[test]
    fn omits_outputs_of_the_current_base() {
        // Arrange
        let artifacts = vec![artifact("/pkg/sibling", "sibling", "1", &[], &[])];
        let mut plan = DependencyPlan::default();
        plan.selections.insert(
            "consumer".to_owned(),
            vec![(
                Depend::new("sibling"),
                aur_depends::DependencySatisfier::Build("sibling".to_owned()),
            )],
        );
        let requirements = [Requirement::new("consumer", &Depend::new("sibling"))];

        // Act
        let selected = super::select_chroot_artifacts(
            &artifacts,
            &requirements,
            &plan,
            &["consumer", "sibling"],
            false,
        );

        // Assert
        assert!(selected.is_empty());
    }

    #[cfg(feature = "mock")]
    #[tokio::test]
    async fn automatic_resolver_selection_reaches_archive_injection() {
        struct Fixture {
            packages: HashMap<String, raur::Package>,
        }
        #[async_trait::async_trait]
        impl raur::Raur for Fixture {
            type Err = raur::Error;
            async fn raw_info<S: AsRef<str> + Send + Sync>(
                &self,
                names: &[S],
            ) -> std::result::Result<Vec<raur::Package>, Self::Err> {
                Ok(names
                    .iter()
                    .filter_map(|name| self.packages.get(name.as_ref()).cloned())
                    .collect())
            }
            async fn search_by<S: AsRef<str> + Send + Sync>(
                &self,
                name: S,
                _by: raur::SearchBy,
            ) -> std::result::Result<Vec<raur::Package>, Self::Err> {
                Ok(self
                    .packages
                    .values()
                    .filter(|package| {
                        package
                            .provides
                            .iter()
                            .any(|provide| Depend::new(provide.as_str()).name() == name.as_ref())
                    })
                    .cloned()
                    .collect())
            }
        }
        let (tmp, mut config) = config();
        let installed = tmp.path().join("local/provider-installed-1-1");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::write(
            installed.join("desc"),
            "%NAME%\nprovider-installed\n\n%VERSION%\n1-1\n\n%PROVIDES%\nvirtual-provider\n",
        )
        .unwrap();
        config.init_alpm().unwrap();
        let mut packages = HashMap::new();
        for name in [
            "warmup",
            "virtual-provider",
            "provider-consumer",
            "provider-installed",
            "foundation",
            "build-tool",
        ] {
            packages.insert(
                name.to_owned(),
                raur::Package {
                    name: name.to_owned(),
                    package_base: name.to_owned(),
                    version: "1-1".to_owned(),
                    ..Default::default()
                },
            );
        }
        for name in ["warmup", "provider-consumer"] {
            packages
                .get_mut(name)
                .unwrap()
                .depends
                .push("virtual-provider".to_owned());
        }
        let provider = packages.get_mut("provider-installed").unwrap();
        provider.provides.push("virtual-provider".to_owned());
        provider.depends.push("foundation".to_owned());
        provider.make_depends.push("build-tool".to_owned());
        let raur = Fixture { packages };
        let mut cache = raur::Cache::new();
        cache.insert(raur.packages["provider-installed"].clone().into());
        let actions = aur_depends::Resolver::new(
            &config.alpm,
            &mut cache,
            &raur,
            aur_depends::Flags::new() | aur_depends::Flags::RESOLVE_SATISFIED_PKGBUILDS,
        )
        .provider_callback(|_, _| 0)
        .resolve_targets(&["warmup", "virtual-provider", "provider-consumer"])
        .await
        .unwrap();
        assert!(actions.missing.is_empty());
        let plan = DependencyPlan::from_actions(&actions);
        let mut artifacts = super::Artifacts::default();
        for package in actions
            .iter_aur_pkgs()
            .take_while(|package| package.pkg.name != "provider-consumer")
        {
            let provides = package
                .pkg
                .provides
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            let runtime = if package.pkg.name == "provider-installed" {
                vec!["foundation>=1"]
            } else {
                Vec::new()
            };
            let path = archive(tmp.path(), &package.pkg.name, "1-1", &provides, &runtime);
            artifacts
                .record(&config.alpm, [path.as_str()].into_iter())
                .unwrap();
        }
        let srcinfo: srcinfo::Srcinfo = "pkgbase = provider-consumer\n pkgver = 1\n pkgrel = 1\n arch = any\n depends = virtual-provider\npkgname = provider-consumer\n".parse().unwrap();
        let selected = artifacts.select(
            &super::dependencies(&srcinfo, "x86_64", true),
            &plan,
            &["provider-consumer"],
            false,
        );
        let names = selected
            .iter()
            .map(|path| {
                std::path::Path::new(path)
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "foundation-1-1-any.pkg.tar",
                "provider-installed-1-1-any.pkg.tar"
            ]
        );
    }

    pub(crate) fn config() -> (tempfile::TempDir, crate::config::Config) {
        let tmp = tempfile::tempdir().unwrap();
        let mut config = crate::config::Config::default();
        config.pacman.root_dir = "/".to_owned();
        config.pacman.db_path = tmp.path().to_string_lossy().into_owned();
        config.init_alpm().unwrap();
        config.alpm.add_architecture("x86_64").unwrap();
        (tmp, config)
    }

    pub(crate) fn archive(
        dir: &std::path::Path,
        name: &str,
        version: &str,
        provides: &[&str],
        depends: &[&str],
    ) -> String {
        let staging = tempfile::tempdir().unwrap();
        let mut info = format!("pkgname = {name}\npkgver = {version}\npkgdesc = fixture\nurl = https://example.com\nbuilddate = 1\npackager = fixture\nsize = 1\narch = any\n");
        for provide in provides {
            info.push_str(&format!("provides = {provide}\n"));
        }
        for dependency in depends {
            info.push_str(&format!("depend = {dependency}\n"));
        }
        std::fs::write(staging.path().join(".PKGINFO"), info).unwrap();
        let path = dir.join(format!("{name}-{version}-any.pkg.tar"));
        let status = std::process::Command::new("tar")
            .arg("-cf")
            .arg(&path)
            .arg("-C")
            .arg(staging.path())
            .arg(".PKGINFO")
            .status()
            .unwrap();
        assert!(status.success());
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn loads_archive_metadata_and_deduplicates_packages() {
        let (tmp, config) = config();
        let a = archive(
            tmp.path(),
            "provider",
            "1-1",
            &["virtual=2"],
            &["foundation"],
        );
        let b = archive(tmp.path(), "foundation", "1-1", &[], &[]);
        let mut artifacts = super::Artifacts::default();
        artifacts
            .record(
                &config.alpm,
                [a.as_str(), a.as_str(), b.as_str()].into_iter(),
            )
            .unwrap();
        assert_eq!(artifacts.packages.len(), 2);
        assert_eq!(
            artifacts.select(
                &[Requirement::new("consumer", &Depend::new("virtual>=2"))],
                &Default::default(),
                &[],
                false
            ),
            vec![a.as_str(), b.as_str()]
        );
        let replacement = archive(tmp.path(), "provider", "2-1", &["virtual=3"], &[]);
        artifacts
            .record(&config.alpm, [replacement.as_str()].into_iter())
            .unwrap();
        assert_eq!(artifacts.packages.len(), 2);
        assert_eq!(
            artifacts.select(
                &[Requirement::new("consumer", &Depend::new("virtual>=3"))],
                &Default::default(),
                &[],
                false
            ),
            vec![replacement.as_str()]
        );
    }
}
