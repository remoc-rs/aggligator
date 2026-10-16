use super::*;
use aggligator::transport::{Acceptor, Connector};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn tls_configs() -> (ClientConfig, ServerConfig) {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let cert = CertificateDer::from(certified.cert.der().to_vec());
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(certified.signing_key.serialize_der()));

    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert.clone()).unwrap();
    let client_tls = rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
    let server_tls =
        rustls::ServerConfig::builder().with_no_client_auth().with_single_cert(vec![cert], key).unwrap();

    (client_config(client_tls).unwrap(), server_config(server_tls).unwrap())
}

#[test]
fn extracts_host_name() {
    assert_eq!(host_name("server.example"), "server.example");
    assert_eq!(host_name("server.example:4242"), "server.example");
    assert_eq!(host_name("127.0.0.1"), "127.0.0.1");
    assert_eq!(host_name("127.0.0.1:4242"), "127.0.0.1");
    assert_eq!(host_name("::1"), "::1");
    assert_eq!(host_name("[::1]:4242"), "::1");
}

#[tokio::test]
async fn link_transfers_data_and_drains_on_drop() {
    let (client_cfg, server_cfg) = tls_configs();
    let acceptor = QuicAcceptor::new([SocketAddr::from((Ipv4Addr::LOCALHOST, 0))], server_cfg).await.unwrap();
    let server_addr = acceptor.local_addrs()[0];

    let (tx, mut rx) = mpsc::channel(1);
    let listener = tokio::spawn(async move { acceptor.listen(tx).await });

    let mut connector = QuicConnector::new([server_addr.to_string()], 0, client_cfg).await.unwrap();
    connector.set_server_name("localhost");
    let tag = QuicLinkTag::new(Local::Address(Ipv4Addr::LOCALHOST.into()), server_addr, Direction::Outgoing);
    let StreamBox::Io(mut client) = connector.connect(&tag).await.unwrap() else { panic!("expected IO stream") };

    let accepted = rx.recv().await.unwrap();
    let server_tag = accepted.tag.as_any().downcast_ref::<QuicLinkTag>().unwrap().clone();
    assert_eq!(server_tag.direction, Direction::Incoming);
    assert_eq!(server_tag.remote.ip(), IpAddr::from(Ipv4Addr::LOCALHOST));
    let StreamBox::Io(mut server) = accepted.stream else { panic!("expected IO stream") };

    // Server to client.
    server.write_all(b"hello").await.unwrap();
    server.flush().await.unwrap();
    let mut buf = [0; 5];
    client.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"hello");

    // Client to server, dropping the client right after writing.
    let reader = tokio::spawn(async move {
        let mut recved = Vec::new();
        server.read_to_end(&mut recved).await.unwrap();
        recved
    });
    let data: Vec<u8> = (0..4_000_000u32).map(|i| i as u8).collect();
    client.write_all(&data).await.unwrap();
    drop(client);

    let recved = timeout(Duration::from_secs(30), reader).await.expect("receive timed out").unwrap();
    assert_eq!(recved.len(), data.len());
    assert!(recved == data);

    listener.abort();
}

#[tokio::test]
async fn half_close_keeps_receiving() {
    let (client_cfg, server_cfg) = tls_configs();
    let acceptor = QuicAcceptor::new([SocketAddr::from((Ipv4Addr::LOCALHOST, 0))], server_cfg).await.unwrap();
    let server_addr = acceptor.local_addrs()[0];

    let (tx, mut rx) = mpsc::channel(1);
    let listener = tokio::spawn(async move { acceptor.listen(tx).await });

    let mut connector = QuicConnector::new([server_addr.to_string()], 0, client_cfg).await.unwrap();
    connector.set_server_name("localhost");
    let tag = QuicLinkTag::new(Local::Address(Ipv4Addr::LOCALHOST.into()), server_addr, Direction::Outgoing);
    let StreamBox::Io(client) = connector.connect(&tag).await.unwrap() else { panic!("expected IO stream") };
    let (mut client_read, mut client_write) = tokio::io::split(client);

    let accepted = rx.recv().await.unwrap();
    let StreamBox::Io(mut server) = accepted.stream else { panic!("expected IO stream") };

    // Client finishes sending and drops its write half.
    client_write.write_all(b"request").await.unwrap();
    client_write.shutdown().await.unwrap();
    drop(client_write);

    let mut request = Vec::new();
    server.read_to_end(&mut request).await.unwrap();
    assert_eq!(request, b"request");

    // Server keeps sending after the linger timeout of the client write half would have passed.
    sleep(Duration::from_millis(500)).await;
    server.write_all(b"response").await.unwrap();
    server.shutdown().await.unwrap();

    let mut response = Vec::new();
    timeout(Duration::from_secs(30), client_read.read_to_end(&mut response))
        .await
        .expect("receive timed out")
        .unwrap();
    assert_eq!(response, b"response");

    listener.abort();
}

#[tokio::test]
async fn aggregated_connection() {
    let (client_cfg, server_cfg) = tls_configs();
    let quic_acceptor =
        QuicAcceptor::new([SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))], server_cfg).await.unwrap();
    let port = quic_acceptor.local_addrs()[0].port();

    let acceptor = Acceptor::new();
    acceptor.add(quic_acceptor);

    let mut connector = Connector::new();
    let mut quic_connector = QuicConnector::new([format!("localhost:{port}")], 0, client_cfg).await.unwrap();
    quic_connector.set_ip_version(IpVersion::IPv4);
    connector.add(quic_connector);
    let outgoing = connector.channel().unwrap();

    let server = tokio::spawn(async move {
        let (ch, control) = acceptor.accept().await.unwrap();
        let links = control.links();
        assert!(!links.is_empty());
        assert!(links.iter().all(|link| link.tag().transport_name() == NAME));

        let mut stream = ch.into_stream();
        let mut buf = vec![0; 100_000];
        stream.read_exact(&mut buf).await.unwrap();
        assert!(buf.iter().enumerate().all(|(i, b)| *b == i as u8));
        stream.write_all(b"done").await.unwrap();
        stream.flush().await.unwrap();
        stream
    });

    let mut stream = outgoing.await.unwrap().into_stream();
    let data: Vec<u8> = (0..100_000u32).map(|i| i as u8).collect();
    stream.write_all(&data).await.unwrap();
    stream.flush().await.unwrap();
    let mut buf = [0; 4];
    stream.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"done");

    let _server_stream = server.await.unwrap();
}
