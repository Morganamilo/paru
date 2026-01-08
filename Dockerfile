FROM debian:13 AS build-stage

ARG PACMAN_HASH=b9f7d4a5b0bea75953f5892621a2caecc5672de5
ARG PACMAN_VER=7.1.0
ARG DEBIAN_FRONTEND=noninteractive

WORKDIR /pacman

RUN apt-get update -y
RUN apt-get install -y build-essential git libcurl4-openssl-dev curl meson ninja-build \
        libarchive-dev pkg-config libgpgme-dev libssl-dev clang python3 python3-setuptools \
        gettext zstd

RUN curl -L -o pacman-${PACMAN_VER}.tar.xz https://gitlab.archlinux.org/pacman/pacman/-/archive/${PACMAN_HASH}/pacman-${PACMAN_HASH}.tar.gz
RUN tar -xf pacman-${PACMAN_VER}.tar.xz

WORKDIR pacman-${PACMAN_HASH}

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
