Temporary notes while porting from upstream move (dead) to sui.

Solana-specific crates are in

```
external-crates/move/solana
```

# Based off of anza-xyz/move commits

Initially based off of

7c49a43059e006199f9070e0a0fd67b11efb38d0

Upgraded through

6981a4702a93de00d03efdf260add96dfd3661df

And applied changes through

f1fc21cd9f94f41fbf5157af7896e85b53109dab

# Required env vars

```
export LLVM_SYS_170_PREFIX=$HOME/solana/platform-tools/1.41/move-dev
export PLATFORM_TOOLS_ROOT=$HOME/solana/platform-tools/1.41/
export MOVE_NATIVE=$HOME/solana/sui/external-crates/move/solana/language/move-native
```

# Test suites

working

```
cargo test --manifest-path external-crates/move/Cargo.toml -p move-to-solana
cargo test --manifest-path external-crates/move/Cargo.toml -p move-mv-llvm-compiler
cargo run --manifest-path external-crates/move/Cargo.toml --features solana-backend -p move-cli -- test -p external-crates/move/crates/move-stdlib/Move.toml --arch solana --solana
cargo test --manifest-path external-crates/move/Cargo.toml --features solana-backend -p move-unit-test -- --test-threads=1
cargo test --manifest-path external-crates/move/Cargo.toml --features solana-backend -p move-cli --test build_testsuite_solana -- --test-threads=1
cargo test --manifest-path external-crates/move/Cargo.toml --features solana-backend -p move-cli --test move_unit_tests_solana -- --test-threads=1
```
