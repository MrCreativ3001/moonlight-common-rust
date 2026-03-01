
# Examples

Examples demonstrating how to use this crate.

## client-simple

Pair to a host and get all app images.
Those images will be stored inside of the [`example-data/apps`](../example-data/apps) folder.

```
cargo run --example client-simple
```

## client-stream

Connects to a host using the rust moonlight protocol implementation.

```
cargo run --example client-stream
```

## client-tokio

Pair to a host and start a stream in an async context using the tokio library.

```
cargo run --example client-tokio --features tokio
```

## client-common-c

Connects to a host using the moonlight common c protocol implementation.

This is currently only possible using [rust nightly](https://rust-lang.github.io/rustup/concepts/channels.html) and has these requirements:
Required for building:
- A [CMake installation](https://cmake.org/download/) which will automatically compile the [moonlight-common-c](https://github.com/moonlight-stream/moonlight-common-c) library
- [openssl-sys](https://docs.rs/openssl-sys/0.9.109/openssl_sys/): For information on building openssl sys go to the [openssl docs](https://docs.rs/openssl/latest/openssl/)
- A [bindgen installation](https://rust-lang.github.io/rust-bindgen/requirements.html) for generating the bindings to the [moonlight-common-c](https://github.com/moonlight-stream/moonlight-common-c) library

```
cargo run --example client-simple --features stream-c
```
