//! Aeron IPC/UDP transport for [nanofix](https://docs.rs/nanofix).
//!
//! [`build_transport`] builds every transport the core crate's
//! `nanofix::transport::build_transport` does, plus `TransportMode::Aeron`.
//! [`normalized_sbe`] converts FIX order-flow messages to and from the SBE
//! messages in `nanofix-sbe`.
//!
//! Building this crate compiles the Aeron C client and media driver with
//! CMake. Point `AERON_SOURCE_DIR` at an Aeron source checkout, or
//! `AERON_LIB_DIR` at a directory holding prebuilt `libaeron` and
//! `libaeron_driver`.

// Low-level pieces the bundled binaries build on. Not a stable API.
#[doc(hidden)]
pub mod aeron_c;
#[doc(hidden)]
pub mod aeron_sbe;

pub mod normalized_sbe;
pub mod transport_aeron;

use std::io;

use nanofix::transport::{Transport, TransportConfig, TransportMode};

pub use transport_aeron::AeronTransport;

/// Build the transport selected by `config`, including Aeron.
pub fn build_transport(config: TransportConfig) -> io::Result<Box<dyn Transport>> {
    match config.mode {
        TransportMode::Aeron => Ok(Box::new(AeronTransport::new(config))),
        _ => nanofix::transport::build_transport(config),
    }
}
