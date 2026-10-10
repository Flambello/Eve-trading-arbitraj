# Makefile for EVE Traveling Merchant on Linux

.PHONY: all check test clippy fmt build release run clean install install-deps validate-desktop generate-sources dist

all: check test release

# Install dependencies using Fedora's DNF package manager
install-deps:
	sudo dnf install -y gcc pkg-config git rust cargo desktop-file-utils appstream

check:
	cargo check --all-targets

test:
	cargo test --verbose

clippy:
	cargo clippy --all-targets -- -D warnings

fmt:
	cargo fmt

build:
	cargo build

release:
	cargo build --release --bin eve-merchant

run:
	./target/release/eve-merchant --gui

validate-desktop:
	desktop-file-validate packaging/flatpak/io.github.flambello.eve_merchant.desktop
	appstreamcli validate --no-net packaging/flatpak/io.github.flambello.eve_merchant.metainfo.xml

generate-sources:
	./packaging/flatpak/generate-sources.sh

dist: release
	rm -rf dist-tmp eve-traveling-merchant-linux-x86_64.tar.gz
	mkdir -p dist-tmp/eve-traveling-merchant
	cp target/release/eve-merchant dist-tmp/eve-traveling-merchant/
	cp README.md LICENSE CHANGELOG.md dist-tmp/eve-traveling-merchant/
	cp packaging/flatpak/io.github.flambello.eve_merchant.desktop dist-tmp/eve-traveling-merchant/
	cp assets/icons/io.github.flambello.eve_merchant.svg dist-tmp/eve-traveling-merchant/
	cp assets/icons/io.github.flambello.eve_merchant.png dist-tmp/eve-traveling-merchant/
	cp start-gui.sh dist-tmp/eve-traveling-merchant/
	chmod +x dist-tmp/eve-traveling-merchant/eve-merchant dist-tmp/eve-traveling-merchant/start-gui.sh
	tar -czf eve-traveling-merchant-linux-x86_64.tar.gz -C dist-tmp eve-traveling-merchant
	rm -rf dist-tmp
	sha256sum eve-traveling-merchant-linux-x86_64.tar.gz > SHA256SUMS
	@echo "Distribution archive created: eve-traveling-merchant-linux-x86_64.tar.gz"

clean:
	cargo clean

install:
	cargo install --path .
