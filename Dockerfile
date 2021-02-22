FROM lopsided/archlinux:devel

WORKDIR /app

COPY ../ .

RUN echo "keyserver hkp://keyserver.ubuntu.com" >> /etc/pacman.d/gnupg/gpg.conf
RUN df -h
RUN grep "[[:space:]]ro[[:space:],]" /proc/mounts 
RUN sed -i 's/CheckSpace/#CheckSpace/g' /etc/pacman.conf
RUN pacman -Sy --noconfirm archlinux-keyring
RUN pacman -Su --noconfirm rust
RUN ./dist

