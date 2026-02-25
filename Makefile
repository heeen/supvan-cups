PREFIX   ?= /usr
BINDIR   ?= $(PREFIX)/bin
LIBDIR   ?= $(PREFIX)/lib/supvan-printer-app
DATADIR  ?= $(PREFIX)/share/supvan-printer-app
UNITDIR  ?= $(PREFIX)/lib/systemd/user
UDEVDIR  ?= $(PREFIX)/lib/udev/rules.d

BINARY   := target/release/supvan-printer-app

.PHONY: build install uninstall

build:
	cargo build --release --bin supvan-printer-app

install: build
	install -Dm755 $(BINARY)                              $(DESTDIR)$(BINDIR)/supvan-printer-app
	install -Dm644 data/models.toml                       $(DESTDIR)$(DATADIR)/models.toml
	install -Dm644 supvan-printer-app.service             $(DESTDIR)$(UNITDIR)/supvan-printer-app.service
	install -Dm755 etc/cups-cleanup.sh                    $(DESTDIR)$(LIBDIR)/cups-cleanup.sh
	install -Dm755 etc/cups-register.sh                   $(DESTDIR)$(LIBDIR)/cups-register.sh
	install -Dm644 etc/udev/rules.d/70-supvan-t50.rules  $(DESTDIR)$(UDEVDIR)/70-supvan-t50.rules

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/supvan-printer-app
	rm -f $(DESTDIR)$(DATADIR)/models.toml
	rm -f $(DESTDIR)$(UNITDIR)/supvan-printer-app.service
	rm -f $(DESTDIR)$(LIBDIR)/cups-cleanup.sh
	rm -f $(DESTDIR)$(LIBDIR)/cups-register.sh
	rm -f $(DESTDIR)$(UDEVDIR)/70-supvan-t50.rules
	-rmdir $(DESTDIR)$(DATADIR) $(DESTDIR)$(LIBDIR) 2>/dev/null
