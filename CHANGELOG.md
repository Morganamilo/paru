# Changelog

## Paru v1.0.0 (2020-10-28) - changes from yay

### Added

- Added --nocheck
- Added --develsuffixes
- Added --installDebug
- Added NoWarn
- Added syntax highlighting to pkgbuild printing
- Auto detection for pacman-git. Paru will build against pacman-git's API

### Removed

- Remove --timeupdate, --requestsplitn
- Remove --save -Pg --Pd -Ps
- Remove -Gf
- Remove --cleanmenu
- Remove -Y/--yay, stuff like --gendb still work even though they belonged to -Y

### Changed

- List based flags now append instead of overwriting existing settings
- Packages are now cloned to ~/.cache/paru/clone by default
- Use asp for repo pkgbuilds
- Improve news printing
- Formatting changes to be more pacman like
- -Yc accounts for makedeps for aur packages
- Devel info is now saved to devel.json
- Devel info schema change
- Sources are no longer downloaded in batch
- Only treat packages matching develsuffixes as devel
