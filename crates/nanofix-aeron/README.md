# nanofix-aeron

[Aeron](https://github.com/aeron-io/aeron) IPC/UDP transport for the [`nanofix`](https://crates.io/crates/nanofix) FIX engine. Each FIX wire message travels as one Aeron message inside an SBE envelope from [`nanofix-sbe`](https://crates.io/crates/nanofix-sbe). The crate also converts FIX order flow to and from normalized SBE messages (`normalized_sbe`).

```toml
[dependencies]
nanofix = "0.1"
nanofix-aeron = "0.1"
```

```rust
use nanofix::transport::TransportConfig;
use nanofix_aeron::build_transport;

let mut transport = build_transport(TransportConfig::aeron_ipc(1001))?;
transport.connect("127.0.0.1", 0)?;
// Hand it to nanofix::engine::FixEngine::new_initiator(transport, session).
```

`build_transport` handles every `TransportMode`: Aeron here, the rest through `nanofix`.

## Building

This crate links the official Aeron C client and media driver (Aeron 1.51 or later). Provide one of:

- `AERON_SOURCE_DIR=/path/to/aeron`: an Aeron source checkout, built with CMake 3.30+ and a C/C++ compiler
- `AERON_LIB_DIR=/path/to/lib`: a directory with prebuilt `libaeron` and `libaeron_driver`

By default the transport expects a standalone media driver on the shared Aeron directory (`TransportConfig::default_aeron_dir()`). The repository's `aeron_media_driver` binary runs one. See [AERON.md](https://github.com/matthart1983/nanofix/blob/main/AERON.md) for the full guide.

MIT licensed.
