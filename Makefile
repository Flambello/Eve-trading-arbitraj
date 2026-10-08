# Makefile for EVE Trade Scout on Fedora Linux

.PHONY: all check test build release run clean install-deps

all: check test release

# Install dependencies using Fedora's DNF package manager
install-deps:
	sudo dnf install -y gcc pkg-config git rust cargo

check:
	cargo check

test:
	cargo test --verbose

build:
	cargo build

release:
	cargo build --release

run:
	./target/release/eve-trade

clean:
	cargo clean

install:
	cargo install --path .
