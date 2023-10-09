FROM debian:unstable as build-stage

ARG PACMAN_VER=6.0.2
ARG DEBIAN_FRONTEND=noninteractive

WORKDIR /pacman

RUN apt-get update -y
RUN apt-get install -y build-essential git libcurl4-openssl-dev curl meson ninja-build \
        libarchive-dev pkg-config libgpgme-dev libssl-dev clang python3 python3-setuptools \
        gettext zstd

RUN curl -O https://sources.archlinux.org/other/pacman/pacman-${PACMAN_VER}.tar.xz
RUN tar -xf pacman-${PACMAN_VER}.tar.xz

WORKDIR pacman-${PACMAN_VER}

RUN meson setup \
        --prefix=/usr \
        --buildtype=plain \
        build

RUN ninja -C build
RUN ninja -C build install

WORKDIR /paru

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
     sh -s -- -y --no-modify-path --profile minimal --default-toolchain nightly

COPY ../ .

RUN --mount=type=tmpfs,target=/usr/local/cargo/git ./scripts/dist

FROM scratch AS export-stage
COPY --from=build-stage /paru/paru.tar.zst /
