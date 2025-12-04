alias w := watch

watch *args="check":
    cargo watch -c -- just {{args}}

check:
    cargo check

server *args:
    cargo run -- server {{args}}

clippy:
    cargo clippy -- -D warnings

fmt:
    cargo fmt

deps:
    sudo apt-get install libasound2-dev
    cargo install --locked cross

build-aarch64-linux-gn:
    cross build --target aarch64-unknown-linux-gnu