# supvan-printer-app — build, test, and install.
#
#   make            # build (release)
#   make test       # run the test suite
#   make deploy     # user-scoped install + (re)start the service (no sudo)
#   sudo make install   # system-wide install (FHS, honours DESTDIR/PREFIX)
#   make help       # list all targets

CARGO ?= cargo

# --- System install layout (GNU/FHS; override PREFIX or DESTDIR) ------------
PREFIX   ?= /usr
BINDIR   ?= $(PREFIX)/bin
LIBDIR   ?= $(PREFIX)/lib/supvan-printer-app
DATADIR  ?= $(PREFIX)/share/supvan-printer-app
UNITDIR  ?= $(PREFIX)/lib/systemd/user
UDEVDIR  ?= $(PREFIX)/lib/udev/rules.d
DBUSDIR  ?= /etc/dbus-1/system.d

# --- User install layout (XDG; no privileges) ------------------------------
APP_CRATE    := crates/supvan-app
CARGO_BIN    := $(HOME)/.cargo/bin
USER_UNITDIR := $(HOME)/.config/systemd/user
USER_LIBDIR  := $(HOME)/.local/lib/supvan-printer-app

BINARY      := target/release/supvan-printer-app
BINARY_CLI  := target/release/supvan-cli

.PHONY: all build debug check test fmt fmt-check clippy lint clean run \
        install uninstall install-user deploy uninstall-user help

all: build ## Build the release binaries (default)

# --- Development ------------------------------------------------------------
build: ## cargo build --release
	$(CARGO) build --release

debug: ## cargo build (debug)
	$(CARGO) build

check: ## cargo check (all targets)
	$(CARGO) check --all-targets

test: ## Run the test suite
	$(CARGO) test

fmt: ## Format the code in place
	$(CARGO) fmt

fmt-check: ## Verify formatting (CI)
	$(CARGO) fmt --check

clippy: ## Run clippy (all targets)
	$(CARGO) clippy --all-targets

lint: fmt-check clippy ## fmt-check + clippy

clean: ## Remove build artifacts
	$(CARGO) clean

run: ## Run the app locally (set SUPVAN_MOCK=1 for a synthetic device)
	$(CARGO) run -p supvan-app

# --- System install (root; FHS) --------------------------------------------
install: $(BINARY) $(BINARY_CLI) ## System-wide install (sudo; DESTDIR/PREFIX aware)
	install -Dm755 $(BINARY)                                    $(DESTDIR)$(BINDIR)/supvan-printer-app
	install -Dm755 $(BINARY_CLI)                                $(DESTDIR)$(BINDIR)/supvan-cli
	install -Dm644 data/models.toml                             $(DESTDIR)$(DATADIR)/models.toml
	install -Dm644 supvan-printer-app.service                   $(DESTDIR)$(UNITDIR)/supvan-printer-app.service
	install -Dm755 etc/cups-cleanup.sh                          $(DESTDIR)$(LIBDIR)/cups-cleanup.sh
	install -Dm644 etc/udev/rules.d/70-supvan-t50.rules         $(DESTDIR)$(UDEVDIR)/70-supvan-t50.rules
	install -Dm644 etc/dbus-1/system.d/com.supvan.battery.conf  $(DESTDIR)$(DBUSDIR)/com.supvan.battery.conf

uninstall: ## Remove a system-wide install
	-sh etc/cups-cleanup.sh   # remove the persistent CUPS queue(s)
	rm -f $(DESTDIR)$(BINDIR)/supvan-printer-app
	rm -f $(DESTDIR)$(BINDIR)/supvan-cli
	rm -f $(DESTDIR)$(DATADIR)/models.toml
	rm -f $(DESTDIR)$(UNITDIR)/supvan-printer-app.service
	rm -f $(DESTDIR)$(LIBDIR)/cups-cleanup.sh
	rm -f $(DESTDIR)$(UDEVDIR)/70-supvan-t50.rules
	rm -f $(DESTDIR)$(DBUSDIR)/com.supvan.battery.conf
	-rmdir $(DESTDIR)$(DATADIR) $(DESTDIR)$(LIBDIR) 2>/dev/null

# --- User install (no privileges) ------------------------------------------
install-user: ## Install binary + user service into $HOME (no sudo)
	$(CARGO) install --path $(APP_CRATE) --force
	install -Dm755 etc/cups-cleanup.sh $(USER_LIBDIR)/cups-cleanup.sh
	install -Dm644 etc/supvan-printer-app.user.service \
		$(USER_UNITDIR)/supvan-printer-app.service
	systemctl --user daemon-reload

deploy: install-user ## install-user, then enable + (re)start the user service
	systemctl --user enable supvan-printer-app
	systemctl --user restart supvan-printer-app
	@systemctl --user --no-pager is-active supvan-printer-app

uninstall-user: ## Remove the user install
	-systemctl --user disable --now supvan-printer-app
	-sh etc/cups-cleanup.sh   # remove the persistent CUPS queue(s)
	-$(CARGO) uninstall supvan-app 2>/dev/null
	rm -f $(USER_UNITDIR)/supvan-printer-app.service
	rm -f $(USER_LIBDIR)/cups-cleanup.sh
	-rmdir $(USER_LIBDIR) 2>/dev/null
	systemctl --user daemon-reload

help: ## List targets
	@grep -hE '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) \
		| awk 'BEGIN{FS=":.*?## "}{printf "  \033[36m%-14s\033[0m %s\n", $$1, $$2}'
