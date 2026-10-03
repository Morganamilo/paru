#!/usr/bin/env bash
# Exercise real makechrootpkg builds without LocalRepo or host installation.
set -euo pipefail

cd "$(dirname "$0")/.."
paru=$(realpath "${CARGO_TARGET_DIR:-target}/release/paru")
work=$(mktemp -d /var/tmp/paru-chroot-test.XXXXXX)
cleanup() {
    sudo rm -rf -- "$work"
    test ! -e "$work"
}
trap cleanup EXIT
printf 'Chroot test directory: %s\n' "$work"

# Keep all paru state and build outputs inside the temporary directory.
mkdir -p "$work/state" "$work/cache" "$work/config"
export XDG_STATE_HOME="$work/state" XDG_CACHE_HOME="$work/cache" XDG_CONFIG_HOME="$work/config"
export PARU_CONF="$work/paru.conf"
printf '[options]\n' > "$PARU_CONF"

fixture() {
    local name=$1
    mkdir -p "$work/$name"
    cat > "$work/$name/PKGBUILD" <<EOF
pkgname=$name
pkgver=1
pkgrel=1
arch=(any)
license=(MIT)
package() {
    install -Dm644 /dev/null "\$pkgdir/usr/share/$name/fixture"
}
EOF
}

for name in paru-chroot-foundation paru-chroot-provider paru-chroot-check paru-chroot-unrelated paru-chroot-virtual paru-chroot-consumer; do
    fixture "$name"
done
cat >> "$work/paru-chroot-provider/PKGBUILD" <<'EOF'
provides=('paru-chroot-virtual=2')
# Git must be pulled from repositories when this archive is injected later.
depends=(paru-chroot-foundation git)
EOF
cat >> "$work/paru-chroot-unrelated/PKGBUILD" <<'EOF'
build() {
    while IFS= read -r package; do
        case $package in
            paru-chroot-foundation|paru-chroot-provider|paru-chroot-check) return 1 ;;
        esac
    done < <(pacman -Qq)
}
EOF
# An explicit matching target must not replace the provider already selected
# for the consumer's dependency.
cat >> "$work/paru-chroot-virtual/PKGBUILD" <<'EOF'
pkgver=2
EOF
cat >> "$work/paru-chroot-consumer/PKGBUILD" <<'EOF'
depends=(paru-chroot-foundation 'paru-chroot-virtual>=2')
checkdepends=(paru-chroot-check)
build() {
    pacman -Q paru-chroot-foundation paru-chroot-provider
    git --version
    while IFS= read -r package; do
        case $package in
            paru-chroot-unrelated|paru-chroot-virtual) return 1 ;;
        esac
    done < <(pacman -Qq)
}
check() {
    pacman -Q paru-chroot-check
}
# Override the runtime dependency to exercise global build requirements.
package() {
    depends=(paru-chroot-provider)
    install -Dm644 /dev/null "$pkgdir/usr/share/paru-chroot-consumer/fixture"
}
EOF

for checks in enabled disabled; do
    args=()
    if [[ $checks == disabled ]]; then
        args+=(--nocheck)
        rm -f "$work/paru-chroot-consumer/"*.pkg.tar.*
    fi
    PARU_DEBUG=1 "$paru" -B \
        "$work/paru-chroot-unrelated" "$work/paru-chroot-foundation" \
        "$work/paru-chroot-provider" "$work/paru-chroot-check" "$work/paru-chroot-virtual" "$work/paru-chroot-consumer" \
        --chroot="$work/chroot" --nolocalrepo --noinstall --skipreview \
        --noconfirm --failfast --nocleanafter "${args[@]}" > "$work/$checks.log" 2>&1 || {
            tail -100 "$work/$checks.log"
            exit 1
        }
    # Check both source preparation and package construction process arguments.
    mapfile -t commands < <(rg 'running command:.*paru-chroot-consumer.*makechrootpkg' "$work/$checks.log")
    [[ ${#commands[@]} == 2 ]]
    for command in "${commands[@]}"; do
        [[ $command == *'-I'*'paru-chroot-foundation-1-1-any.pkg.tar.'* ]]
        [[ $command == *'-I'*'paru-chroot-provider-1-1-any.pkg.tar.'* ]]
        [[ $command != *'paru-chroot-unrelated-1-1-any.pkg.tar.'* ]]
        [[ $command != *'paru-chroot-virtual-2-1-any.pkg.tar.'* ]]
        if [[ $checks == enabled ]]; then
            [[ $command == *'paru-chroot-check-1-1-any.pkg.tar.'* ]]
        else
            [[ $command != *'paru-chroot-check-1-1-any.pkg.tar.'* ]]
        fi
    done
    mapfile -t packages < <(compgen -G "$work/paru-chroot-consumer/paru-chroot-consumer-1-1-any.pkg.tar.*")
    [[ ${#packages[@]} == 1 ]]
    buildinfo=$(bsdtar -xOf "${packages[0]}" .BUILDINFO)
    rg -q '^installed = git-' <<< "$buildinfo"
    root_packages=$(sudo arch-nspawn "$work/chroot/root" pacman -Qq)
    if rg -qx git <<< "$root_packages"; then
        printf 'Git is already present in the clean root; archive runtime dependency path was not exercised.\n' >&2
        exit 1
    fi
    if rg -q '^installed = paru-chroot-virtual-' <<< "$buildinfo"; then
        printf 'Explicit target replaced the resolved provider in the consumer chroot.\n' >&2
        exit 1
    fi
    if [[ $checks == enabled ]]; then
        rg -q '^installed = paru-chroot-check-' <<< "$buildinfo"
    elif rg -q '^installed = paru-chroot-check-' <<< "$buildinfo"; then
        printf 'Check dependency found in a build with checks disabled.\n' >&2
        exit 1
    fi
    printf 'Real chroot builds passed with checks %s.\n' "$checks"
done

# A failed planned provider must leave each consumer's declarations to makepkg.
for name in paru-chroot-git-failure paru-chroot-git-consumer paru-chroot-version-consumer paru-chroot-version-unsatisfied paru-chroot-unavailable-consumer; do
    fixture "$name"
done
cat >> "$work/paru-chroot-git-failure/PKGBUILD" <<'EOF'
provides=('git=999999' paru-chroot-unavailable)
build() {
    printf 'Deliberate provider build failure.\n' >&2
    return 1
}
EOF
for consumer in git-consumer version-consumer version-unsatisfied unavailable-consumer; do
    case $consumer in
        git-consumer) dependency=git ;;
        version-consumer) dependency='git>=1' ;;
        version-unsatisfied) dependency='git>=999999' ;;
        unavailable-consumer) dependency='paru-chroot-unavailable' ;;
    esac
    printf '\nmakedepends=(%q)\nbuild() { git --version; }\n' "$dependency" >> "$work/paru-chroot-$consumer/PKGBUILD"
done

if PARU_DEBUG=1 "$paru" -B \
    "$work/paru-chroot-git-failure" "$work/paru-chroot-git-consumer" \
    "$work/paru-chroot-version-consumer" "$work/paru-chroot-version-unsatisfied" \
    "$work/paru-chroot-unavailable-consumer" \
    --chroot="$work/chroot" --nolocalrepo --noinstall --skipreview \
    --noconfirm --nocleanafter > "$work/fallback.log" 2>&1; then
    printf 'Deliberately failed packages were reported as successful.\n' >&2
    exit 1
fi

# Success means actual consumer archives, with Git recorded in their chroots.
for consumer in git-consumer version-consumer; do
    mapfile -t packages < <(compgen -G "$work/paru-chroot-$consumer/paru-chroot-$consumer-1-1-any.pkg.tar.*")
    if [[ ${#packages[@]} != 1 ]]; then
        tail -100 "$work/fallback.log"
        exit 1
    fi
    buildinfo=$(bsdtar -xOf "${packages[0]}" .BUILDINFO)
    rg -q '^installed = git-' <<< "$buildinfo"
    mapfile -t commands < <(rg "running command:.*paru-chroot-$consumer.*makechrootpkg" "$work/fallback.log")
    [[ ${#commands[@]} == 2 ]]
    for command in "${commands[@]}"; do
        [[ $command != *'"-I"'* ]]
    done
done
for consumer in git-failure version-unsatisfied unavailable-consumer; do
    if compgen -G "$work/paru-chroot-$consumer/paru-chroot-$consumer-1-1-any.pkg.tar.*" >/dev/null; then
        printf 'An unsatisfied dependency or failed provider produced an archive.\n' >&2
        exit 1
    fi
done
rg -q 'Deliberate provider build failure' "$work/fallback.log"
rg -q 'target not found: git>=999999' "$work/fallback.log"
rg -q 'target not found: paru-chroot-unavailable' "$work/fallback.log"
if rg -q 'missing built package|does not satisfy.*resolve dependencies again' "$work/fallback.log"; then
    printf 'Archive selection blocked normal chroot resolution.\n' >&2
    exit 1
fi
summary=$(rg 'error: packages failed to build:' "$work/fallback.log")
for consumer in git-failure version-unsatisfied unavailable-consumer; do
    [[ $summary == *"paru-chroot-$consumer-1-1"* ]]
done
[[ $summary != *'paru-chroot-git-consumer-1-1'* ]]
[[ $summary != *'paru-chroot-version-consumer-1-1'* ]]
printf 'Real chroot fallback and unsatisfied dependency builds passed.\n'
