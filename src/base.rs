use crate::{AurPackage, CustomPackage};

use std::fmt::{Display, Formatter};

enum PkgNames<A, C> {
    Aur(A),
    Custom(C),
}

impl<'a, A, C> Iterator for PkgNames<A, C>
where
    A: Iterator<Item = &'a str>,
    C: Iterator<Item = &'a str>,
{
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            PkgNames::Aur(i) => i.next(),
            PkgNames::Custom(i) => i.next(),
        }
    }
}

/// Packages from a custom repo.
#[derive(Debug, Eq, Clone, PartialEq, Ord, PartialOrd, Hash)]
pub struct CustomPackages {
    /// the repo the package came from.
    pub repo: String,
    /// The srcinfo of the pkgbase.
    pub srcinfo: Box<srcinfo::Srcinfo>,
    /// The pkgs from the srcinfo to install.
    pub pkgs: Vec<CustomPackage>,
}

/// Describes an AUR package base.
#[derive(Debug, Eq, Clone, PartialEq, Ord, PartialOrd, Hash)]
pub struct AurBase {
    /// List of packages belonging to the package base.
    pub pkgs: Vec<AurPackage>,
}

/// A package base.
/// This describes  packages that should be built then installed.
#[derive(Debug, Eq, Clone, PartialEq, Ord, PartialOrd, Hash)]
pub enum Base {
    /// Aur packages.
    Aur(AurBase),
    /// Custom packages.
    Custom(CustomPackages),
}

impl Display for AurBase {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let pkgbase = self.package_base();
        let ver = self.version();

        let name = self.package_base();
        let len = self.pkgs.len();

        if len == 1 && name == pkgbase {
            write!(f, "{}-{}", pkgbase, ver)
        } else {
            write!(f, "{}-{} ({}", self.package_base(), self.version(), name)?;
            for pkg in self.pkgs.iter().skip(1) {
                f.write_str(" ")?;
                f.write_str(&pkg.pkg.name)?;
            }
            f.write_str(")")?;
            Ok(())
        }
    }
}

impl Display for CustomPackages {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let pkgbase = self.package_base();
        let ver = self.version();

        let name = self.package_base();
        let len = self.pkgs.len();

        if len == 1 && name == pkgbase {
            write!(f, "{}-{}", pkgbase, ver)
        } else {
            write!(f, "{}-{} ({}", self.package_base(), self.version(), name)?;
            for pkg in self.pkgs.iter().skip(1) {
                f.write_str(" ")?;
                f.write_str(&pkg.pkg.pkgname)?;
            }
            f.write_str(")")?;
            Ok(())
        }
    }
}

impl Display for Base {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Base::Aur(base) => base.fmt(f),
            Base::Custom(base) => base.fmt(f),
        }
    }
}

impl AurBase {
    /// Gets the package base of base.
    pub fn package_base(&self) -> &str {
        &self.pkgs[0].pkg.package_base
    }

    /// Gets the version of base.
    pub fn version(&self) -> String {
        self.pkgs[0].pkg.version.clone()
    }
}

impl CustomPackages {
    /// Gets the package base of base.
    pub fn package_base(&self) -> &str {
        &self.srcinfo.base.pkgbase
    }

    /// Gets the version of base.
    pub fn version(&self) -> String {
        self.srcinfo.version()
    }
}

impl Base {
    /// Gets the package base of base.
    pub fn package_base(&self) -> &str {
        match self {
            Base::Aur(base) => base.package_base(),
            Base::Custom(base) => base.package_base(),
        }
    }

    /// Gets the version of base.
    pub fn version(&self) -> String {
        match self {
            Base::Aur(base) => base.version(),
            Base::Custom(base) => base.version(),
        }
    }

    /// Amount of packages in this base.
    pub fn package_count(&self) -> usize {
        match self {
            Base::Aur(base) => base.pkgs.len(),
            Base::Custom(base) => base.pkgs.len(),
        }
    }

    /// Iterator of package names in this base.
    pub fn packages(&self) -> impl Iterator<Item = &str> {
        match self {
            Base::Aur(base) => PkgNames::Aur(base.pkgs.iter().map(|p| p.pkg.name.as_str())),
            Base::Custom(base) => {
                PkgNames::Custom(base.pkgs.iter().map(|p| p.pkg.pkgname.as_str()))
            }
        }
    }
}
