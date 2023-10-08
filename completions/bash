# This file is in the public domain.

_arch_compgen() {
  local i r
  COMPREPLY=($(compgen -W '$*' -- "$cur"))
  for ((i = 1; i < ${#COMP_WORDS[@]} - 1; i++)); do
    for r in ${!COMPREPLY[@]}; do
      if [[ ${COMP_WORDS[i]} == ${COMPREPLY[r]} ]]; then
        unset 'COMPREPLY[r]'
        break
      fi
    done
  done
}

_arch_ptr2comp() {
  local list= x y
  for x; do
    for y in '0 --' '1 -'; do
      eval 'set -- ${'$x'[${y% *}]}'
      list+=\ ${@/#/${y#* }}
    done
  done
  _arch_compgen $list
}

_arch_incomp() {
  local r="[[:space:]]-(-${1#* }[[:space:]]|[[:alnum:]_]*${1% *})"
  [[ $COMP_LINE =~ $r ]]
}

_pacman_pkg() {
  _arch_compgen "$(
    if [[ $2 ]]; then
      \pacman -$1 2>/dev/null | \cut -d' ' -f1 | \sort -u
    else
      \pacman -$1 2>/dev/null
    fi
  )"
}

_paru_pkg() {
  [ -z "$cur" ] && _pacman_pkg Slq && return
  _arch_compgen "$(paru -Pc)"
}

_pacman_repo_list() {
  _arch_compgen "$(pacman-conf --repo-list)"
}

_paru() {
  compopt -o default
  local common core cur database files prev query remove sync upgrade o
  local show getpkgbuild
  local cur prev words cword

  _init_completion || return
  database=('asdeps asexplicit')
  files=('list machinereadable refresh regex' 'l x y')
  query=('changelog check deps explicit file foreign groups info list native owns
          search unrequired upgrades' 'c e g i k l m n o p s t u')
  remove=('cascade dbonly nodeps assume-installed nosave print recursive unneeded local' 'c n p s u')
  sync=('asdeps asexplicit clean dbonly downloadonly overwrite groups ignore ignoregroup
         info list needed nodeps assume-installed print refresh recursive search sysupgrade'
    'c g i l p s u w y')
  upgrade=('asdeps asexplicit overwrite needed nodeps assume-installed print recursive install' 'p i')
  core=('database files help query remove sync upgrade version' 'D F Q R S U V h')

  common=('arch cachedir color config confirm dbpath debug gpgdir help hookdir logfile
           noconfirm noprogressbar noscriptlet quiet root verbose

           repo aur aururl clonedir makepkg mflags pacman pacman-conf git gitflags sudo sudoflags
           asp gpg gpgflags fm fmflags pager completioninterval sortby searchby limit upgrademenu
           removemake noremovemake cleanafter nocleanafter rebuild rebuildall norebuild
           rebuildtree redownload noredownload redownloadall pgpfetch nopgpfetch useask
           nouseask savechanges nosavechanges failfast nofailfast keepsrc nokeepsrc combinedupgrade
           nocombinedupgrade batchinstall nobatchinstall provides noprovides devel nodevel
           develsuffixes sudoloop nosudoloop bottomup topdown newsonupgrade bat batflags
           chroot nochroot sign nosign keeprepocache nokeeprepocache signdb nosigndb
           localrepo nolocalrepo review skipreview' 'b d h q r v a')

  show=('news stats' 'w s')
  getpkgbuild=('print comments' 'p c')
  chrootctl=('sysupgrade install' 'u i')
  repoctl=('quiet list delete refresh clean' 'q l d y c')

  for o in 'D database' 'F files' 'Q query' 'R remove' 'S sync' 'U upgrade' 'P show' 'G getpkgbuild' 'C chrootctl' 'L repoctl'; do
    _arch_incomp "$o" && break
  done

  if [[ $? != 0 ]]; then
    _arch_ptr2comp core
  elif [[ ! $prev =~ ^-[[:alnum:]_]*[Vbhr] && ! $prev == --@(cachedir|color|config|dbpath|help|hookdir|gpgdir|logfile|root|version) ]]; then
    [[ $cur == -* ]] && _arch_ptr2comp ${o#* } common ||
      case ${o% *} in
      D | R)
        _pacman_pkg Qq
        ;;
      F)
        { _arch_incomp 'l list' && _pacman_pkg Slq; } ||
          _arch_incomp 'o owns' ||
          compopt +o default
        ;;
      Q)
        { _arch_incomp 'g groups' && _pacman_pkg Qg sort; } ||
          { _arch_incomp 'p file' && _pacman_file; } ||
          { _arch_incomp 's search' && compopt +o default; } ||
          { _arch_incomp 'u upgrades' && compopt +o default; } ||
          _arch_incomp 'o owns' ||
          _pacman_pkg Qq
        ;;
      S)
        { _arch_incomp 'g groups' && _pacman_pkg Sg; } ||
          { _arch_incomp 'l list' && _pacman_repo_list; } ||
          { _arch_incomp 's search' && compopt +o default; } ||
          _paru_pkg
        ;;
      U)
        _pacman_file
        ;;
      G)
        _paru_pkg
        ;;
      esac
  fi
  true
}

_pacman_file() {
  compopt -o filenames
  _filedir 'pkg.tar*'
}

complete -F _paru paru

# ex:et ts=2 sw=2 ft=sh
