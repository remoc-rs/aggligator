#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/remoc-rs/aggligator/master/.misc/aggligator.png",
    html_favicon_url = "https://raw.githubusercontent.com/remoc-rs/aggligator/master/.misc/aggligator.png",
    issue_tracker_base_url = "https://github.com/remoc-rs/aggligator/issues/"
)]

//! [Aggligator](aggligator) transport wrapper using zstd compression
//!
//! This compresses the data sent over each Aggligator link using [zstd](https://facebook.github.io/zstd/).
//!
//! The wrapper must be used on both ends of a connection, since compression is not negotiated.
//! The compression level only affects the sending side and thus may differ between both ends.
//!
//! Compression only pays off when the transferred data is compressible and the links,
//! not the CPU, are the bottleneck. Data that is already compressed or encrypted does not
//! become smaller. Alternatively, the application can compress the data of the aggregated
//! connection itself, which compresses the data only once instead of separately on each link.
//!
//! # Combining with TLS
//! Wrappers are applied in the order they are added, thus the first wrapper is applied directly
//! to the transport. To compress data before it is encrypted, add the TLS wrapper first and then
//! this wrapper.
//!
//! Compressing data before encrypting it may reveal information about the data through
//! the length of the encrypted data. This allows attacks if data controlled by an attacker is
//! transferred together with secret data over the same connection.
//!
//! # Example
//! ```no_run
//! use aggligator::transport::ConnectorBuilder;
//! use aggligator_wrapper_zstd::Zstd;
//!
//! let mut builder = ConnectorBuilder::new(Default::default());
//! builder.wrap(Zstd::new());
//! let connector = builder.build();
//! ```

use async_compression::{
    Level,
    tokio::{bufread::ZstdDecoder, write::ZstdEncoder},
    zstd::{CParameter, DParameter},
};
use async_trait::async_trait;
use std::io::{Error, ErrorKind, Result};
use tokio::io::BufReader;

use aggligator::{
    io::{IoBox, StreamBox},
    transport::{AcceptingWrapper, ConnectingWrapper},
};

static NAME: &str = "zstd";

/// zstd compression wrapper for outgoing and incoming links.
///
/// Only IO-based streams are supported; wrapping a packet-based stream fails.
///
/// Add this to a [`ConnectorBuilder`](aggligator::transport::ConnectorBuilder) and the
/// [`AcceptorBuilder`](aggligator::transport::AcceptorBuilder) of the remote endpoint
/// to compress all links of the connection.
#[derive(Debug, Clone)]
#[must_use = "you must pass this wrapper to the connector or acceptor"]
pub struct Zstd {
    level: i32,
    window_log: Option<u32>,
}

impl Default for Zstd {
    fn default() -> Self {
        Self::new()
    }
}

impl Zstd {
    /// Default compression level.
    ///
    /// This is the fastest regular level of zstd.
    pub const DEFAULT_LEVEL: i32 = 1;

    /// Creates a new zstd compression wrapper using the [default level](Self::DEFAULT_LEVEL).
    pub fn new() -> Self {
        Self::with_level(Self::DEFAULT_LEVEL)
    }

    /// Creates a new zstd compression wrapper using the specified compression level.
    ///
    /// Higher levels compress better but are slower. Negative levels are faster than
    /// level 1 but compress worse. The level is clamped to the range supported by zstd.
    pub fn with_level(level: i32) -> Self {
        Self { level, window_log: None }
    }

    /// The compression level.
    pub fn level(&self) -> i32 {
        self.level
    }

    /// Sets the base 2 logarithm of the maximum window size.
    ///
    /// This limits the memory used for compressing and decompressing each link.
    /// Received data that was compressed using a larger window cannot be decompressed,
    /// thus both ends must use the same setting.
    ///
    /// If `None` (default), the window size is determined by the compression level
    /// for compressing and the default limit of zstd (128 MiB) applies to decompressing.
    pub fn set_window_log(&mut self, window_log: Option<u32>) {
        self.window_log = window_log;
    }

    /// Wraps the stream of a link.
    fn wrap(&self, stream: StreamBox) -> Result<StreamBox> {
        let StreamBox::Io(io) = stream else {
            return Err(Error::new(ErrorKind::Unsupported, "zstd wrapper only supports IO-based streams"));
        };
        let (read, write) = io.into_split();

        let cparams: Vec<_> = self.window_log.map(CParameter::window_log).into_iter().collect();
        let encoder = ZstdEncoder::with_quality_and_params(write, Level::Precise(self.level), &cparams);

        let dparams: Vec<_> = self.window_log.map(DParameter::window_log_max).into_iter().collect();
        let mut decoder = ZstdDecoder::with_params(BufReader::new(read), &dparams);
        decoder.multiple_members(true);

        Ok(IoBox::new(decoder, encoder).into())
    }
}

#[async_trait]
impl ConnectingWrapper for Zstd {
    fn name(&self) -> &str {
        NAME
    }

    async fn wrap(&self, stream: StreamBox) -> Result<StreamBox> {
        Zstd::wrap(self, stream)
    }
}

#[async_trait]
impl AcceptingWrapper for Zstd {
    fn name(&self) -> &str {
        NAME
    }

    async fn wrap(&self, stream: StreamBox) -> Result<StreamBox> {
        Zstd::wrap(self, stream)
    }
}

#[cfg(test)]
mod tests;
