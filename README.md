# Paru

An AUR helper and pacman wrapper

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

- **Man pages**: For documentation on paru's options and config file see `paru(8)` and `paru.conf(5)` respectfully.

- **Color**: Paru only enables color if color is enabled in pacman. Enable `color` in your `pacman.conf`.

- **File based review**: To get a more advanced review process enable `FileManager` with your file manager of choice in `paru.conf`.

- **Flip search order**: To get search results to start at the bottom and go upwards, enable `BottomUp` in `paru.conf`.

## Examples

`paru <target>` -- interactive search and install of `<target>`

`paru` -- aliased to `paru -Syu`

`paru -Sua` -- upgrade AUR packages

`paru -Qua` -- print AUR updates

`paru -G <target>` -- download `<target>`'s pkgbuild and related files

`paru -Gp <target>` -- print `<target>`'s pkgbuild

`paru --gendb` -- generate the devel database for tracking -git packages. This is only needed when you initially start using paru.


## Debugging

Paru is not offical tool. If paru can not build a package you should first check if makepkg can sucessfully build the package. If it can't then you should report the issue to the maintainer. Otherwise it is likley an issue with paru and should be repported here.
