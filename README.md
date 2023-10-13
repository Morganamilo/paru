# Paru

Feature packed AUR helper

[![paru](https://img.shields.io/aur/version/paru?color=1793d1&label=paru&logo=arch-linux&style=for-the-badge)](https://aur.archlinux.org/packages/paru/)
[![paru-bin](https://img.shields.io/aur/version/paru-bin?color=1793d1&label=paru-bin&logo=arch-linux&style=for-the-badge)](https://aur.archlinux.org/packages/paru-bin/)
[![paru-git](https://img.shields.io/aur/version/paru-git?color=1793d1&label=paru-git&logo=arch-linux&style=for-the-badge)](https://aur.archlinux.org/packages/paru-git/)

## Description

Paru is your standard pacman wrapping AUR helper with lots of features and minimal interaction.

[![asciicast](https://asciinema.org/a/sEh1ZpZZUgXUsgqKxuDdhpdEE.svg)](https://asciinema.org/a/sEh1ZpZZUgXUsgqKxuDdhpdEE)

## Installation

```
sudo pacman -S --needed base-devel
git clone https://aur.archlinux.org/paru.git
cd paru
makepkg -si
```

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md).

## General Tips

- **Man pages**: For documentation on paru's options and config file see `paru(8)` and `paru.conf(5)` respectively.

- **Color**: Paru only enables color if color is enabled in pacman. Enable `color` in your `pacman.conf`.

- **File based review**: To get a more advanced review process enable `FileManager` with your file manager of choice in `paru.conf`.

- **Flip search order**: To get search results to start at the bottom and go upwards, enable `BottomUp` in `paru.conf`.

- **Editing PKGBUILDs**: When editing PKGBUILDs, you can commit your changes to make them permanent. When the package is upgraded, `git` will try to merge your changes with upstream's.

- **PKGBUILD syntax highlighting**: You can install [`bat`](https://github.com/sharkdp/bat) to enable syntax highlighting during PKGBUILD review.

- **Tracking -git packages**: Paru tracks -git package by monitoring the upstream repository. Paru can only do this for packages that paru itself installed. `paru --gendb` will make paru aware of packages it did not install.

## Examples

`paru <target>` -- Interactively search and install `<target>`.

`paru` -- Alias for `paru -Syu`.

`paru -S <target>` -- Install a specific package.

`paru -Sua` -- Upgrade AUR packages.

`paru -Qua` -- Print available AUR updates.

`paru -G <target>` -- Download the PKGBUILD and related files of `<target>`.

`paru -Gp <target>` -- Print the PKGBUILD of `<target>`.

`paru -Gc <target>` -- Print the AUR comments  of `<target>`.

`paru --gendb` -- Generate the devel database for tracking `*-git` packages. This is only needed when you initially start using paru.

`paru -Bi .` -- Build and install a PKGBUILD in the current directory.

## IRC

Paru now has an IRC. #paru on [Libera Chat](https://libera.chat/). Feel free to join for discussion and help with paru.

## Debugging

Paru is not an official tool. If paru can't build a package, you should first check if makepkg can successfully build the package. If it can't, then you should report the issue to the maintainer. Otherwise, it is likely an issue with paru and should be reported here.
