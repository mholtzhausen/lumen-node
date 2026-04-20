PKG_CONFIG_PATH := /usr/lib/x86_64-linux-gnu/pkgconfig:/usr/lib/pkgconfig:/usr/share/pkgconfig
export PKG_CONFIG_PATH

.PHONY: build run check clean

build:
	cargo build

run:
	cargo run

check:
	cargo check

clean:
	cargo clean
