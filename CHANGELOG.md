# Changelog

## Paru v1.0.2 (2020-11-2)

### Fixed

- Fix Git wanting login credentials for some repos ea3062eb10be9fef99d0b429544a436d0d0ea8ea
- Fix diffs breaking with `--cleanafter` 0b66f9cd818ce976f3baa70ed2d63f888e961a79
- Fix provider menu formatting d7f100a0d1220227499ad6c08112f7e4b8217cbe
- Fix devel not handling split packages properly a9f47eaa21c7de8a23f3be1cda758d0c1800c48b
- Fix rouge "info" object in devel.json d43446769774bccc6f64177b7d247264562c6f7d
- Fix version compare 30da3bd1cba4a56f900eb95e3bf32266ef0e1ac0

### Added

- Support repo prefix for `-G` ea512c6bd42b556c7744658048969d63ec9c143e
- Added --newsonupgrade to print news on upgrade b30a35e648ad2f1d04d4bfe9663c4fa8b478e810

### Changed

- News now returns 1 when there is no new news 792e15c9d723f1061171a093c5e7cef673942531

## Paru v1.0.1 (2020-10-30)

### Fixed

- Fix `-Ss` not supporting `--bottomup`/`--topdown` #31 
- Fix typos and wording #30 #32 #36 #40 #46 91e8009acfcfa082d39927a3c6d755de1b276328
- Fix `-G` using lcoaldb instead of syncdb d34d3b0cb3a7f238c6a2ea66c81857af632e8769
- Fix warnings not printing when there are no upgrades 4b14b50f7f972af4f5f30f0a1d2d8babe3be089f
- Fix `--upgrademenu` printing the wrong local version #37
- Fix `paru <target>` trying to install all packages when input is empty ba00b9f82e8a9e9d5fe48c8f554585565b24deed
- Fix upgrade menu not showing sometimes 21e0b19ee9073d241796fa615b6f64a11dceb603
- Fix missing =all option to redownload/rebuild c2fc530502eb47ce08638cd5cf4e96858e15d38e
- Fix --overwrite not expecting an arg 9253841f844fcde2ad645aadbf0c72cc8b4deee6
- Fix not setting usage in alpm 9ee5a12831842f72addd432414b7a28f17970981
- Fix group printing 0451d604c12578df83c927de87e43ace893f166f
- Fix `--removemake` not accounting for repomake b3a62bc10e6d23b1c676026b5d616417e637e14c
- Fix ignorepkg not working for devel packages 0bf26921be7c0b3fbc627bd590fdabaafe2e9386

### Added

- Add `PARU_CONF` to configure paru's config file location eb9e1b1a49ebc317274740ff222d677003bf0ee9

### Changed

- AUR search is now case insensitive 5b27fc12a09375dff70c837f100436e5c3ef4256
- `-Sh` and other operations will now forward to pacman's help for those options d148352bbe6e61dd7e470d32417d8d1a3703d2de
- Print an error when there's no action to do ce48f12401cdcdc5129e197a6cece18c2fc46c6f
- `--gendb` will now continue on srcinfo parse errors 73eb796e63581897ac698c12dd98a3a97f23beed
- Don't prompt to continue install when pacman is going to prompt 9663f84fd975e159fdf0760ab522a982504b44ec

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
