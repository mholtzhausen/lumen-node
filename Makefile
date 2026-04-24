PKG_CONFIG_PATH := /usr/lib/x86_64-linux-gnu/pkgconfig:/usr/lib/pkgconfig:/usr/share/pkgconfig
export PKG_CONFIG_PATH

INSTALL_PREFIX := $(HOME)/.local
VERSION := $(shell grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')

.PHONY: build run check clean install uninstall appimage appimage-audit run-release-isolated run-release-diagnose release release-preflight

build:
	cargo build

run:
	cargo run

run-release-isolated: build
	LUMEN_NODE_APP_ID=com.lumennode.app.dev LUMEN_NODE_NON_UNIQUE=1 target/release/lumen-node

run-release-diagnose: build
	LUMEN_NODE_PIN_THEME=0 LUMEN_NODE_PIN_SCALE=0 target/release/lumen-node

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

appimage-audit: appimage
	bash packaging/audit-appdir.sh

release-preflight:
	bash scripts/release-preflight.sh

release: release-preflight appimage
	@LATEST_APPIMAGE=$$(ls -1t packaging/LumenNode-x86_64-*.AppImage | sed -n '1p'); \
	if [ -z "$$LATEST_APPIMAGE" ]; then \
		echo "No timestamped AppImage found in packaging/"; \
		exit 1; \
	fi; \
	@if git tag | grep -q "^v$(VERSION)$$"; then \
		echo "Tag v$(VERSION) already exists. Skipping tag creation."; \
	else \
		git tag -a v$(VERSION) -m "Release v$(VERSION)"; \
		git push origin v$(VERSION); \
	fi
	@if gh release view v$(VERSION) >/dev/null 2>&1; then \
		echo "GitHub release v$(VERSION) already exists. Skipping release creation."; \
	else \
		gh release create v$(VERSION) \
			$$LATEST_APPIMAGE \
			--title "LumenNode v$(VERSION)" \
			--generate-notes; \
	fi
