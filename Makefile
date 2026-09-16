# Vinilo — build and install to a personal (per-user) prefix.
#
# No sudo: this is a one-user, one-machine app (see CLAUDE.md), so everything
# lands under ~/.local, which is already on PATH and XDG_DATA_DIRS. Override
# PREFIX for a system install (make PREFIX=/usr/local install, with sudo).
#
# Unlike its siblings, Vinilo installs a *second* artefact: the Electron
# sidecar that owns DRM playback. It is ~200 MB of Chromium and is fetched by
# npm, never committed. The binary finds it via VINILO_SIDECAR, else
# $(DATADIR)/vinilo/sidecar, else ./sidecar for a dev tree.

PREFIX  ?= $(HOME)/.local
BINDIR   = $(PREFIX)/bin
DATADIR  = $(PREFIX)/share
APPID    = dev.danielmiguelt.Vinilo
# rustup may put cargo in ~/.cargo/bin *or* behind /usr/bin/cargo (Arch's
# rustup package). Prefer an explicit binary so `make` does not depend on
# fish_add_path having run in this shell.
CARGO ?= $(shell \
	if command -v cargo >/dev/null 2>&1; then command -v cargo; \
	elif [ -x "$(HOME)/.cargo/bin/cargo" ]; then echo "$(HOME)/.cargo/bin/cargo"; \
	elif command -v rustup >/dev/null 2>&1; then rustup which cargo 2>/dev/null; \
	fi)
# aguja has an id of its own because it is a separate program with a separate
# entry — a terminal one, launched by the desktop into a terminal.
AGUJA   = dev.danielmiguelt.Aguja
SIDECAR  = $(DATADIR)/vinilo/sidecar

ICON_SIZES = 16 32 48 64 128 256 512

.PHONY: all help build run test check sidecar sidecar-run gapless footprint install install-sidecar \
        dev-install update uninstall clean flatpak flatpak-bundle aur aur-publish
.DEFAULT_GOAL := help

help:
	@echo "Vinilo — reproductor de Apple Music para Linux"
	@echo
	@echo "Ejecuta estos comandos DENTRO de esta carpeta (donde está este Makefile),"
	@echo "no desde ~."
	@echo
	@echo "  make install    Compila e instala en ~/.local (sin sudo)"
	@echo "  make update     git pull (ignorando Cargo.lock local) e instala"
	@echo "  make run        Arranca la app GNOME desde el árbol de fuentes"
	@echo "  make check      fmt + clippy + tests"
	@echo "  make uninstall  Quita los binarios e iconos de ~/.local"
	@echo
	@echo "Después de instalar: vinilo   (o ábrelo desde la parrilla de apps)"
	@echo "Si fish no encuentra el comando:  fish_add_path ~/.local/bin"
	@echo "Si no hay compilador de Rust:     sudo pacman -S rustup && rustup default stable"
	@echo "Si git pull se queja de Cargo.lock:  make update"

all: build

build:
	@if [ -z "$(CARGO)" ] || { [ ! -x "$(CARGO)" ] && ! command -v "$(CARGO)" >/dev/null 2>&1; }; then \
		echo "No encuentro el compilador de Rust (cargo)."; \
		echo "En Arch no basta con el paquete «rust» si no está instalado. Instala rustup y un toolchain:"; \
		echo; \
		echo "  sudo pacman -S --needed rustup"; \
		echo "  rustup default stable"; \
		echo; \
		echo "Luego, en esta carpeta:  make install"; \
		exit 127; \
	fi
	$(CARGO) build --release

run:
	$(CARGO) run

test:
	$(CARGO) test

# The bar from CLAUDE.md. --all-targets so tests are linted too.
check:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings
	$(CARGO) test
	# The Flatpak build is offline and does not run on pull requests, so a
	# dependency added without regenerating the source list only fails after
	# a merge. Two files and a second, here instead.
	python3 packaging/flatpak/check-sources.py

# Fetch castLabs Electron. Two steps, both required: `npm install` brings down
# the ~14 MB wrapper, and install.js fetches the ~200 MB Chromium itself.
# castLabs ships no postinstall hook, so skipping the second step leaves you
# with a package that has no binary in it.
# (The Widevine CDM itself arrives later, at first run, via Chromium's
# component updater — that needs network too.)
sidecar:
	cd sidecar && npm install && node node_modules/electron/install.js

# Run the sidecar standalone with its window visible — the isolation step from
# CLAUDE.md. If a track plays here, DRM is fine and the bug is in the Rust side.
sidecar-run: sidecar
	cd sidecar && npm run debug

# Watch the audio stream across a track boundary. Run it in one terminal and
# `RUST_LOG=vinilo=info cargo run` in another — the log says whether Rust
# drove the transition, this says whether the decoder stopped.
gapless:
	./scripts/gapless-check.sh

# What the app costs the machine: memory, CPU and disk. Needs a running
# instance for the first two; `--disk` alone needs nothing.
footprint:
	./scripts/footprint.sh

# A native `flatpak-builder` if there is one, otherwise the Flathub app. They
# are the same tool; the difference is that the flatpak'd one runs sandboxed,
# and on a CI runner that sandbox cannot see runtimes installed into the user
# installation — it fails with `Unable to find sdk org.gnome.Sdk version 49`
# twenty seconds after installing exactly that.
FLATPAK_BUILDER := $(shell command -v flatpak-builder >/dev/null 2>&1 \
	&& echo flatpak-builder || echo flatpak run org.flatpak.Builder)

flatpak:
	$(FLATPAK_BUILDER) --force-clean --user --install \
		--repo=flatpak-repo build-dir packaging/flatpak/dev.danielmiguelt.Vinilo.yml
	test -f build-dir/files/share/vinilo/sidecar/queue-identity.js

# `--runtime-repo` is the difference between a bundle that installs and one
# that stops with "requires the runtime org.gnome.Platform/x86_64/49 which was
# not found". A .flatpak carries the *app* and never the runtime, so on a
# machine with no Flathub remote there is nothing for it to sit on and flatpak
# has no idea where to look. The URL is recorded inside the bundle, so
# installing it offers to add Flathub and pull the runtime itself.
#
# Found by installing on a clean Ubuntu VM, which is the only place it could
# have been found: every machine that has ever built this already had the
# runtime.
flatpak-bundle: flatpak
	flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
		flatpak-repo Vinilo.flatpak dev.danielmiguelt.Vinilo master
	@echo "Vinilo.flatpak — copy it anywhere and: flatpak install ./Vinilo.flatpak"

# Show what publishing to the AUR would do, without doing it. `make aur-publish`
# is the same thing with --push. Deliberately local rather than a CI job: the
# key that can publish under your name should not live in a repository secret.
aur:
	./scripts/aur-publish.sh vinilo
	./scripts/aur-publish.sh vinilo-git

aur-publish:
	./scripts/aur-publish.sh vinilo --push
	./scripts/aur-publish.sh vinilo-git --push

install: build install-sidecar
	@if git rev-parse --is-inside-work-tree >/dev/null 2>&1 \
		&& ! git diff --quiet -- Cargo.lock 2>/dev/null; then \
		echo "AVISO: Cargo.lock está modificado. Si querías la última versión: make update"; \
	fi
	install -Dm755 target/release/vinilo $(BINDIR)/vinilo
	install -Dm755 target/release/vinilod $(BINDIR)/vinilod
	install -Dm755 target/release/aguja $(BINDIR)/aguja
	$(MAKE) dev-install
	@echo "Installed to $(PREFIX)."
	@echo "Launch Vinilo from the app grid, or run 'vinilo' — or 'aguja' for the terminal."
	@echo "If the grid still does nothing: log out and back in, then cat ~/.cache/vinilo/launcher.log"

install-sidecar: sidecar
	install -d $(SIDECAR)
	cp -r sidecar/package.json sidecar/main.js sidecar/preload.js \
		sidecar/queue-identity.js \
		sidecar/node_modules $(SIDECAR)/

# Everything except the binaries: the .desktop entry and the icons.
# Not a way to get a dev-mode icon — on Wayland only the fully installed app
# shows one.
#
# `@BINDIR@` in the desktop files is replaced with the real prefix. GNOME
# launches from a session PATH that often lacks ~/.local/bin, so `Exec=vinilo`
# is a no-op from the app grid while the same command works in a terminal.
dev-install:
	install -Dm755 data/vinilo-desktop $(BINDIR)/vinilo-desktop
	install -d $(DATADIR)/applications
	sed -e 's|@BINDIR@|$(BINDIR)|g' data/$(APPID).desktop \
		> $(DATADIR)/applications/$(APPID).desktop
	sed -e 's|@BINDIR@|$(BINDIR)|g' data/$(AGUJA).desktop \
		> $(DATADIR)/applications/$(AGUJA).desktop
	chmod 644 $(DATADIR)/applications/$(APPID).desktop \
		$(DATADIR)/applications/$(AGUJA).desktop
	install -d $(DATADIR)/systemd/user
	sed -e 's|@BINDIR@|$(BINDIR)|g' packaging/systemd/vinilod.service \
		> $(DATADIR)/systemd/user/vinilod.service
	chmod 644 $(DATADIR)/systemd/user/vinilod.service
	install -d $(DATADIR)/dbus-1/services
	sed -e 's|@BINDIR@|$(BINDIR)|g' packaging/dbus/$(APPID).service \
		> $(DATADIR)/dbus-1/services/$(APPID).service
	chmod 644 $(DATADIR)/dbus-1/services/$(APPID).service
	@# Leftover names from earlier installs: GNOME may keep showing those
	@# icons, and they still say `Exec=vinilo`.
	rm -f $(DATADIR)/applications/vinilo.desktop \
		$(DATADIR)/applications/slipmat.desktop \
		$(DATADIR)/applications/Slipmat.desktop
	install -Dm644 data/icons/hicolor/scalable/apps/$(APPID).svg \
		$(DATADIR)/icons/hicolor/scalable/apps/$(APPID).svg
	install -Dm644 data/icons/hicolor/scalable/apps/$(AGUJA).svg \
		$(DATADIR)/icons/hicolor/scalable/apps/$(AGUJA).svg
	install -Dm644 data/icons/hicolor/symbolic/apps/$(APPID)-symbolic.svg \
		$(DATADIR)/icons/hicolor/symbolic/apps/$(APPID)-symbolic.svg
	install -Dm644 data/icons/hicolor/symbolic/actions/vinilo-lyrics-symbolic.svg \
		$(DATADIR)/icons/hicolor/symbolic/actions/vinilo-lyrics-symbolic.svg
	@# Raster sizes, rendered from the same SVG the app installs so the two
	@# can never drift. GTK resolves the SVG on its own, but the shell, the
	@# notification daemon and anything reading the icon theme without an SVG
	@# loader all want PNGs — and this loop used to look for files that were
	@# never in the tree, so it silently installed none.
	@if command -v rsvg-convert >/dev/null 2>&1; then \
		for id in $(APPID) $(AGUJA); do \
			for sz in $(ICON_SIZES); do \
				install -d $(DATADIR)/icons/hicolor/$${sz}x$${sz}/apps; \
				rsvg-convert -w $${sz} -h $${sz} \
					data/icons/hicolor/scalable/apps/$${id}.svg \
					-o $(DATADIR)/icons/hicolor/$${sz}x$${sz}/apps/$${id}.png; \
			done; \
		done; \
		echo "Rendered PNG icons: $(ICON_SIZES)"; \
	else \
		echo "rsvg-convert not found — installing the SVG only."; \
	fi
	@if [ -f $(DATADIR)/icons/hicolor/index.theme ]; then \
		touch $(DATADIR)/icons/hicolor; \
		gtk-update-icon-cache -q -t -f $(DATADIR)/icons/hicolor; \
	fi
	-update-desktop-database -q $(DATADIR)/applications
	@if grep -q '@BINDIR@' $(DATADIR)/applications/$(APPID).desktop; then \
		echo "error: @BINDIR@ was not substituted in $(APPID).desktop"; \
		exit 1; \
	fi
	@echo "Desktop Exec: $$(grep '^Exec=' $(DATADIR)/applications/$(APPID).desktop)"
	@echo "If the app grid still does nothing: grep Exec $(DATADIR)/applications/$(APPID).desktop"
	@echo "and check ~/.cache/vinilo/launcher.log after a click."

# Pull this tree and reinstall. A local `cargo build` often dirties
# Cargo.lock; that is exactly what blocked `git pull` and left the app
# grid launching an old `Exec=vinilo` desktop file (a no-op on GNOME's PATH).
update:
	git restore -- Cargo.lock 2>/dev/null || git checkout -- Cargo.lock
	# Whatever this branch tracks — often `github/main`, not Cursor.
	# Hardcoding `origin` aborts on a clone whose two remotes diverged.
	git pull --ff-only
	$(MAKE) install

uninstall:
	rm -f $(BINDIR)/vinilo
	rm -f $(BINDIR)/vinilo-desktop
	rm -f $(BINDIR)/vinilod
	rm -f $(BINDIR)/aguja
	rm -rf $(DATADIR)/vinilo
	rm -f $(DATADIR)/systemd/user/vinilod.service
	rm -f $(DATADIR)/dbus-1/services/$(APPID).service
	rm -f $(DATADIR)/applications/$(APPID).desktop
	rm -f $(DATADIR)/applications/$(AGUJA).desktop
	rm -f $(DATADIR)/applications/vinilo.desktop \
		$(DATADIR)/applications/slipmat.desktop \
		$(DATADIR)/applications/Slipmat.desktop
	rm -f $(DATADIR)/icons/hicolor/scalable/apps/$(APPID).svg
	rm -f $(DATADIR)/icons/hicolor/scalable/apps/$(AGUJA).svg
	rm -f $(DATADIR)/icons/hicolor/symbolic/apps/$(APPID)-symbolic.svg
	rm -f $(DATADIR)/icons/hicolor/symbolic/actions/vinilo-lyrics-symbolic.svg
	@for sz in $(ICON_SIZES); do \
		rm -f $(DATADIR)/icons/hicolor/$${sz}x$${sz}/apps/$(APPID).png; \
		rm -f $(DATADIR)/icons/hicolor/$${sz}x$${sz}/apps/$(AGUJA).png; \
	done
	@if [ -f $(DATADIR)/icons/hicolor/index.theme ]; then \
		gtk-update-icon-cache -q -t -f $(DATADIR)/icons/hicolor; \
	fi
	-update-desktop-database -q $(DATADIR)/applications
	@echo "Uninstalled from $(PREFIX)."

clean:
	$(CARGO) clean
	rm -rf sidecar/node_modules
