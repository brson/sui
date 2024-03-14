Temporary notes.

Solana-specific crates are in

```
external-crates/move/solana
```

# Based off of anza-xyz/move commit

7c49a43059e006199f9070e0a0fd67b11efb38d0

# Required env vars

```
export LLVM_SYS_150_PREFIX=$HOME/solana/platform-tools/1.39/move-dev
export PLATFORM_TOOLS_ROOT=$HOME/solana/platform-tools/1.39/
export MOVE_NATIVE=$HOME/solana/sui/external-crates/move/solana/language/move-native
```

# Test suites

```
cargo test --manifest-path external-crates/move/Cargo.toml -p move-to-solana
cargo test --manifest-path external-crates/move/Cargo.toml -p move-mv-llvm-compiler
cargo run --manifest-path external-crates/move/Cargo.toml --features solana-backend -p move-cli -- test -p external-crates/move/crates/move-stdlib/Move.toml --arch solana
```
