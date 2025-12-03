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
