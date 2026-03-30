Integration tests for this Rust project live in the standard `tests/` directory so `cargo test` will discover and run them automatically.

The actual API coverage source requested for Phase 5 lives in `test/api_endpoint_coverage.rs`.

`tests/api_endpoint_coverage.rs` is a thin wrapper that includes that file so Cargo will run it.
