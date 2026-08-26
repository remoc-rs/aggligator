//! Tools for establishing connections consisting of aggregated TCP links,
//! optionally encrypted and authenticated using TLS.
//!
//! This module provides the simplest functions to establish outgoing or
//! accept incoming connections consisting of aggregated TCP links.
//!

use std::{io::Result, net::SocketAddr};

use aggligator::{
    alc::Stream,
    transport::{Acceptor, Connector},
};

use crate::{TcpAcceptor, TcpConnector};

/// Builds a connection consisting of aggregated TCP links to the target.
///
/// `target` specifies a set of IP addresses or hostnames of the target host.
/// If a hostname resolves to multiple IP addresses this is taken into account
/// automatically.
/// If an entry in target specifies no port number, `default_port` is used.
///
/// Links are established automatically from all available local network interfaces
/// to all IP addresses of the target. If a link fails, it is reconnected
/// automatically.
///
/// Returns the connection stream.
///
/// # Example
/// This example connects to the host `server` on port 5900.
///
/// Multiple links will be used if the local machine has multiple interfaces
/// that can all connect to `server`, or `server` has multiple interfaces
/// that are registered with their IP addresses in DNS.
/// ```no_run
/// use aggligator_transport_tcp::simple::tcp_connect;
///
/// #[tokio::main]
/// async fn main() -> std::io::Result<()> {
///     let stream = tcp_connect(["server"], 5900).await?;
///
///     // use the connection
///
///     Ok(())
/// }
/// ```
pub async fn tcp_connect(target: impl IntoIterator<Item = impl AsRef<str>>, default_port: u16) -> Result<Stream> {
    let mut connector = Connector::new();
    connector.add(TcpConnector::new(target, default_port).await?);
    let ch = connector.channel().unwrap().await?;
    Ok(ch.into_stream())
}

/// Listener for incoming connections of aggregated TCP links.
///
/// Create this using [`tcp_listen`] or, for TLS-encrypted connections,
/// [`tls_listen`].
#[derive(Debug)]
pub struct Listener(Acceptor);

impl Listener {
    /// Waits for an incoming connection of aggregated TCP links and accepts it.
    ///
    /// Returns the connection stream.
    ///
    /// This function is cancel-safe.
    pub async fn accept(&self) -> Result<Stream> {
        let (ch, _control) = self.0.accept().await?;
        Ok(ch.into_stream())
    }

    /// The underlying acceptor.
    ///
    /// Use this for advanced configuration and monitoring, for example to
    /// subscribe to link errors using [`Acceptor::link_errors`].
    pub fn acceptor(&self) -> &Acceptor {
        &self.0
    }
}

/// Listens on `addr` for incoming connections of aggregated TCP links.
///
/// # Example
/// This example listens on all interfaces on port 5900.
///
/// If the server has multiple interfaces, all IP addresses should be registered
/// in DNS so that clients can discover them and establish multiple links.
/// ```no_run
/// use std::net::{Ipv6Addr, SocketAddr};
/// use aggligator_transport_tcp::simple::tcp_listen;
///
/// #[tokio::main]
/// async fn main() -> std::io::Result<()> {
///     let listener = tcp_listen(
///         SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 5900)
///     ).await?;
///
///     loop {
///         let stream = listener.accept().await?;
///
///         tokio::spawn(async move {
///             // use the incoming connection
///         });
///     }
/// }
/// ```
pub async fn tcp_listen(addr: SocketAddr) -> Result<Listener> {
    let acceptor = Acceptor::new();
    acceptor.add(TcpAcceptor::new([addr]).await?);
    Ok(Listener(acceptor))
}

#[cfg(feature = "tls")]
mod tls {
    use std::{io::Result, net::SocketAddr, sync::Arc};

    use aggligator::{
        alc::Stream,
        transport::{Acceptor, Connector},
    };
    use aggligator_wrapper_tls::{TlsClient, TlsServer};

    #[doc(no_inline)]
    pub use aggligator_wrapper_tls::{ClientConfig, RootCertStore, ServerConfig, ServerName};

    use super::Listener;
    use crate::{TcpAcceptor, TcpConnector};

    /// Builds a connection consisting of aggregated TCP links to the target,
    /// which are encrypted and authenticated using TLS.
    ///
    /// `target` specifies a set of IP addresses or hostnames of the target host.
    /// If a hostname resolves to multiple IP addresses this is taken into account
    /// automatically.
    /// If an entry in target specifies no port number, `default_port` is used.
    ///
    /// Links are established automatically from all available local network interfaces
    /// to all IP addresses of the target. If a link fails, it is reconnected
    /// automatically.
    ///
    /// The identity of the server is verified using TLS against `server_name`.
    /// Each outgoing link is encrypted using TLS with the configuration specified
    /// in `tls_client_cfg`.
    ///
    /// Returns the connection stream.
    ///
    /// # Example
    /// This example connects to the host `agl.server.rs` on port 5901.
    ///
    /// Multiple links will be used if the local machine has multiple interfaces
    /// that can all connect to `agl.server.rs`, or `agl.server.rs` has multiple interfaces
    /// that are registered with their IP addresses in DNS.
    /// ```no_run
    /// use std::sync::Arc;
    /// use aggligator_transport_tcp::simple::tls_connect;
    /// use aggligator_transport_tcp::simple::{ClientConfig, RootCertStore, ServerName};
    ///
    /// #[tokio::main]
    /// async fn main() -> std::io::Result<()> {
    ///     let server_name = "agl.server.rs";
    ///
    ///     let mut root_store = RootCertStore::empty();
    ///     // add certificates to the root_store
    ///
    ///     let tls_cfg = Arc::new(
    ///         ClientConfig::builder()
    ///             .with_root_certificates(root_store)
    ///             .with_no_client_auth()
    ///     );
    ///
    ///     let stream = tls_connect(
    ///         [server_name],
    ///         5901,
    ///         tls_cfg,
    ///         ServerName::try_from(server_name).unwrap(),
    ///     ).await?;
    ///
    ///     // use the connection
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn tls_connect(
        target: impl IntoIterator<Item = impl AsRef<str>>, default_port: u16, tls_client_cfg: Arc<ClientConfig>,
        server_name: ServerName<'static>,
    ) -> Result<Stream> {
        let mut connector = Connector::wrapped(TlsClient::new(tls_client_cfg, server_name));
        connector.add(TcpConnector::new(target, default_port).await?);
        let ch = connector.channel().unwrap().await?;
        Ok(ch.into_stream())
    }

    /// Listens on `addr` for incoming connections of aggregated TCP links,
    /// which are encrypted and authenticated using TLS.
    ///
    /// Each incoming link is encrypted using TLS with the configuration specified
    /// in `tls_server_cfg`.
    ///
    /// # Example
    /// This example listens on all interfaces on port 5901.
    ///
    /// If the server has multiple interfaces, all IP addresses should be registered
    /// in DNS so that clients can discover them and establish multiple links.
    /// ```no_run
    /// use std::net::{Ipv6Addr, SocketAddr};
    /// use std::sync::Arc;
    /// use aggligator_transport_tcp::simple::{tls_listen, ServerConfig};
    ///
    /// #[tokio::main]
    /// async fn main() -> std::io::Result<()> {
    ///     let tls_certs = todo!("load certificate tree");
    ///     let tls_key = todo!("load private key");
    ///
    ///     let tls_cfg = Arc::new(
    ///         ServerConfig::builder()
    ///             .with_no_client_auth()
    ///             .with_single_cert(tls_certs, tls_key)
    ///             .unwrap()
    ///     );
    ///
    ///     let listener = tls_listen(
    ///         SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 5901),
    ///         tls_cfg,
    ///     ).await?;
    ///
    ///     loop {
    ///         let stream = listener.accept().await?;
    ///
    ///         tokio::spawn(async move {
    ///             // use the incoming connection
    ///         });
    ///     }
    /// }
    /// ```
    pub async fn tls_listen(addr: SocketAddr, tls_server_cfg: Arc<ServerConfig>) -> Result<Listener> {
        let acceptor = Acceptor::wrapped(TlsServer::new(tls_server_cfg));
        acceptor.add(TcpAcceptor::new([addr]).await?);
        Ok(Listener(acceptor))
    }
}

#[cfg(feature = "tls")]
pub use tls::*;
