FROM debian:unstable
ARG PACMAN_VER=6.0.2

RUN apt-get update -y
RUN apt-get install -y build-essential git libcurl4-openssl-dev curl meson ninja-build\
        libarchive-dev pkg-config libgpgme-dev libssl-dev

RUN curl -O https://sources.archlinux.org/other/pacman/pacman-${PACMAN_VER}.tar.xz
RUN tar -xf pacman-${PACMAN_VER}.tar.xz
WORKDIR pacman-${PACMAN_VER}
RUN meson setup --prefix=/usr \
        --buildtype=plain \
        -Dscriptlet-shell=/usr/bin/bash \
        -Dldconfig=/usr/bin/ldconfig \
        build

RUN meson compile -C build
RUN meson compile -C build install

