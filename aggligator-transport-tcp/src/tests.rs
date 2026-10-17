use super::*;
use aggligator::transport::{Acceptor, Connector};
use std::net::Ipv4Addr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn single_and_multiple_links_per_path() {
    for links_per_path in [1, 4] {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let tcp_acceptor = TcpAcceptor::from_listeners([listener]).unwrap();

        let acceptor = Acceptor::new();
        acceptor.add(tcp_acceptor);

        let mut connector = Connector::new();
        let mut tcp_connector = TcpConnector::new([addr.to_string()], addr.port()).await.unwrap();
        tcp_connector.set_multi_interface(false);
        tcp_connector.set_links_per_path(links_per_path);
        connector.add(tcp_connector);
        let control = connector.control();
        let outgoing = connector.channel().unwrap();

        let server = tokio::spawn(async move {
            let (ch, server_control) = acceptor.accept().await.unwrap();
            let mut stream = ch.into_stream();
            let mut buf = vec![0; 64_000];
            stream.read_exact(&mut buf).await.unwrap();
            stream.write_all(b"done").await.unwrap();
            stream.flush().await.unwrap();
            (stream, server_control)
        });

        let mut stream = outgoing.await.unwrap().into_stream();
        while control.links().len() < links_per_path {
            sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(control.links().len(), links_per_path);

        let data = vec![42u8; 64_000];
        stream.write_all(&data).await.unwrap();
        stream.flush().await.unwrap();
        let mut reply = [0; 4];
        stream.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"done");

        let (_server_stream, server_control) = server.await.unwrap();
        assert_eq!(server_control.links().len(), links_per_path);
    }
}
