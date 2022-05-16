# vim:fdm=marker foldlevel=0 tabstop=2 shiftwidth=2 filetype=fish
# Original Author for pacman: Giorgio Lando <patroclo7@gmail.com>
# Updated for paru by jguer

set -l progname paru

# paru constants
set -l noopt 'not __fish_contains_opt -s G -s V -s P -s S -s D -s Q -s R -s U -s T -s F -s L -s C database query sync remove upgrade deptest files version repoctl chrootctl gendb'
set -l listall "(paru -Pc | string replace ' ' \t)"
set -l listpacman "(__fish_print_packages)"
set -l show '__fish_contains_opt -s P show'
set -l getpkgbuild '__fish_contains_opt -s G getpkgbuild'
set -l repoctl '__fish_contains_opt -s L repoctl'
set -l chrootctl '__fish_contains_opt -s C chrootctl'

# Pacman constants
set -l listinstalled "(pacman -Q | string replace ' ' \t)"
set -l listrepos "(__fish_print_pacman_repos)"
set -l listgroups "(pacman -Sg)\t'Package Group'"
set -l database '__fish_contains_opt -s D database'
set -l query '__fish_contains_opt -s Q query'
set -l remove '__fish_contains_opt -s R remove'
set -l sync '__fish_contains_opt -s S sync'
set -l upgrade '__fish_contains_opt -s U upgrade'
set -l files '__fish_contains_opt -s F files'

complete -c $progname -e
complete -c $progname -f
# HACK: We only need these two to coerce fish to stop file completion and complete options
complete -c $progname -n "$noopt" -a "-D" -d "Modify the package database"
complete -c $progname -n "$noopt" -a "-Q" -d "Query the package database"

# Primary operations
complete -c $progname -s D -f -l database -n "$noopt" -d 'Modify the package database'
complete -c $progname -s Q -f -l query -n "$noopt" -d 'Query the package database'
complete -c $progname -s R -f -l remove -n "$noopt" -d 'Remove packages from the system'
complete -c $progname -s S -f -l sync -n "$noopt" -d 'Synchronize packages'
complete -c $progname -s T -f -l deptest -n "$noopt" -d 'Check dependencies'
complete -c $progname -s U -f -l upgrade -n "$noopt" -d 'Upgrade or add a local package'
complete -c $progname -s F -f -l files -n "$noopt" -d 'Query the files database'
complete -c $progname -s V -f -l version -n "$noopt" -d 'Display version and exit'
complete -c $progname -s h -f -l help -d 'Display help'

# General options
# Only offer these once a command has been given so they get prominent display
complete -c $progname -n "not $noopt" -s b -l dbpath -d 'Alternate database location' -xa "(__fish_complete_directories)"
complete -c $progname -n "not $noopt" -s r -l root -d 'Alternate installation root' -xa "(__fish_complete_directories)"
complete -c $progname -n "not $noopt" -s v -l verbose -d 'Output more status messages' -f
complete -c $progname -n "not $noopt" -l arch -d 'Alternate architecture' -f
complete -c $progname -n "not $noopt" -l cachedir -d 'Alternate package cache location' -xa "(__fish_complete_directories)"
complete -c $progname -n "not $noopt" -l color -d 'Colorize the output' -fa '{auto,always,never}'
complete -c $progname -n "not $noopt" -l config -d 'Alternate config file' -r
complete -c $progname -n "not $noopt" -l confirm -d 'Always ask for confirmation' -f
complete -c $progname -n "not $noopt" -l debug -d 'Display debug messages' -f
complete -c $progname -n "not $noopt" -l disable-download-timeout -d 'Use relaxed timeouts for download' -f
complete -c $progname -n "not $noopt" -l gpgdir -d 'Alternate home directory for GnuPG' -xa "(__fish_complete_directories)"
complete -c $progname -n "not $noopt" -l hookdir -d 'Alternate hook location' -xa "(__fish_complete_directories)"
complete -c $progname -n "not $noopt" -l logfile -d 'Alternate log file'
complete -c $progname -n "not $noopt" -l noconfirm -d 'Bypass any confirmation' -f
complete -c $progname -n "not $noopt" -l sysroot -d 'Operate on a mounted guest system (root-only)' -xa "(__fish_complete_directories)"

# File, query, sync options (files, query, sync)
for condition in files query sync
    complete -c $progname -n "$$condition" -s q -l quiet -d 'Show less information' -f
end

# Transaction options (sync, remove, upgrade)
for condition in sync remove upgrade
    complete -c $progname -n "$$condition" -s d -l nodeps -d 'Skip [all] dependency checks' -f
    complete -c $progname -n "$$condition" -s p -l print -d 'Dry run, only print targets' -f
    complete -c $progname -n "$$condition" -l assume-installed -d 'Add a virtual package to satisfy dependencies' -f
    complete -c $progname -n "$$condition" -l dbonly -d 'Modify database entry only' -f
    complete -c $progname -n "$$condition" -l noprogressbar -d 'Do not display progress bar' -f
    complete -c $progname -n "$$condition" -l noscriptlet -d 'Do not execute install script' -f
    complete -c $progname -n "$$condition" -l print-format -d 'Specify printf-like format' -x
end

# File and query options (files, query)
for condition in files query
    complete -c $progname -n "$$condition" -s l -l list -d 'List the files owned by PACKAGE' -f
end

# File and sync options (files, sync)
for condition in files sync
    complete -c $progname -n "$$condition" -s y -l refresh -d 'Download fresh package databases [force]' -f
end

# Query and sync options (query, sync)
for condition in query sync
    complete -c $progname -n "$$condition" -s g -l groups -d 'Display members of [all] package GROUP' -xa "$listgroups"
end

# Sync and upgrade options (sync, upgrade)
for condition in sync upgrade
    complete -c $progname -n "$$condition" -l asdeps -d 'Install packages as non-explicitly installed' -f
    complete -c $progname -n "$$condition" -l asexplicit -d 'Install packages as explicitly installed' -f
    complete -c $progname -n "$$condition" -l ignore -d 'Ignore a package upgrade (can be used more than once)' -xa "$listall"
    complete -c $progname -n "$$condition" -l ignoregroup -d 'Ignore a group upgrade (can be used more than once)' -xa "$listgroups"
    complete -c $progname -n "$$condition" -l needed -d 'Do not reinstall up to date packages' -f
    complete -c $progname -n "$$condition" -l overwrite -d 'Overwrite conflicting files (can be used more than once)' -r
end

# Database options
set -l has_db_opt '__fish_contains_opt asdeps asexplicit check -s k'
complete -c $progname -n "$database; and not $has_db_opt" -s k -l check -d 'Check database validity'
complete -c $progname -n "$database" -s q -l quite -d 'Suppress output of success messages' -f
complete -c $progname -n "$database; and not $has_db_opt" -l asdeps -d 'Mark PACKAGE as dependency' -x
complete -c $progname -n "$database; and not $has_db_opt" -l asexplicit -d 'Mark PACKAGE as explicitly installed' -x
complete -c $progname -n "$has_db_opt; and $database" -xa "$listinstalled"

# File options - since pacman 5
complete -c $progname -n "$files" -s x -l regex -d 'Interpret each query as a regular expression' -f
complete -c $progname -n "$files" -l machinereadable -d 'Print each match in a machine readable output format' -f
complete -c $progname -n "$files" -d 'Package' -xa "$listpacman"

# Query options
complete -c $progname -n "$query" -s c -l changelog -d 'View the change log of PACKAGE' -f
complete -c $progname -n "$query" -s d -l deps -d 'List only non-explicit packages (dependencies)' -f
complete -c $progname -n "$query" -s e -l explicit -d 'List only explicitly installed packages' -f
complete -c $progname -n "$query" -s i -l info -d 'View PACKAGE [backup files] information' -f
complete -c $progname -n "$query" -s k -l check -d 'Check that PACKAGE files exist' -f
complete -c $progname -n "$query" -s m -l foreign -d 'List installed packages not found in sync database' -f
complete -c $progname -n "$query" -s n -l native -d 'list installed packages only found in sync database' -f
complete -c $progname -n "$query" -s o -l owns -d 'Query the package that owns FILE' -r
complete -c $progname -n "$query" -s p -l file -d 'Query a package file instead of the database' -r
complete -c $progname -n "$query" -s s -l search -d 'Search locally-installed packages for regexp' -f
complete -c $progname -n "$query" -s t -l unrequired -d 'List only unrequired packages [and optdepends]' -f
complete -c $progname -n "$query" -s u -l upgrades -d 'List only out-of-date packages' -f
complete -c $progname -n "$query" -d 'Installed package' -xa "$listinstalled"

# Remove options
complete -c $progname -n "$remove" -s c -l cascade -d 'Also remove packages depending on PACKAGE' -f
complete -c $progname -n "$remove" -s n -l nosave -d 'Ignore file backup designations' -f
complete -c $progname -n "$remove" -s s -l recursive -d 'Also remove dependencies of PACKAGE' -f
complete -c $progname -n "$remove" -s u -l unneeded -d 'Only remove targets not required by PACKAGE' -f
complete -c $progname -n "$remove" -l local -d 'Also remove the package from the local repo' -f
complete -c $progname -n "$remove" -d 'Installed package' -xa "$listinstalled"

# Sync options
complete -c $progname -n "$sync" -s c -l clean -d 'Remove [all] packages from cache' -f
complete -c $progname -n "$sync" -s i -l info -d 'View PACKAGE [extended] information' -f
complete -c $progname -n "$sync" -s l -l list -d 'List all packages in REPOSITORY' -xa "$listrepos"
complete -c $progname -n "$sync" -s s -l search -d 'Search remote repositories for regexp' -f
complete -c $progname -n "$sync" -s u -l sysupgrade -d 'Upgrade all packages that are out of date'
complete -c $progname -n "$sync" -s w -l downloadonly -d 'Only download the target packages'
complete -c $progname -n "$sync" -xa "$listall $listgroups"

# Upgrade options
# Theoretically, pacman reads packages in all formats that libarchive supports
# In practice, it's going to be tar.xz, tar.gz or tar.zst
complete -c $progname -n "$upgrade" -xa '(__fish_complete_suffix pkg.tar.zst; __fish_complete_suffix pkg.tar.xz; __fish_complete_suffix pkg.tar.gz)' -d 'Package file'

complete -c $progname -n "$upgrade" -s i -l install -d 'Install package as well as building' -f

# paru operations
complete -c $progname -s P -f -l show -n "$noopt" -d 'Print information'
complete -c $progname -s G -f -l getpkgbuild -n "$noopt" -d 'Get PKGBUILD from ABS or AUR'
complete -c $progname -s L -f -l repoctl -n "$noopt" -d 'List local repos'
complete -c $progname -s C -f -l chrootctl -n "$noopt" -d 'Interactive shell to the chroot'


# paru options
complete -c $progname -s c -l clean -n "$noopt" -d 'Remove unneeded dependencies' -f
complete -c $progname -l gendb -n "$noopt" -d 'Generate development package DB' -f

# Show options
complete -c $progname -n "$show" -s w -l news -d 'Print arch news' -f
complete -c $progname -n "$show" -s s -l stats -d 'Display system package statistics' -f

# Getpkgbuild options
complete -c $progname -n "$getpkgbuild" -s p -l print -d 'Print PKGBUILD instead of downloading' -f
complete -c $progname -n "$getpkgbuild" -s c -l comments -d "Print the AUR comments from the PKGBUILD's AUR page" -f

complete -c $progname -n "$getpkgbuild" -xa "$listall"

# Repoctl options
complete -c $progname -n "$repoctl" -s l -l list -d 'List packages in local repos' -f
complete -c $progname -n "$repoctl" -s d -l delete -d 'Remove a package from the local repo' -f
complete -c $progname -n "$repoctl; and __fish_contains_opt -s d delete" -d 'Package in local repo' -xa '(paru --repoctl --list --quiet)'
complete -c $progname -n "$repoctl" -s q -l quiet -d 'Show less information' -f
complete -c $progname -n "$repoctl" -s y -l refresh -d 'Refresh local repos' -f
complete -c $progname -n "$repoctl" -s c -l clean -d 'Remove uninstalled packages from repos' -f

# Chrootctl options
complete -c $progname -n "$chrootctl" -s i -l install -d 'Install a package into the chroot' -xa "$listpacman"
complete -c $progname -n "$chrootctl" -s u -l sysupgrade -d 'Upgrade the chroot' -f

# New options
complete -c $progname -n "not $noopt" -l repo -d 'Assume targets are from the repositories' -f
complete -c $progname -n "not $noopt" -s a -l aur -d 'Assume targets are from the AUR' -f
complete -c $progname -n "not $noopt" -l aururl -d 'Set an alternative AUR URL' -f
complete -c $progname -n "not $noopt" -l clonedir -d 'Directory used to download and run PKGBUILDs' -r
complete -c $progname -n "not $noopt" -l makepkg -d 'Makepkg command to use' -f
complete -c $progname -n "not $noopt" -l pacman -d 'Pacman command to use' -f
complete -c $progname -n "not $noopt" -l git -d 'Git command to use' -f
complete -c $progname -n "not $noopt" -l asp -d 'Asp command to use' -f
complete -c $progname -n "not $noopt" -l gpg -d 'Gpg command to use' -f
complete -c $progname -n "not $noopt" -l fm -d 'File manager to use' -f
complete -c $progname -n "not $noopt" -l pager -d 'Pager command to use' -f
complete -c $progname -n "not $noopt" -l completioninterval -d 'Refresh interval for completion cache' -f
complete -c $progname -n "not $noopt" -l sortby -d 'Sort AUR results by a specific field during search' -xa "{votes,popularity,id,baseid,name,base,submitted,modified}"
complete -c $progname -n "not $noopt" -l searchby -d 'Search for AUR packages by querying the specified field' -xa "{name,name-desc,maintainer,depends,checkdepends,makedepends,optdepends}"
complete -c $progname -n "not $noopt" -l limit -d 'Limit number of search results' -x
complete -c $progname -n "not $noopt" -l upgrademenu -d 'Show a detailed list of updates with the option to skip any' -f
complete -c $progname -n "not $noopt" -l noupgrademenu -d 'Do not show the upgrade menu' -f
complete -c $progname -n "not $noopt" -l removemake -d 'Remove make deps after install' -f
complete -c $progname -n "not $noopt" -l noremovemake -d 'Do not remove make deps after install' -f
complete -c $progname -n "not $noopt" -l topdown -d 'Shows repository packages first and then aur' -f
complete -c $progname -n "not $noopt" -l bottomup -d 'Shows aur packages first and then repository' -f
complete -c $progname -n "not $noopt" -l devel -d 'Check -git/-svn/-hg development version' -f
complete -c $progname -n "not $noopt" -l nodevel -d 'Disable development version checking' -f
complete -c $progname -n "not $noopt" -l develsuffixes -d 'Suffixes to define development packages' -f
complete -c $progname -n "not $noopt" -l cleanafter -d 'Clean package sources after successful build' -f
complete -c $progname -n "not $noopt" -l nocleanafter -d 'Disable package sources cleaning' -f
complete -c $progname -n "not $noopt" -l redownload -d 'Redownload PKGBUILD of package even if up-to-date' -f
complete -c $progname -n "not $noopt" -l noredownload -d 'Do not redownload up-to-date PKGBUILDs' -f
complete -c $progname -n "not $noopt" -l provides -d 'Look for matching providers when searching for packages' -f
complete -c $progname -n "not $noopt" -l noprovides -d 'Just look for packages by pkgname' -f
complete -c $progname -n "not $noopt" -l pgpfetch -d 'Prompt to import PGP keys from PKGBUILDs' -f
complete -c $progname -n "not $noopt" -l nopgpfetch -d 'Do not prompt to import PGP keys' -f
complete -c $progname -n "not $noopt" -l newsonupgrade -d 'Print new news during sysupgrade' -f
complete -c $progname -n "not $noopt" -l useask -d 'Automatically resolve conflicts using pacmans ask flag' -f
complete -c $progname -n "not $noopt" -l nouseask -d 'Confirm conflicts manually during the install' -f
complete -c $progname -n "not $noopt" -l savechanges -d 'Commit changes to pkgbuilds made during review' -f
complete -c $progname -n "not $noopt" -l nosavechanges -d "Don't commit changes to pkgbuilds made during review" -f
complete -c $progname -n "not $noopt" -l failfast -d 'Exit as soon as any AUR packages fail to build' -f
complete -c $progname -n "not $noopt" -l nofailfast -d "Don't Exit as soon as any AUR packages fail to build" -f
complete -c $progname -n "not $noopt" -l keepsrc -d "Keep src/ and pkg/ directories after building packages" -f
complete -c $progname -n "not $noopt" -l nokeepsrc -d "Don't keep src/ and pkg/ directories after building packages" -f
complete -c $progname -n "not $noopt" -l combinedupgrade -d 'Refresh then perform the repo and AUR upgrade together' -f
complete -c $progname -n "not $noopt" -l nocombinedupgrade -d 'Perform the repo upgrade and AUR upgrade separately' -f
complete -c $progname -n "not $noopt" -l batchinstall -d 'Build multiple AUR packages then install them together' -f
complete -c $progname -n "not $noopt" -l nobatchinstall -d 'Build and install each AUR package one by one' -f
complete -c $progname -n "not $noopt" -l rebuild -d 'Always build target packages' -f
complete -c $progname -n "not $noopt" -l norebuild -d 'Skip package build if in cache and up to date' -f
complete -c $progname -n "not $noopt" -l mflags -d 'Pass the following options to makepkg' -f
complete -c $progname -n "not $noopt" -l gitflags -d 'Pass the following options to git' -f
complete -c $progname -n "not $noopt" -l gpgflags -d 'Pass the following options to gpg' -f
complete -c $progname -n "not $noopt" -l sudoflags -d 'Pass the following options to sudo' -f
complete -c $progname -n "not $noopt" -l batflags -d 'Pass the following options to sudo' -f
complete -c $progname -n "not $noopt" -l fmflags -d 'Pass the following options to file manager' -f
complete -c $progname -n "not $noopt" -l sudoloop -d 'Loop sudo calls in the background to avoid timeout' -f
complete -c $progname -n "not $noopt" -l nosudoloop -d 'Do not loop sudo calls in the background' -f
complete -c $progname -n "not $noopt" -l chroot -d 'Build packages in a chroot' -f
complete -c $progname -n "not $noopt" -l nochroot -d "Don't build packages in a chroot" -f
complete -c $progname -n "not $noopt" -l sign -d 'Sign packages with gpg' -f
complete -c $progname -n "not $noopt" -l nosign -d "Don't sign packages with gpg" -f
complete -c $progname -n "not $noopt" -l keeprepocache -d 'Keep old versions of packages with local repo' -f
complete -c $progname -n "not $noopt" -l nokeeprepocache -d "Don't keep old versions of packages with local repo" -f
complete -c $progname -n "not $noopt" -l signdb -d 'Sign databases with gpg' -f
complete -c $progname -n "not $noopt" -l nosigndb -d "Don't sign databases with gpg" -f
complete -c $progname -n "not $noopt" -l localrepo -d 'Build packages in a local repo' -f
complete -c $progname -n "not $noopt" -l skipreview -d 'Skip the review process' -f
complete -c $progname -n "not $noopt" -l review -d "Don't skip the review process" -f
