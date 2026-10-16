#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/remoc-rs/aggligator/master/.misc/aggligator.png",
    html_favicon_url = "https://raw.githubusercontent.com/remoc-rs/aggligator/master/.misc/aggligator.png",
    issue_tracker_base_url = "https://github.com/remoc-rs/aggligator/issues/"
)]

//! [Aggligator](aggligator) transport: QUIC
//!
//! Each link is a separate QUIC connection carrying a single bidirectional stream.
//! Like the TCP transport, one link is established from each local network interface
//! to each IP address of the server.
//!
//! QUIC connections are always encrypted using TLS 1.3. Use [`client_config`] and
//! [`server_config`] to build QUIC configurations from [rustls] configurations,
//! or supply your own [`quinn`] configurations.
//!
//! Connection migration is disabled on the server, since it would allow a link
//! to silently move to another network path.

#[cfg(not(any(feature = "rustls-aws-lc-rs", feature = "rustls-ring")))]
compile_error!("either the `rustls-aws-lc-rs` or the `rustls-ring` feature must be enabled");

use aggligator::{
    Link,
    control::{Direction, DisconnectReason},
    io::{IoBox, StreamBox},
    transport::{AcceptedStreamBox, AcceptingTransport, ConnectingTransport, LinkTag, LinkTagBox},
};
use async_trait::async_trait;
use futures::{FutureExt, StreamExt, future, stream::FuturesUnordered};
use quinn::{
    ClientConfig, Connection, Endpoint, EndpointConfig, Incoming, RecvStream, SendStream, ServerConfig,
    TransportConfig,
};
use socket2::{Domain, Protocol, Socket, Type};
use std::{
    any::Any,
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fmt,
    hash::{Hash, Hasher},
    io::{Error, ErrorKind, Result},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::{Notify, mpsc, watch},
    time::{Instant, sleep, timeout},
};

pub use aggligator_transport_tcp::{IpVersion, Local, TcpLinkFilter, util::NetworkInterface};
pub use quinn;

use aggligator_transport_tcp::util;

static NAME: &str = "quic";

/// ALPN protocol identifier set by [`client_config`] and [`server_config`].
pub const ALPN: &[u8] = b"aggligator";

/// Magic sent by the client on the stream of a new link.
///
/// QUIC only announces a stream to the peer once data has been sent on it.
const MAGIC: &[u8; 8] = b"AGGQUIC1";

/// Maximum UDP payload size accepted from the peer.
///
/// This fits an Ethernet MTU of 1500 bytes and exceeds the default upper bound of
/// QUIC path MTU discovery.
/// The receive buffer of each QUIC endpoint is proportional to this value.
const MAX_UDP_PAYLOAD_SIZE: u16 = 1472;

/// Timeout for establishing an incoming link.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Time to wait for the peer to acknowledge sent data before closing the QUIC connection.
const LINGER_TIMEOUT: Duration = Duration::from_secs(10);

/// Time to wait for a client endpoint to send the connection close to the peer.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(3);

/// Link tag for QUIC link.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QuicLinkTag {
    /// Local source.
    pub local: Local,
    /// Remote address.
    pub remote: SocketAddr,
    /// Link direction.
    pub direction: Direction,
}

impl fmt::Display for QuicLinkTag {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let dir = match self.direction {
            Direction::Incoming => "<-",
            Direction::Outgoing => "->",
        };
        write!(f, "{:16} {dir} {} (QUIC)", self.local, self.remote)
    }
}

impl QuicLinkTag {
    /// Creates a new link tag for a QUIC link.
    pub fn new(local: Local, remote: SocketAddr, direction: Direction) -> Self {
        Self { local, remote, direction }
    }
}

impl LinkTag for QuicLinkTag {
    fn transport_name(&self) -> &str {
        NAME
    }

    fn direction(&self) -> Direction {
        self.direction
    }

    fn user_data(&self) -> Vec<u8> {
        self.local.user_data()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn box_clone(&self) -> LinkTagBox {
        Box::new(self.clone())
    }

    fn dyn_cmp(&self, other: &dyn LinkTag) -> Ordering {
        let other = other.as_any().downcast_ref::<Self>().unwrap();
        Ord::cmp(self, other)
    }

    fn dyn_hash(&self, mut state: &mut dyn Hasher) {
        Hash::hash(self, &mut state)
    }
}

/// Returns a QUIC transport configuration suitable for aggregated links.
///
/// It sends keep-alives, so that idle links are not closed by QUIC or NAT
/// gateways, and allows only a single bidirectional stream per connection.
pub fn transport_config() -> TransportConfig {
    let mut cfg = TransportConfig::default();
    cfg.keep_alive_interval(Some(Duration::from_secs(5)));
    cfg.max_idle_timeout(Some(Duration::from_secs(60).try_into().unwrap()));
    cfg.max_concurrent_bidi_streams(1u8.into());
    cfg.max_concurrent_uni_streams(0u8.into());
    cfg
}

/// Builds a QUIC client configuration from a rustls client configuration.
///
/// The ALPN protocol is set to [`ALPN`] and the transport configuration
/// to [`transport_config`].
pub fn client_config(mut tls: rustls::ClientConfig) -> Result<ClientConfig> {
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto = quinn::crypto::rustls::QuicClientConfig::try_from(tls)
        .map_err(|err| Error::new(ErrorKind::InvalidInput, err))?;
    let mut cfg = ClientConfig::new(Arc::new(crypto));
    cfg.transport_config(Arc::new(transport_config()));
    Ok(cfg)
}

/// Builds a QUIC server configuration from a rustls server configuration.
///
/// The ALPN protocol is set to [`ALPN`] and the transport configuration
/// to [`transport_config`].
pub fn server_config(mut tls: rustls::ServerConfig) -> Result<ServerConfig> {
    tls.alpn_protocols = vec![ALPN.to_vec()];
    let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls)
        .map_err(|err| Error::new(ErrorKind::InvalidInput, err))?;
    let mut cfg = ServerConfig::with_crypto(Arc::new(crypto));
    cfg.transport_config(Arc::new(transport_config()));
    Ok(cfg)
}

/// QUIC transport for outgoing connections.
///
/// This transport is IO-stream based.
#[derive(Clone)]
pub struct QuicConnector {
    hosts: Vec<String>,
    server_name: String,
    client_cfg: ClientConfig,
    ip_version: IpVersion,
    resolve_interval: Duration,
    connect_timeout: Duration,
    link_filter: TcpLinkFilter,
    multi_interface: bool,
    interface_filter: Arc<dyn Fn(&NetworkInterface) -> bool + Send + Sync>,
    link_disconnected: Arc<Notify>,
    endpoints: Arc<Mutex<HashMap<EndpointKey, Endpoint>>>,
}

/// Local binding of a client endpoint.
///
/// All links using the same local binding share one endpoint and thus UDP socket.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EndpointKey {
    local: Local,
    ipv6: bool,
}

impl EndpointKey {
    fn new(tag: &QuicLinkTag) -> Self {
        Self { local: tag.local.clone(), ipv6: tag.remote.is_ipv6() }
    }
}

impl fmt::Debug for QuicConnector {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("QuicConnector")
            .field("hosts", &self.hosts)
            .field("server_name", &self.server_name)
            .field("ip_version", &self.ip_version)
            .field("resolve_interval", &self.resolve_interval)
            .field("connect_timeout", &self.connect_timeout)
            .field("link_filter", &self.link_filter)
            .field("multi_interface", &self.multi_interface)
            .finish()
    }
}

impl fmt::Display for QuicConnector {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.hosts.len() > 1 {
            write!(f, "QUIC [{}]", self.hosts.join(", "))
        } else {
            write!(f, "QUIC {}", self.hosts[0])
        }
    }
}

impl QuicConnector {
    /// Create a new QUIC transport for outgoing connections.
    ///
    /// `hosts` can contain IP addresses and hostnames, including port numbers.
    /// If an entry does not specify a port number, the `default_port` is used.
    ///
    /// The server name used for TLS defaults to the host name of the first entry
    /// in `hosts`. Use [`set_server_name`](Self::set_server_name) to change it.
    ///
    /// It is checked at creation that `hosts` resolves to at least one IP address.
    ///
    /// Host name resolution is retried periodically, thus DNS updates will be taken
    /// into account without the need to recreate this transport.
    pub async fn new(
        hosts: impl IntoIterator<Item = impl AsRef<str>>, default_port: u16, client_cfg: ClientConfig,
    ) -> Result<Self> {
        let this = Self::unresolved(hosts, default_port, client_cfg).await?;

        let addrs = this.resolve().await;
        if addrs.is_empty() {
            return Err(Error::new(ErrorKind::NotFound, "cannot resolve IP address of host"));
        }
        tracing::info!(%this, ?addrs, "hosts initially resolved");

        Ok(this)
    }

    /// Create a new QUIC transport for outgoing connections without checking that at least one host can be resolved.
    ///
    /// See [`new`](Self::new) for details.
    pub async fn unresolved(
        hosts: impl IntoIterator<Item = impl AsRef<str>>, default_port: u16, client_cfg: ClientConfig,
    ) -> Result<Self> {
        let mut hosts: Vec<String> = hosts.into_iter().map(|host| host.as_ref().to_string()).collect();

        if hosts.is_empty() {
            return Err(Error::new(ErrorKind::InvalidInput, "at least one host is required"));
        }

        let server_name = host_name(&hosts[0]).to_string();

        for host in &mut hosts {
            if !host.contains(':') {
                host.push_str(&format!(":{default_port}"));
            }
        }

        Ok(Self {
            hosts,
            server_name,
            client_cfg,
            ip_version: IpVersion::Both,
            resolve_interval: Duration::from_secs(10),
            connect_timeout: Duration::from_secs(10),
            link_filter: TcpLinkFilter::default(),
            multi_interface: !cfg!(target_os = "android"),
            interface_filter: Arc::new(|_| true),
            link_disconnected: Arc::new(Notify::new()),
            endpoints: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Sets the server name used for TLS server name indication and certificate verification.
    pub fn set_server_name(&mut self, server_name: impl Into<String>) {
        self.server_name = server_name.into();
    }

    /// Sets the IP version used for connecting.
    pub fn set_ip_version(&mut self, ip_version: IpVersion) {
        self.ip_version = ip_version;
    }

    /// Sets the interval for re-resolving the hostname and checking for changed network interfaces.
    pub fn set_resolve_interval(&mut self, resolve_interval: Duration) {
        self.resolve_interval = resolve_interval;
    }

    /// Sets the timeout for establishing a link.
    ///
    /// Since QUIC is unaware of unreachable network paths, the QUIC handshake
    /// would otherwise only fail after the idle timeout has been reached.
    ///
    /// The default is 10 seconds.
    pub fn set_connect_timeout(&mut self, connect_timeout: Duration) {
        self.connect_timeout = connect_timeout;
    }

    /// Sets the link filter method.
    ///
    /// It works the same as for the TCP transport.
    pub fn set_link_filter(&mut self, link_filter: TcpLinkFilter) {
        self.link_filter = link_filter;
    }

    /// Sets whether all available local interfaces should be used for connecting.
    ///
    /// If this is true (default for non-Android platforms), a separate link is
    /// established for each pair of server IP and local interface. Each outgoing socket
    /// is explicitly bound to a local interface.
    ///
    /// If this is false (default for Android platform), one link is established for
    /// each server IP. The operating system automatically assigns a local interface
    /// for the outgoing socket.
    pub fn set_multi_interface(&mut self, multi_interface: bool) {
        self.multi_interface = multi_interface;
    }

    /// Sets the local interface filter.
    ///
    /// It is only used when multi interface is enabled.
    ///
    /// The provided function is called for each discoved local interface and should
    /// return whether the interface should be used for establishing links.
    ///
    /// By default all local interfaces are used.
    pub fn set_interface_filter(
        &mut self, interface_filter: impl Fn(&NetworkInterface) -> bool + Send + Sync + 'static,
    ) {
        self.interface_filter = Arc::new(interface_filter);
    }

    /// Resolve target to socket addresses.
    async fn resolve(&self) -> Vec<SocketAddr> {
        util::resolve_hosts(&self.hosts, self.ip_version).await
    }

    /// Gets the shared client endpoint for the local binding of the link tag.
    fn endpoint(&self, tag: &QuicLinkTag) -> Result<Endpoint> {
        let mut endpoints = self.endpoints.lock().unwrap();
        let key = EndpointKey::new(tag);
        if let Some(endpoint) = endpoints.get(&key) {
            return Ok(endpoint.clone());
        }

        let runtime = quinn::default_runtime()
            .ok_or_else(|| Error::new(ErrorKind::Unsupported, "no async runtime found"))?;
        let endpoint = Endpoint::new(endpoint_config(), None, Self::socket(tag)?, runtime)?;
        tracing::debug!(local =% tag.local, addr =? endpoint.local_addr(), "created QUIC endpoint");

        endpoints.insert(key, endpoint.clone());
        Ok(endpoint)
    }

    /// Removes the shared client endpoint after a link failed to connect over it.
    ///
    /// The endpoint might have become unusable, for example if its network interface
    /// has been recreated. Existing connections continue to use it.
    fn evict_endpoint(&self, tag: &QuicLinkTag, endpoint: &Endpoint) {
        let mut endpoints = self.endpoints.lock().unwrap();
        let key = EndpointKey::new(tag);
        if endpoints.get(&key).is_some_and(|cached| cached.local_addr().ok() == endpoint.local_addr().ok()) {
            tracing::debug!(local =% tag.local, "removing QUIC endpoint");
            endpoints.remove(&key);
        }
    }

    /// Establishes a link over the specified endpoint.
    async fn connect_over(&self, tag: &QuicLinkTag, endpoint: &Endpoint) -> Result<IoBox> {
        let connecting = endpoint
            .connect_with(self.client_cfg.clone(), tag.remote, &self.server_name)
            .map_err(|err| Error::new(ErrorKind::InvalidInput, err))?;
        let conn = timeout(self.connect_timeout, connecting)
            .await
            .map_err(|_| Error::new(ErrorKind::TimedOut, "QUIC handshake timed out"))??;
        tracing::debug!(remote =% tag.remote, local =% tag.local, "QUIC connection established");

        let (mut send, recv) = conn.open_bi().await?;
        send.write_all(MAGIC).await?;

        Ok(link_stream(send, recv, conn, Some(endpoint.clone())))
    }

    /// Creates the UDP socket for an outgoing link.
    fn socket(tag: &QuicLinkTag) -> Result<UdpSocket> {
        let socket = Socket::new(Domain::for_address(tag.remote), Type::DGRAM, Some(Protocol::UDP))?;
        let unspecified: IpAddr = match tag.remote {
            SocketAddr::V4(_) => Ipv4Addr::UNSPECIFIED.into(),
            SocketAddr::V6(_) => Ipv6Addr::UNSPECIFIED.into(),
        };

        let bound = match &tag.local {
            Local::Interface(interface) => bind_socket_to_interface(&socket, interface, tag.remote.ip())?,
            Local::Address(addr) => {
                socket.bind(&SocketAddr::new(*addr, 0).into())?;
                true
            }
        };

        if !bound {
            socket.bind(&SocketAddr::new(unspecified, 0).into())?;
        }

        Ok(socket.into())
    }
}

/// QUIC endpoint configuration.
fn endpoint_config() -> EndpointConfig {
    let mut cfg = EndpointConfig::default();
    cfg.max_udp_payload_size(MAX_UDP_PAYLOAD_SIZE).unwrap();
    cfg
}

/// Extracts the host name from a `host:port` string.
fn host_name(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    match host.rsplit_once(':') {
        Some((name, port)) if !name.contains(':') && port.parse::<u16>().is_ok() => name,
        _ => host,
    }
}

/// Binds the socket to the specifed network interface.
///
/// Returns whether the socket has also been bound to a local address.
fn bind_socket_to_interface(socket: &Socket, interface: &[u8], remote: IpAddr) -> Result<bool> {
    #[cfg(any(target_os = "android", target_os = "fuchsia", target_os = "linux"))]
    {
        let _ = remote;
        socket.bind_device(Some(interface))?;
        Ok(false)
    }

    #[cfg(not(any(target_os = "android", target_os = "fuchsia", target_os = "linux")))]
    {
        for ifn in util::local_interfaces()? {
            if ifn.name.as_bytes() == interface {
                for addr in ifn.addr {
                    match (addr.ip(), remote) {
                        (IpAddr::V4(_), IpAddr::V4(_)) => (),
                        (IpAddr::V6(_), IpAddr::V6(_)) => (),
                        _ => continue,
                    }

                    if addr.ip().is_loopback() != remote.is_loopback() {
                        continue;
                    }

                    tracing::debug!("binding to {addr:?} on interface {}", &ifn.name);
                    socket.bind(&SocketAddr::new(addr.ip(), 0).into())?;
                    return Ok(true);
                }
            }
        }

        Err(Error::new(ErrorKind::NotFound, "no IP address for interface"))
    }
}

#[async_trait]
impl ConnectingTransport for QuicConnector {
    fn name(&self) -> &str {
        NAME
    }

    async fn link_tags(&self, tx: watch::Sender<HashSet<LinkTagBox>>) -> Result<()> {
        let mut addrs = Vec::new();
        let mut addrs_resolved: Option<Instant> = None;

        loop {
            let interfaces: Option<Vec<NetworkInterface>> = match self.multi_interface {
                true => Some(
                    util::local_interfaces()?
                        .into_iter()
                        .filter(|iface| (self.interface_filter)(iface))
                        .collect(),
                ),
                false => None,
            };

            if addrs_resolved.is_none_or(|t| t.elapsed() >= self.resolve_interval) {
                addrs = self.resolve().await;
                addrs_resolved = Some(Instant::now());
            }

            let mut tags: HashSet<LinkTagBox> = HashSet::new();
            for addr in &addrs {
                match &interfaces {
                    Some(interfaces) => {
                        for iface in util::interface_names_for_target(interfaces, addr.ip()) {
                            let tag = QuicLinkTag::new(Local::Interface(iface), *addr, Direction::Outgoing);
                            tags.insert(Box::new(tag));
                        }
                    }
                    None => {
                        if let Ok(src) = util::local_address_for_target(*addr) {
                            let tag = QuicLinkTag::new(Local::Address(src), *addr, Direction::Outgoing);
                            tags.insert(Box::new(tag));
                        }
                    }
                }
            }

            // Remove endpoints for vanished local bindings.
            self.endpoints.lock().unwrap().retain(|key, _| {
                tags.iter().any(|tag| {
                    let tag: &QuicLinkTag = tag.as_any().downcast_ref().unwrap();
                    EndpointKey::new(tag) == *key
                })
            });

            tx.send_if_modified(|v| {
                if *v != tags {
                    *v = tags;
                    true
                } else {
                    false
                }
            });

            tokio::select! {
                () = sleep(Duration::from_secs(1)) => (),
                () = self.link_disconnected.notified() => (),
            }
        }
    }

    async fn connect(&self, tag: &dyn LinkTag) -> Result<StreamBox> {
        let tag: &QuicLinkTag = tag.as_any().downcast_ref().unwrap();

        let endpoint = self.endpoint(tag)?;
        match self.connect_over(tag, &endpoint).await {
            Ok(io) => Ok(io.into()),
            Err(err) => {
                self.evict_endpoint(tag, &endpoint);
                Err(err)
            }
        }
    }

    async fn link_filter(&self, new: &Link<LinkTagBox>, existing: &[Link<LinkTagBox>]) -> bool {
        let Some(new_tag) = new.tag().as_any().downcast_ref::<QuicLinkTag>() else { return true };

        let intro = format!(
            "Judging {} QUIC link {} {} ({}) on {}",
            new.direction(),
            match new.direction() {
                Direction::Incoming => "from",
                Direction::Outgoing => "to",
            },
            new_tag.remote,
            String::from_utf8_lossy(new.remote_user_data()),
            new_tag.local,
        );

        match existing.iter().find(|link| {
            let Some(tag) = link.tag().as_any().downcast_ref::<QuicLinkTag>() else { return false };
            match self.link_filter {
                TcpLinkFilter::None => false,
                TcpLinkFilter::InterfaceInterface => {
                    tag.local == new_tag.local && link.remote_user_data() == new.remote_user_data()
                }
                TcpLinkFilter::InterfaceIp => {
                    tag.local == new_tag.local && tag.remote.ip() == new_tag.remote.ip()
                }
            }
        }) {
            Some(other) => {
                let other_tag = other.tag().as_any().downcast_ref::<QuicLinkTag>().unwrap();
                tracing::debug!("{intro} => link {} is redundant, rejecting.", other_tag.remote);
                false
            }
            None => {
                tracing::debug!("{intro} => accepted.");
                true
            }
        }
    }

    async fn link_disconnected(
        &self, _tag: &dyn LinkTag, _result: std::result::Result<DisconnectReason, &Error>,
    ) {
        self.link_disconnected.notify_waiters();
    }
}

/// QUIC transport for incoming connections.
///
/// This transport is IO-stream based.
pub struct QuicAcceptor {
    endpoints: Vec<Endpoint>,
}

impl fmt::Debug for QuicAcceptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("QuicAcceptor").field("local_addrs", &self.local_addrs()).finish()
    }
}

impl fmt::Display for QuicAcceptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let addrs: Vec<_> = self.local_addrs().iter().map(|addr| addr.to_string()).collect();
        if addrs.len() == 1 { write!(f, "{}", addrs[0]) } else { write!(f, "[{}]", addrs.join(", ")) }
    }
}

impl QuicAcceptor {
    /// Create a new QUIC transport listening for incoming connections.
    ///
    /// It listens on the local addresses specified in `addrs`.
    /// Connection migration is disabled in the provided server configuration.
    pub async fn new(addrs: impl IntoIterator<Item = SocketAddr>, mut server_cfg: ServerConfig) -> Result<Self> {
        server_cfg.migration(false);

        let runtime = quinn::default_runtime()
            .ok_or_else(|| Error::new(ErrorKind::Unsupported, "no async runtime found"))?;

        let mut endpoints = Vec::new();
        for addr in addrs {
            let socket = Socket::new(Domain::for_address(addr), Type::DGRAM, Some(Protocol::UDP))?;
            if addr.is_ipv6() {
                let _ = socket.set_only_v6(false);
            }
            socket.bind(&addr.into())?;

            let endpoint =
                Endpoint::new(endpoint_config(), Some(server_cfg.clone()), socket.into(), runtime.clone())?;
            endpoints.push(endpoint);
        }

        Self::from_endpoints(endpoints)
    }

    /// Create a new QUIC transport for incoming connections using the specified QUIC endpoints.
    ///
    /// The endpoints must have been created with a server configuration.
    /// It is recommended to disable connection migration in it.
    pub fn from_endpoints(endpoints: impl IntoIterator<Item = Endpoint>) -> Result<Self> {
        let endpoints: Vec<_> = endpoints.into_iter().collect();

        if endpoints.is_empty() {
            return Err(Error::new(ErrorKind::InvalidInput, "at least one endpoint is required"));
        }

        Ok(Self { endpoints })
    }

    /// The local addresses the transport is listening on.
    pub fn local_addrs(&self) -> Vec<SocketAddr> {
        self.endpoints.iter().filter_map(|endpoint| endpoint.local_addr().ok()).collect()
    }

    /// Performs the handshake of an incoming connection.
    async fn handshake(incoming: Incoming, bound: SocketAddr) -> Result<AcceptedStreamBox> {
        let local_ip = incoming.local_ip();

        let (conn, send, recv) = timeout(HANDSHAKE_TIMEOUT, async {
            let conn = incoming.accept()?.await?;
            let (send, mut recv) = conn.accept_bi().await?;

            let mut magic = [0; MAGIC.len()];
            recv.read_exact(&mut magic).await.map_err(Error::other)?;
            if magic != *MAGIC {
                conn.close(1u8.into(), b"invalid magic");
                return Err(Error::new(ErrorKind::InvalidData, "invalid magic"));
            }

            Ok::<_, Error>((conn, send, recv))
        })
        .await
        .map_err(|_| Error::new(ErrorKind::TimedOut, "QUIC handshake timed out"))??;

        let mut remote = conn.remote_address();
        util::use_proper_ipv4(&mut remote);

        let local = match local_ip.or_else(|| conn.local_ip()) {
            Some(ip) => {
                let ip = ip.to_canonical();
                match util::local_interface_for_ip(ip)? {
                    Some(interface) => Local::Interface(interface),
                    None => Local::Address(ip),
                }
            }
            None => Local::Address(bound.ip().to_canonical()),
        };

        tracing::debug!(%remote, %local, "Accepted QUIC connection");
        let tag = QuicLinkTag::new(local, remote, Direction::Incoming);

        Ok(AcceptedStreamBox::new(link_stream(send, recv, conn, None).into(), tag))
    }
}

#[async_trait]
impl AcceptingTransport for QuicAcceptor {
    fn name(&self) -> &str {
        NAME
    }

    async fn listen(&self, tx: mpsc::Sender<AcceptedStreamBox>) -> Result<()> {
        let mut handshakes = FuturesUnordered::new();

        loop {
            tokio::select! {
                (incoming, idx, _) = future::select_all(self.endpoints.iter().map(|ep| ep.accept().boxed())) => {
                    let Some(incoming) = incoming else {
                        return Err(Error::new(ErrorKind::BrokenPipe, "QUIC endpoint was closed"));
                    };
                    let bound = self.endpoints[idx].local_addr()?;
                    handshakes.push(Self::handshake(incoming, bound));
                }
                Some(res) = handshakes.next() => {
                    match res {
                        Ok(accepted) => {
                            let _ = tx.send(accepted).await;
                        }
                        Err(err) => tracing::debug!(%err, "incoming QUIC connection failed"),
                    }
                }
            }
        }
    }
}

/// Wraps the streams of a QUIC link.
///
/// The connection is closed once both returned stream halves have been dropped.
fn link_stream(send: SendStream, recv: RecvStream, conn: Connection, endpoint: Option<Endpoint>) -> IoBox {
    let guard = Arc::new(ConnectionGuard { conn, endpoint });
    IoBox::new(
        GuardedRecvStream { recv, _guard: guard.clone() },
        LingeringSendStream { send: Some(send), guard: Some(guard) },
    )
}

/// Closes a QUIC connection when dropped.
///
/// A client endpoint may stop before it has notified the peer of the closure,
/// thus it is kept alive until the connection has been closed.
struct ConnectionGuard {
    conn: Connection,
    /// Client endpoint.
    endpoint: Option<Endpoint>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.conn.close(0u8.into(), b"");

        let Some(endpoint) = self.endpoint.take() else { return };
        let Ok(rt) = tokio::runtime::Handle::try_current() else { return };
        rt.spawn(async move {
            let _ = timeout(CLOSE_TIMEOUT, endpoint.wait_idle()).await;
        });
    }
}

/// QUIC receive stream keeping the connection open.
struct GuardedRecvStream {
    recv: RecvStream,
    _guard: Arc<ConnectionGuard>,
}

impl AsyncRead for GuardedRecvStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<Result<()>> {
        AsyncRead::poll_read(Pin::new(&mut self.get_mut().recv), cx, buf)
    }
}

/// QUIC send stream that keeps the connection open until sent data has been acknowledged.
///
/// A QUIC connection is closed immediately once it is closed or all handles to it are dropped,
/// which would discard data that has been sent but not yet acknowledged by the peer.
struct LingeringSendStream {
    send: Option<SendStream>,
    guard: Option<Arc<ConnectionGuard>>,
}

impl LingeringSendStream {
    fn send(&mut self) -> Pin<&mut SendStream> {
        Pin::new(self.send.as_mut().unwrap())
    }
}

impl AsyncWrite for LingeringSendStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<Result<usize>> {
        AsyncWrite::poll_write(self.get_mut().send(), cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        AsyncWrite::poll_flush(self.get_mut().send(), cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>> {
        AsyncWrite::poll_shutdown(self.get_mut().send(), cx)
    }
}

impl Drop for LingeringSendStream {
    fn drop(&mut self) {
        let (Some(mut send), Some(guard)) = (self.send.take(), self.guard.take()) else { return };
        let Ok(rt) = tokio::runtime::Handle::try_current() else { return };
        rt.spawn(async move {
            let _ = send.finish();
            let _ = timeout(LINGER_TIMEOUT, send.stopped()).await;
            drop(guard);
        });
    }
}

#[cfg(test)]
mod tests;
