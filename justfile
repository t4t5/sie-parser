default: build

build:
    cargo build --release

test:
    cargo test

fmt:
    cargo fmt --all

clippy:
    cargo clippy --all-targets -- -D warnings
