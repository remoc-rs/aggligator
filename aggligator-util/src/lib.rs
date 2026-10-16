#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/remoc-rs/aggligator/master/.misc/aggligator.png",
    html_favicon_url = "https://raw.githubusercontent.com/remoc-rs/aggligator/master/.misc/aggligator.png",
    issue_tracker_base_url = "https://github.com/remoc-rs/aggligator/issues/"
)]

//! Aggligator command line utilities.

use anyhow::{Context, bail};
use std::{path::PathBuf, sync::Arc};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use aggligator::cfg::Cfg;
use aggligator_transport_quic::quinn::{self, congestion};
use aggligator_transport_tcp::TcpLinkFilter;

pub mod tls;

/// Initializes logging for command line utilities.
pub fn init_log() {
    tracing_subscriber::registry().with(fmt::layer()).with(EnvFilter::from_default_env()).init();
    tracing_log::LogTracer::init().unwrap();
}

/// Prints the default Aggligator configuration.
pub fn print_default_cfg() {
    println!("{}", serde_json::to_string_pretty(&Cfg::default()).unwrap());
}

/// Loads an Aggligator configuration from disk or returns the default
/// configuration if the path is empty.
pub fn load_cfg(path: &Option<PathBuf>) -> anyhow::Result<Cfg> {
    match path {
        Some(path) => {
            let file = std::fs::File::open(path).context("cannot open configuration file")?;
            serde_json::from_reader(file).context("cannot parse configuration file")
        }
        None => Ok(Cfg::default()),
    }
}

/// Parse [TcpLinkFilter] option.
pub fn parse_tcp_link_filter(s: &str) -> anyhow::Result<TcpLinkFilter> {
    match s {
        "none" => Ok(TcpLinkFilter::None),
        "interface-interface" => Ok(TcpLinkFilter::InterfaceInterface),
        "interface-ip" => Ok(TcpLinkFilter::InterfaceIp),
        other => bail!("unknown TCP link filter: {other}"),
    }
}

/// Checks that the TCP congestion control algorithm can be used.
///
/// This is done by setting it on a newly created TCP socket.
pub fn check_tcp_congestion(algorithm: &str) -> anyhow::Result<()> {
    let socket =
        socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::STREAM, Some(socket2::Protocol::TCP))?;
    aggligator_transport_tcp::util::set_congestion_control(&socket, algorithm)
        .with_context(|| format!("cannot use TCP congestion control algorithm {algorithm}"))
}

/// QUIC congestion control algorithm.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum QuicCongestion {
    /// CUBIC.
    #[default]
    Cubic,
    /// BBR.
    Bbr,
    /// NewReno.
    NewReno,
}

impl QuicCongestion {
    /// QUIC transport configuration using this congestion control algorithm.
    pub fn transport_config(self) -> quinn::TransportConfig {
        let mut cfg = aggligator_transport_quic::transport_config();
        match self {
            Self::Cubic => cfg.congestion_controller_factory(Arc::new(congestion::CubicConfig::default())),
            Self::Bbr => cfg.congestion_controller_factory(Arc::new(congestion::BbrConfig::default())),
            Self::NewReno => cfg.congestion_controller_factory(Arc::new(congestion::NewRenoConfig::default())),
        };
        cfg
    }

    /// QUIC client configuration using the specified TLS configuration and this congestion control algorithm.
    pub fn client_config(self, tls: rustls::ClientConfig) -> anyhow::Result<quinn::ClientConfig> {
        let mut cfg = aggligator_transport_quic::client_config(tls)?;
        cfg.transport_config(Arc::new(self.transport_config()));
        Ok(cfg)
    }

    /// QUIC server configuration using the specified TLS configuration and this congestion control algorithm.
    pub fn server_config(self, tls: rustls::ServerConfig) -> anyhow::Result<quinn::ServerConfig> {
        let mut cfg = aggligator_transport_quic::server_config(tls)?;
        cfg.transport_config(Arc::new(self.transport_config()));
        Ok(cfg)
    }
}

/// Waits for a platform-specific termination signal.
pub async fn wait_sigterm() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigterm = signal(SignalKind::terminate()).unwrap();
        let mut sigint = signal(SignalKind::interrupt()).unwrap();
        let mut sighup = signal(SignalKind::hangup()).unwrap();

        tokio::select! {
            _ = sigterm.recv() => {},
            _ = sigint.recv() => {},
            _ = sighup.recv() => {},
        }
    }

    #[cfg(windows)]
    {
        use tokio::signal::windows;

        let mut ctrl_c = windows::ctrl_c().unwrap();
        let mut ctrl_break = windows::ctrl_break().unwrap();
        let mut ctrl_close = windows::ctrl_close().unwrap();
        let mut ctrl_logoff = windows::ctrl_logoff().unwrap();
        let mut ctrl_shutdown = windows::ctrl_shutdown().unwrap();

        tokio::select! {
            _ = ctrl_c.recv() => {},
            _ = ctrl_break.recv() => {},
            _ = ctrl_close.recv() => {},
            _ = ctrl_logoff.recv() => {},
            _ = ctrl_shutdown.recv() => {},
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        std::future::pending::<()>().await;
    }

    tracing::info!("received termination signal");
}
