FROM lopsided/archlinux:devel

WORKDIR /app

COPY ../ .

RUN pacman -Sy --noconfirm archlinux-keyring
RUN pacman -Su --noconfirm rust
RUN ./scripts/dist

