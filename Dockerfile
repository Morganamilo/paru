FROM lopsided/archlinux:devel

WORKDIR /app

COPY ../ .

RUN echo "keyserver hkp://keyserver.ubuntu.com" >> /etc/pacman.d/gnupg/gpg.conf
RUN df -h
RUN sed -i "s/CheckSpace/#CheckSpace" /etc/pacman.conf
RUN pacman -Sy --noconfirm archlinux-keyring
RUN df -h
RUN pacman -Su --noconfirm rust
RUN ls -la
RUN ./dist

