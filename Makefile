PKG_CONFIG_PATH := /usr/lib/x86_64-linux-gnu/pkgconfig:/usr/lib/pkgconfig:/usr/share/pkgconfig
export PKG_CONFIG_PATH

INSTALL_PREFIX := $(HOME)/.local
VERSION := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')

.PHONY: build run check clean install uninstall appimage release

build:
	cargo build

run:
	cargo run

check:
	cargo check

clean:
	cargo clean

install: build
	cargo build --release
	install -Dm755 target/release/lumen-node $(INSTALL_PREFIX)/bin/lumen-node
	install -Dm644 data/com.lumennode.app.desktop $(INSTALL_PREFIX)/share/applications/com.lumennode.app.desktop
	install -Dm644 data/icons/com.lumennode.app.svg $(INSTALL_PREFIX)/share/icons/hicolor/scalable/apps/com.lumennode.app.svg
	install -Dm644 data/com.lumennode.app.metainfo.xml $(INSTALL_PREFIX)/share/metainfo/com.lumennode.app.metainfo.xml
	gtk-update-icon-cache -f -t $(INSTALL_PREFIX)/share/icons/hicolor || true

uninstall:
	rm -f $(INSTALL_PREFIX)/bin/lumen-node
	rm -f $(INSTALL_PREFIX)/share/applications/com.lumennode.app.desktop
	rm -f $(INSTALL_PREFIX)/share/icons/hicolor/scalable/apps/com.lumennode.app.svg
	rm -f $(INSTALL_PREFIX)/share/metainfo/com.lumennode.app.metainfo.xml

appimage:
	bash packaging/build-appimage.sh

release: appimage
	git tag -a v$(VERSION) -m "Release v$(VERSION)"
	git push origin v$(VERSION)
	gh release create v$(VERSION) \
		packaging/LumenNode-x86_64.AppImage \
		--title "LumenNode v$(VERSION)" \
		--generate-notes
