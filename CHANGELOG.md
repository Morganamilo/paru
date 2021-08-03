# Changelog

## Paru v1.8.0 (2021-8-3)

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

## Paru v1.7.3 (2021-6-24)

### Fixed

- Fix -U trying to resolve already satisfied deps 012db72
- Don't pass deps twice during -U afe0a1f
- Fix -dd not being passed to pacman 5a496c4

### Changed

- Move arch prompt and pgp import out of review 3d3eccf
- Retry aur search when one targ fails 4020507
- Only add packages to be installed to local repos 23f0a8c

## Paru v1.7.2 (2021-6-8)

### Fixed

- Fix compilation on 32 bit 9a211ed
- Fix -S'ing ignored packages b6ac4f3

### Changed

- Include #AurOnly in the example config 8f1d6d9

## Paru v1.7.1 (2021-6-3)

### Fixed

- Fix panic in provider menu 73ecc7

## Paru v1.7.0 (2021-5-31)

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

## Paru v1.6.1 (2021-5-4)

### Fixed

- Fix out of date packages not being shown dca010e
- Fix typo in man page #359

## Paru v1.6.0 (2021-5-3)

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

## Paru v1.5.1 (2021-4-13)

### Fixed

- Fix --overwrite not being passed to pacman calls 28e4c0e
- Fix typo in manpage #300
- Fix args not allowing pacman globals e82ba9a
- Fix copy error in help message 25f6cc7

### Changed

- Use makepkg to generate srcinfo with paru -U #307
- Don't noconfirm -Ui 9d9cdb1
- Use -d/--delete to clean the whole package instead of -Scc edd43ef

## Paru v1.5.0 (2021-3-30)

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

## Paru v1.4.0 (2021-3-14)

### Fixed

- Fix news continuie prompt not working c9431b4
- Fix ignore of .SRCINFO during initial review #264
- Fix packages.aur not being updated 28478c8
- Fix help menu: rename --noskipreview to --review #272
- Fix wrong return code with --needed 64409bf

### Added

- Added verbose search (-Sss) #266
- Warn when using chroot without devtools 06ff12a

## Paru v1.3.0 (2021-2-19)

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
- Skip review prompt when nothign to review 2a5e441
- Improve local repo documentation 


## Paru v1.2.2 (2021-2-3)

### Fixed

- Fix upgrademenu padding wrong with local repo 3198feb
- Fix chroot not installing repo packages f083096
- Fix sigpipe killing paru during review 27733e3

## Paru v1.2.1 (2021-2-2)

### Fixed

- Fix -G with split packages 16998c9
- Fix --bat not expecting an argument  e82a0ff
- Make PAGER a command instead of just the binary name d3e605c

### Added

- Add out of date info on -Ss 5e458eb

## Paru v1.2.0 (2021-1-29)

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
- Fix error if running `-Sy` with no tagets 95f5681
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
