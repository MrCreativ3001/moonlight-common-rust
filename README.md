# moonlight-common-rust [WIP]

`moonlight-common-rust` is a Rust implementation of the Moonlight game streaming protocol built around a Sans-IO architecture.

It provides a transport-agnostic protocol core with packet parsing and state management fully decoupled from networking and async runtimes. The crate also includes bindings to Moonlight Common C for interoperability with the existing implementation.

The Sans IO implementation is not finished yet.

## Why Sans-IO?

Separating protocol logic from I/O makes the library flexible and reusable across different environments.

Because the core does not depend on native sockets or a specific runtime, it can:

- Support multiple independent streams within a single process
- Work with any async ecosystem
- Integrate with custom networking backends
- Compile to WebAssembly and run in the browser, where networking is provided externally (e.g. WebRTC, WebTransport, Direct Sockets in IWA's)
- Easily be tested for without real network I/O

This design allows the same protocol implementation to be reused across native and web targets while remaining modular and easy to embed.

## Usage

The [`examples/`](./examples) directory contains examples demonstrating how to use the crate with the I/O implementations this library provides.

If you directly want to use the Sans IO protocol implementation, take a look at the [proto module](src/stream/proto/mod.rs) and the [std](src/stream/std/mod.rs) or [tokio](TODO) stream implementations as an example on how to use it.