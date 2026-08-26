# Install

Wormhole is one binary. Install it straight from the repository, no clone:

```sh
cargo install --git https://github.com/naborka/wormhole --locked wormhole
```

That puts `wormhole` into `~/.cargo/bin`, which rustup already has on your
`PATH`. Run the same command again to update. `--locked` builds against the
dependency versions the repository was tested with.

Then check the host before anything else:

```sh
wormhole doctor
```

It runs nine probes — user namespaces, mount propagation, the pieces a box
needs — and says plainly what works, what does not, and what that rules
out. Exit code 0 means boxes will run.

## Requirements

- Rust 1.97 or newer, to build it
- Linux with unprivileged user namespaces enabled (most desktop distros)
- `curl`, `tar`, `cp` with reflink support on the filesystem for cheap
  image copies (Btrfs, XFS; works without, just slower)

## If you work on wormhole itself

```sh
git clone https://github.com/naborka/wormhole
cd wormhole
cargo install --path crates/wormhole
```

A symlink to the release build saves the reinstall after every change —
pick one of the two, not both:

```sh
cargo build --release
ln -s "$(pwd)/target/release/wormhole" ~/.local/bin/wormhole
```
