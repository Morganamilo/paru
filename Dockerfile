FROM lopsided/archlinux:devel

WORKDIR /app

COPY ../ .

RUN pacman -Syu --noconfirm rust
RUN ls -la
RUN ./dist

