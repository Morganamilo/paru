DESTDIR = /usr/local
BINDIR = $(DESTDIR)/bin
MANDIR = $(DESTDIR)/share/man

paru:
	@set -o errexit -o pipefail
	@cargo build --release --locked --target-dir target
	@strip target/release/paru
	@tar --zstd -cfparu.tar.zst  man completions paru.conf -C target/release paru

clean:
	@cargo clean

install: paru
	@mkdir -pv $(BINDIR)
	@cp -fv target/release/paru $(BINDIR)
	@chmod 755 $(BINDIR)/paru
	@mkdir -pv $(MANDIR)
	@cp -fv man/paru.conf.5 $(MANDIR)/man5
	@chmod 644 $(MANDIR)/man5/paru.conf.5
	@cp -fv man/paru.8 $(MANDIR)/man8
	@chmod 644 $(MANDIR)/man8/paru.8

uninstall:
	@rm -rfv $(BINDIR)/paru
	@rm -rfv $(MANDIR)/man5/paru.conf.5
	@rm -rfv $(MANDIR)/man8/paru.8

.PHONY: paru clean install uninstall
