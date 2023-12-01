# Changelog

## Paru v2.0.1 (2023-12-01)

### Added

- Print which package failed to clean in -Sc #1074

### Fixed

- Fix local pkgbuild review bf35cab
- Don't try interactive install with empty package list 26ce55f
- Don't refresh when no Git pkgbuild repos e090af4
- Fix compile on arm d1a9c4

### Localization

- Update sv locale #1088

## Paru v2.0.0 (2023-11-26)

Paru v2.0.0 comes after a long time of no releases, mostly due to a lack of free time
and a lot of changes that needed a lot of work to get done.

This update brings a lot of big changes, mostly aimed at power users, and a handul of general
improvements and quality of life changes.

As there's not been a release in so long, consider a lot of the things here not battle tested,
with a .1 patch to follow.

This changelog won't include minor changes as there are a lot.

### PKGBUILD Repos

The main feature of this release is properly integrating non aur pkgbuilds into the build
engine. You can now add pkgbuild repos to your paru.conf in the form of:

```
[repo_name]
Url = https://path/to/git/repo
```

Then syncing the repo with `paru -Sy --pkgbuilds`.

You can also specify 'Path =' instead to point to a pkgbuild repo on disk.

paru will then recognise this repo as a source of pkgbuilds just like the AUR.

```
paru -S foo
```

pkgbuild repos have a higher priority than the AUR so this can also be used to shadow AUR
packages with your own pkgbuilds. The deps of these pkgbuilds can still include AUR deps.

There is also an automatic pkgbuild repo named `.` in the current directory. Essentionally
there's an invisible:

```
[.]
Path = .
```

in your paru.conf.

This allows doing `paru -S ./foo` where `foo` is the name of a package (not path) under the
current directory. this means if you have a bunch of pkgbuilds in a directory that depend on
each other you can build one with `paru -S ./foo` and paru will solve and build the dependencies
across pkgbuilds.

Previously `paru -U` could be used to build a pkgbuild in the current directory. This has been
renamed to `paru -B <dirs>...` allowing you to specify multiple pkgbuilds to build at once.

See `paru.conf(5)` for more information on this.

### Chroot

--chroot now works without local repos, though it still works better with them.

### --interactive

`paru foo` has always been there for interactive search and install. There is now `--interactive`
which works for other operactions.

- `paru foo` is an alias for `paru -S --interactive foo`
- `paru -R --interactive foo` can be used for an interactive remove.
- `paru -Ss/-Qs --interactive foo` will give an interactive prompt then print the chosen packages so they can be piped to other commands.

### Provides

Provide searching is now better and is now enabled in the default paru.conf. You probably
want to uncomment this option if you have an existing paru.conf.

### Contributors

Thanks to every one who has contributed code and translations for paru.
And thanks to every one who has decided to sponsor the project.

### Added

- Add --interactive
- Add --nolocalrepo
- Add --pacmanconfbin
- Add --provides=all
- Add --pkgbuilds and --mode
- Add --rebuild=tree
- Add -o to ignore optional deps with --clean
- Add IgnoreDevelsource
- Add IgnoreDevel
- Add AurRpcUrl
- Add pkgbuild repos
- Add -S ./ syntax

### Changed

- Replace devel.json with devel.toml
- Move devel.toml to $XDG_STATE_HOME
- Dates now use the local time zone
- Fallback to cat when less is not available
- No confirm pacman install after final paru confirmation
- Copy DB into chroot before update
- Disable -w/--downloadonly when installing aur packages
- Allow chroot without local repo
- Replace -U with -B

### Fixed

- Pass env into chroot
- Fix separate install of split packages
- Create local repo when refreshing
- Respect makepkg config outside of chroot
- Fix no sudo when running paru -Sc
- Fix --redownload=yes
- Don't try set install reason if package didn't actually install
- colour version when printing install
- Fix assume install for chroot
- Don't install makedeps when chroot
- Fix -dd in chroot
- Don't review when no packages
- Fix aur packages not being case sensitive

## Paru v1.11.2 (2022-11-05)

Rebuild for openssl 3

## Paru v1.11.1 (2022-07-06)

### Fixed

- Fix installing split packages that depend on each other 8c24f92
- Fix man page typos #785

### Localization

- Update ja locale #791
- Update ca locale #788
- Update zh_CN locale #786
- Update zh_TW locale #784

## Paru v1.11.0 (2022-06-24)

### Added

- Don't try to build packages if deps are missing on fail baceee9
- Support verbose pkg list 84f16c4
- Add armv7h builds #723
- Add --failfast df382ce
- Add --keepsrc 6b7858a
- Add -Lc 57c2f23
- add fossil as devel suffix to the config #728

### Fixed

- Fix typo in paru.conf man page #709
- Include sudo flags in sudo loop 3620089
- Properly check for deps via provides with batch install a90ced5
- Don't check check_depends during batch install if --nocheck 386633d
- Fix fish completion #736 #749 #756
- Fix rebuild option #750
- Fix typo #738
- Fix --redownload=all 5371b65
- Fix crash when package changes packagename bf9691c
- Fix chroot cleaning c60d17e
- Use clean instead of remove for -Sc message ca8040e

### Localization

- Update pt locale #706
- Update hr locale #704
- Update nl locale #724 #743 #765
- Update pt_BR locale #732

### Changed

- Add triple dots to upgrade search messages #769

## Paru v1.10.0 (2022-03-24)

### Added

- Add flag to download AUR packages via SSH #669
- Add option to keep older packages in local repo #668

### Fixed

- Fix being unable to create chroot 5bf10e1
- Fix typo in man page #695

### Localization

- Update ko_KR locale #680
- Update pt_BR locale #687

## Paru v1.9.3 (2022-02-16)

### Added

- Add --pager and MakepkgConf options #603
- Add --asdep --asexp as short hand options 348bdb3

### Fixed

- Fix crash when updating local repo where local version != sync 017e05b
- Add completion to -Ui 19fd836
- Return no instead of default for invalid input 408f3cc
- Fix crash when current directory no longer exists ffef79d
- Fix comment fetching with new aurweb ea02818

### Changed

- Rename -C --upgrade to --sysupgrade and -Lu to -Ly 0bace84
- Expand docs on --fm 48b0086

### Localization

- Add hr locale #632
- Update es locale #645
- Update zh_CN locale #675

## Paru v1.9.2 (2021-12-09)

## Fixed

- Fix paru not actually waiting on children properly e336648
- Fix typo #608 f8e0888

### Localization

- Update pt locale #553
- Update es locale #606
- Update ru locale #607
- Update pt_br locale #605

## Paru v1.9.1 (2021-11-26)

### Fixed

- Make paru -G return a correct exit code #601
- Fix manpage wording #598
- Add docs for -Po and --regex 3ebff0f
- Remove Stale develinfo when package changes source Remove dbaec6c

### Changed

- Sort -Sla output 200b8b1
- Wait for child process to exit on signal 8c04dbb

### Localization

- Update pt_br locale #568

## Paru v1.9.0 (2021-11-01)

### Added

- Add -Po to show package install order #550
- Add timeout to git ls-remote 6278891
- Add -hg to default DevelSuffixes #546
- Add search size limit #541
- Add socks proxy support 5a25ec3
- Add -x/--regex for -Ss 6790a5d
- Add --savechanges to save pkgbuild changes e05ca07

### Fixed

- Fix typos #560
- Fix -d acting as -cc 69ae7bf
- Fix -U picking non local repo if package with same name 95c1f91

### Localization

- Add ko locale #565
- Update de locale #513

## Paru v1.8.2 (2021-08-18)

### Fixed

- Fix wrong length check leading to overflow in -Si 9de304b
- Fix format string printing wrongly f6b5431
- Make upgrade text bold to match pacman d5d1724

### Localization

- Add zh_TW locale #493
- Update fr locale #510
- Update ja locale #504

## Paru v1.8.1 (2021-08-12)

### Fixed

- Fix makepkg command giving wrong args in error message #492
- Fix stack overflow with some cyclic deps 22ec445
- Fix excessive output during -Ld

### Changed

- Ignore PKGEXT in chroot 8ff2ba9
- -Ld also removes sig files 2a1a412

### Localization

- Update nl locale #488
- Update de locale #487
- Update pt_BR locale #496

## Paru v1.8.0 (2021-08-03)

### Added

- Localization support 8667e98 5b1ff38
- Added ability to static link libalpm a39641c
- Add [env] section in paru.conf daf225f

### Fixed

- Fix short opts not taking args after space #485
- Fix -Sc not cleaning untracked files #430
- Fix -U sometimes not installing repo deps 61b10e0
- Fix chroot builds sometimes doing nothing when no AUR updates 08aa1f7
- Bail if call to gpg fails 9fab5d9

### Changed

- Order packages by dep order even when satisfied 210e375
- Add -F to default less flags 3b6cb32
- Warn instead of bail on unknown section 11bc07d

### Localization

- Add zh_CN locale #439
- Add ca locale #439
- Add it locale #440
- Add pt_BR locale #434
- Add pt locale #438
- Add es locale #435
- Add fi locale #441
- Add nl locale #444
- Add ru locale #449
- Add de locale #436
- Add ja locale #451
- Add tr locale #437
- Add ro locale #453
- Add fr locale #456
- Add sv locale #460
- Add hi locale #463

## Paru v1.7.3 (2021-06-24)

### Fixed

- Fix -U trying to resolve already satisfied deps 012db72
- Don't pass deps twice during -U afe0a1f
- Fix -dd not being passed to pacman 5a496c4

### Changed

- Move arch prompt and pgp import out of review 3d3eccf
- Retry aur search when one targ fails 4020507
- Only add packages to be installed to local repos 23f0a8c

## Paru v1.7.2 (2021-06-08)

### Fixed

- Fix compilation on 32 bit 9a211ed
- Fix -S'ing ignored packages b6ac4f3

### Changed

- Include #AurOnly in the example config 8f1d6d9

## Paru v1.7.1 (2021-06-03)

### Fixed

- Fix panic in provider menu 73ecc7

## Paru v1.7.0 (2021-05-31)

### Fixed

- Correctly decide if a pkg is aur or repo with -G 911ae21
- Fix provider menu showing during -Y 5c9b390
- Fix -G not updating already downloaded packages f5d5145
- Fix provider menu sometimes showing twice for targets da91f84
- Handle query part in devel urls bc5f545
- Fix exiting if devel pkg can not be checked 7463ae3
- Fix skipping repo packages with upgrademenu ba35318
- Fix crash in upgrade menu cc98ec6

### Added

- Add signing option d0ca8dd 74c7120
- Add -P --stats #240
- Support --assume-installed e6c6460
- Add -Ldd c525fab
- libalpm 13 support 062b0db

### Removed

- Remove -R --local c525fab
- Remove MovePkgs 69b5fb0
- libalpm \<13 support 062b0db

### Changed

- Better error messages when calling commands 257f73a
- Install repo packages with aur pkgs when using chroot 06a0f1f
- Pass mflags to makechrootpkg 7bce35d
- Expect local repos to be user writable 4d224b8
- -C will create the chroot if it does not exist 5b6730f

## Paru v1.6.1 (2021-05-04)

### Fixed

- Fix out of date packages not being shown dca010e
- Fix typo in man page #359

## Paru v1.6.0 (2021-05-03)

### Fixed

- Fix -Sc failing when pkg/ is not writable #324
- Fix not checking return of `git ls-remote` cd2049a
- Fix progress bar display 97b2d0b
- Fix upgrademenu crashing on non installed packages de26909
- Don't drop to a shell after -Cu b951d73
- Fix -Sc doc f83cbd3

### Added

- Paru now sets the user agent for aur requests 841ebb5
- support latest pacman-git 2cbe4ed

### Removed

- Removed pacman6-alpha support -- use pacman or pacman-git instead

### Changed

- Cancel sudo loop after sudo fails acbcfc1
- --gendb: don't update already existing entries 49eb22
- Better error message when AUR does not return 400 #351

## Paru v1.5.1 (2021-04-13)

### Fixed

- Fix --overwrite not being passed to pacman calls 28e4c0e
- Fix typo in manpage #300
- Fix args not allowing pacman globals e82ba9a
- Fix copy error in help message 25f6cc7

### Changed

- Use makepkg to generate srcinfo with paru -U #307
- Don't noconfirm -Ui 9d9cdb1
- Use -d/--delete to clean the whole package instead of -Scc edd43ef

## Paru v1.5.0 (2021-03-30)

### Fixed

- Fix typos #283 #276
- Fix packages being marked as explicit when they shouldn't 47c4caf
- Fix PAGER having higher priority than PARU_PAGER 82a7326
- Fix pacman error message not including sudo #297

### Added

- Wait for pacman when db is locked 57d73db

### Changed

- Only upgrade installed packages when using chroot 4d126bc
- Handle devel packages in local repo but not installed c5dfff2 c698f2e
- Don't enable provider menu on sysupgrade c698f2e

## Paru v1.4.0 (2021-03-14)

### Fixed

- Fix news continuie prompt not working c9431b4
- Fix ignore of .SRCINFO during initial review #264
- Fix packages.aur not being updated 28478c8
- Fix help menu: rename --noskipreview to --review #272
- Fix wrong return code with --needed 64409bf

### Added

- Added verbose search (-Sss) #266
- Warn when using chroot without devtools 06ff12a

## Paru v1.3.0 (2021-02-19)

### Fixed

- Fix misleading error when doing --nonexistantarg=foo 37fddea
- Fix build dir not existing and rename to clonedir 5786bb2
- Fix -Qu with local repo 7a9454f
- Fix typos #225 #245
- Create cache dir before caching aur list f18a901
- Make -Ta return 127 when not all packages are in AUR b91d7ee
 
### Added

- Add pager option ca9bfea
- Add PARU_PAGER 097433d
- Allow setting custom sudo loop behaviour c165166
- Add --skipreview a6c8511
- Add paru -U e54d635

### Changed

- Show all files in review 9830c74
- Skip review prompt when nothing to review 2a5e441
- Improve local repo documentation 


## Paru v1.2.2 (2021-02-03)

### Fixed

- Fix upgrademenu padding wrong with local repo 3198feb
- Fix chroot not installing repo packages f083096
- Fix sigpipe killing paru during review 27733e3

## Paru v1.2.1 (2021-02-02)

### Fixed

- Fix -G with split packages 16998c9
- Fix --bat not expecting an argument  e82a0ff
- Make PAGER a command instead of just the binary name d3e605c

### Added

- Add out of date info on -Ss 5e458eb

## Paru v1.2.0 (2021-01-29)

### Fixed

- Fix devel packages with multiple sources not updating 237f739
- Fix tab completion for -a in fish #155
- Fix -y's not being respected dcdf6f1
- Fix asdeps/asexp check 3657863

### Added

- aarch64 builds #182
- Add doas example to config 164f349
- Local repo and chroot support 4078200 319859f

### Changed

- Don't error out when arch news is down 929fa34
- Simplify help file 1031ebc 55b2992
- Print all diffs into a single less instance 89c3f22
- Continue building packages when one fails 315f93e
- Ignore aur when p/print is passed 76433ff
- --noconfirm also skips review #175
- NoWarn now supports globbing #176

## Paru v1.1.4 (2020-12-21)

### Fixed

- Remove nonexistent topdown option from `paru.conf(5)` #124
- Fix ignorepkg for devel pkgs ac0de9f
- Fix rebuild not taking an arg a516eda

### Changed

- Add example for packages exclude in ask input #130

## Paru v1.1.3 (2020-11-29)

### Fixed

- Fix --color behaving differently to pacman 5b56f86

### Changed

- Wait for editor to exit before deleting view #110
- Warn instead of error on unknown values in paru.conf 0e4e618

## Paru v1.1.2 (2020-11-18)

### Fixed

- Fix some packages not resolving and tweak package not found message e860dad
- Remove --stats from docs f8ce25b
- removing code that would false-flag debug pkgs #105 4ec0bb9

### Changed

- Improve error message for unknown option b3d0e95
- Do aur + devel upgrade concurrently 836bba8
- Fix warnings not showing with upgrade menu fedddb6

## Paru v1.1.1 (2020-11-12)

### Fixed

- Fix numbers in upgrade menu being printed improperly for devel packages #100
- Fix `--batflags` zsh completion #98

## Paru v1.1.0 (2020-11-12)

### Fixed

- Don't try to remove packages with -c when nothing to remove d3c668e
- Fix error if running `-Sy` with no targets 95f5681
- Hide warnings for debug packages b3e0fe0
- Don't try to install non existing devel packages 5e910d9
- Fix clean checking for wrong file type e9a260e
- Try and make pkg writable before deleting 70676bf
- Remove dashes from InstallDebug in paru.conf.5 #86
- Add missing asp completion 8e19638
-  Fix -Qu return value not always being correct 0b791bc

### Added

- Add AUR comment support faf4d51
- Add --bat and --batflags 8693ccd

### Changed

- Print [ignored] for ignored aur packages during -Qu 960fb1a
- Sort devel.json 30e1c6b
- Add mflags example to config ecc4ecd
- Always pass ignored packages to pacman 591d593
- Flip upgrade menu 79a4ab7

## Paru v1.0.2 (2020-11-02)

### Fixed

- Fix Git wanting login credentials for some repos ea3062e
- Fix diffs breaking with `--cleanafter` 0b66f9c
- Fix provider menu formatting d7f100a
- Fix devel not handling split packages properly a9f47ea
- Fix rogue "info" object in devel.json d434467
- Fix version compare 30da3bd

### Added

- Support repo prefix for `-G` ea512c6
- Added --newsonupgrade to print news on upgrade b30a35e

### Changed

- News now returns 1 when there is no new news 792e15c

## Paru v1.0.1 (2020-10-30)

### Fixed

- Fix `-Ss` not supporting `--bottomup`/`--topdown` #31 
- Fix typos and wording #30 #32 #36 #40 #46 91e8009
- Fix `-G` using lcoaldb instead of syncdb d34d3b0
- Fix warnings not printing when there are no upgrades 4b14b50
- Fix `--upgrademenu` printing the wrong local version #37
- Fix `paru <target>` trying to install all packages when input is empty ba00b9f
- Fix upgrade menu not showing sometimes 21e0b19
- Fix missing =all option to redownload/rebuild c2fc530
- Fix --overwrite not expecting an arg 9253841
- Fix not setting usage in alpm 9ee5a12
- Fix group printing 0451d60
- Fix `--removemake` not accounting for repomake b3a62bc
- Fix ignorepkg not working for devel packages 0bf2692

### Added

- Add `PARU_CONF` to configure paru's config file location eb9e1b1

### Changed

- AUR search is now case insensitive 5b27fc1
- `-Sh` and other operations will now forward to pacman's help for those options d148352
- Print an error when there's no action to do ce48f12
- `--gendb` will now continue on srcinfo parse errors 73eb796
- Don't prompt to continue install when pacman is going to prompt 9663f84

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
