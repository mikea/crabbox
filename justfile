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

build-rpi:
    cross build --target aarch64-unknown-linux-gnu --features rpi

deploy: build-rpi
    rsync -avz target/aarch64-unknown-linux-gnu/debug/crabbox jukebox.zt.aizatsky.com:/tmp/crabbox 
    ssh jukebox.zt.aizatsky.com 'killall crabbox ; /tmp/crabbox server /home/mike/crabbox/config.toml'