# Paru

An AUR helper and pacman wrapper

[![paru](https://img.shields.io/aur/version/paru?color=1793d1&label=paru&logo=arch-linux&style=for-the-badge)](https://aur.archlinux.org/packages/paru/)
[![paru-git](https://img.shields.io/aur/version/paru-git?color=1793d1&label=paru-git&logo=arch-linux&style=for-the-badge)](https://aur.archlinux.org/packages/paru-git/)

## Description

Paru is an AUR helper written in rust and based on the design of yay. It aims to be your standard pacman wrapping AUR helper with minimal interaction.

[![asciicast](https://asciinema.org/a/VtcPx2heIOEdoOoOkYXzSiToM.svg)](https://asciinema.org/a/VtcPx2heIOEdoOoOkYXzSiToM)

## Installation

```
sudo pacman -S --needed base-devel
git clone https://aur.archlinux.org/paru.git
cd paru
makepkg -si
```

## Common tips

- **Man pages**: For documentation on paru's options and config file see `paru(8)` and `paru.conf(5)` respectively.

- **Color**: Paru only enables color if color is enabled in pacman. Enable `color` in your `pacman.conf`.

- **File based review**: To get a more advanced review process enable `FileManager` with your file manager of choice in `paru.conf`.

- **Flip search order**: To get search results to start at the bottom and go upwards, enable `BottomUp` in `paru.conf`.

- **Editing pkgbuilds**: When editing pkgbuilds, you can commit your changes to make them perminent. When the package is upgraded, git will try and merge your changes with upstream's.

## Examples

`paru <target>` -- interactive search and install of `<target>`

`paru` -- aliased to `paru -Syu`

`paru -Sua` -- upgrade AUR packages

`paru -Qua` -- print AUR updates

`paru -G <target>` -- download `<target>`'s pkgbuild and related files

`paru -Gp <target>` -- print `<target>`'s pkgbuild

`paru --gendb` -- generate the devel database for tracking -git packages. This is only needed when you initially start using paru.


## Debugging

Paru is not an official tool. If paru can't build a package, you should first check if makepkg can successfully build the package. If it can't, then you should report the issue to the maintainer. Otherwise, it is likely an issue with paru and should be reported here.
