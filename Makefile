PREFIX   ?= /usr
BINDIR   ?= $(PREFIX)/bin
LIBDIR   ?= $(PREFIX)/lib/supvan-printer-app
DATADIR  ?= $(PREFIX)/share/supvan-printer-app
UNITDIR  ?= $(PREFIX)/lib/systemd/user
UDEVDIR  ?= $(PREFIX)/lib/udev/rules.d
DBUSDIR  ?= /etc/dbus-1/system.d

BINARY      := target/release/supvan-printer-app
BINARY_CLI  := target/release/supvan-cli

.PHONY: build install uninstall

build:
	cargo build --release

install: $(BINARY) $(BINARY_CLI)
	install -Dm755 $(BINARY)                              $(DESTDIR)$(BINDIR)/supvan-printer-app
	install -Dm755 $(BINARY_CLI)                          $(DESTDIR)$(BINDIR)/supvan-cli
	install -Dm644 data/models.toml                       $(DESTDIR)$(DATADIR)/models.toml
	install -Dm644 supvan-printer-app.service             $(DESTDIR)$(UNITDIR)/supvan-printer-app.service
	install -Dm755 etc/cups-cleanup.sh                    $(DESTDIR)$(LIBDIR)/cups-cleanup.sh
	install -Dm755 etc/cups-register.sh                   $(DESTDIR)$(LIBDIR)/cups-register.sh
	install -Dm644 etc/udev/rules.d/70-supvan-t50.rules  $(DESTDIR)$(UDEVDIR)/70-supvan-t50.rules
	install -Dm644 etc/dbus-1/system.d/com.supvan.battery.conf $(DESTDIR)$(DBUSDIR)/com.supvan.battery.conf

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/supvan-printer-app
	rm -f $(DESTDIR)$(BINDIR)/supvan-cli
	rm -f $(DESTDIR)$(DATADIR)/models.toml
	rm -f $(DESTDIR)$(UNITDIR)/supvan-printer-app.service
	rm -f $(DESTDIR)$(LIBDIR)/cups-cleanup.sh
	rm -f $(DESTDIR)$(LIBDIR)/cups-register.sh
	rm -f $(DESTDIR)$(UDEVDIR)/70-supvan-t50.rules
	rm -f $(DESTDIR)$(DBUSDIR)/com.supvan.battery.conf
	-rmdir $(DESTDIR)$(DATADIR) $(DESTDIR)$(LIBDIR) 2>/dev/null
