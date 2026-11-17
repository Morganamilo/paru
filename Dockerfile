ARG BASE_IMAGE=docker.io/archlinux:base-devel@sha256:fa2d4ce3310ddd90a4208e5168be8ad367da5129af1245f38fd2d803bcbe3239
FROM ${BASE_IMAGE} AS build-stage

# QEMU user emulation does not support pacman's sandbox syscalls.
RUN pacman-key --init && pacman-key --populate && \
    pacman --disable-sandbox -Syu --noconfirm --needed \
        ca-certificates clang gettext git openssl pacman pkgconf python rust zstd

ENV CARGO_HOME=/usr/local/cargo

WORKDIR /paru

COPY . .

RUN --mount=type=tmpfs,target=/usr/local/cargo/git \
    OPENSSL_STATIC=0 ./scripts/dist && \
    python3 scripts/check-release.py target/release/paru paru.tar.zst

FROM scratch AS export-stage
COPY --from=build-stage /paru/paru.tar.zst /
