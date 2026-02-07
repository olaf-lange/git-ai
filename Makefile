.PHONY: all build test bench lint doc format update

all: build test lint doc

build:
	cargo build

test:
	cargo test

bench:
	cargo bench

lint:
	cargo check
	cargo clippy

doc:
	cargo doc

format:
	cargo fmt

update:
	cargo update
