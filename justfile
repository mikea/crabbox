import? 'justfile.local'

alias w := watch

watch *args="test":
    cargo watch -c -- just {{args}}

check:
    cargo check

test:
    cargo test

server *args:
    cargo run -- server {{args}}

clippy:
    cargo clippy -- -D warnings

fmt:
    cargo fmt

deps:
    sudo apt-get install libasound2-dev
    cargo install --locked cross

build-rpi:
    cross build --target aarch64-unknown-linux-gnu --features rpi --release
