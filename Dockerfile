FROM lopsided/archlinux:devel

WORKDIR /app

COPY ../ .

RUN echo "keyserver hkp://keyserver.ubuntu.com" >> /etc/pacman.d/gnupg/gpg.conf
RUN pacman -Sy --noconfirm archlinux-keyring
RUN pacman -Su --noconfirm rust
RUN ls -la
RUN ./dist

