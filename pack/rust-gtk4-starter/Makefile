# pkg-config paths for Debian/Ubuntu multi-arch GTK installs.
PKG_CONFIG_PATH := /usr/lib/x86_64-linux-gnu/pkgconfig:/usr/lib/pkgconfig:/usr/share/pkgconfig
export PKG_CONFIG_PATH

INSTALL_PREFIX := $(HOME)/.local
APP_ID := com.example.GtkStarter
BINARY := gtk-starter
VERSION := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')

.PHONY: build run check clean install uninstall run-dev-isolated

build:
	cargo build

run:
	cargo run

run-dev-isolated: build
	GTK_STARTER_APP_ID=$(APP_ID).dev GTK_STARTER_NON_UNIQUE=1 cargo run

check:
	cargo check

clean:
	cargo clean

install: build
	cargo build --release
	install -Dm755 target/release/$(BINARY) $(INSTALL_PREFIX)/bin/$(BINARY)
	install -Dm644 data/$(APP_ID).desktop $(INSTALL_PREFIX)/share/applications/$(APP_ID).desktop
	install -Dm644 data/icons/$(APP_ID).svg $(INSTALL_PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	install -Dm644 data/$(APP_ID).metainfo.xml $(INSTALL_PREFIX)/share/metainfo/$(APP_ID).metainfo.xml
	gtk-update-icon-cache -f -t $(INSTALL_PREFIX)/share/icons/hicolor || true

uninstall:
	rm -f $(INSTALL_PREFIX)/bin/$(BINARY)
	rm -f $(INSTALL_PREFIX)/share/applications/$(APP_ID).desktop
	rm -f $(INSTALL_PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	rm -f $(INSTALL_PREFIX)/share/metainfo/$(APP_ID).metainfo.xml
