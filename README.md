# moonlight-common-rust [WIP]

`moonlight-common-rust` is a Rust implementation of the Moonlight game streaming protocol built around a Sans-IO architecture.

It provides a transport-agnostic protocol core with packet parsing and state management fully decoupled from networking and async runtimes. The crate also includes bindings to Moonlight Common C for interoperability with the existing implementation.

## Why Sans-IO?

Separating protocol logic from I/O makes the library flexible and reusable across different environments.

Because the core does not depend on native sockets or a specific runtime, it can:

- Integrate with custom networking backends
- Work with any async ecosystem
- Support multiple independent streams within a single process
- Compile to WebAssembly and run in the browser, where networking is provided externally (e.g. WebRTC, WebTransport, Direct Sockets in IWA's)

This design allows the same protocol implementation to be reused across native and web targets while remaining modular and easy to embed.

## Usage

The [`examples/`](./examples) directory contains examples demonstrating how to use the crate with the I/O implementations this library provides:
- Creating and initializing a Moonlight client
- Pairing with a host
- Establishing a streaming session
  - Receiving Video and Audio
  - Sending Inputs to the host

