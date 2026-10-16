use super::*;
use aggligator::{
    io::TxRxBox,
    transport::{Acceptor, Connector},
};
use aggligator_transport_tcp::{TcpAcceptor, TcpConnector};
use bytes::Bytes;
use futures::SinkExt;
use std::{
    net::{Ipv4Addr, SocketAddr},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt, ReadBuf, duplex, split},
    time::timeout,
};

/// Counts the bytes read from the inner reader.
struct CountingRead<R> {
    inner: R,
    count: Arc<AtomicUsize>,
}

impl<R: AsyncRead + Unpin> AsyncRead for CountingRead<R> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<Result<()>> {
        let before = buf.filled().len();
        let res = Pin::new(&mut self.inner).poll_read(cx, buf);
        self.count.fetch_add(buf.filled().len() - before, Ordering::Relaxed);
        res
    }
}

/// Returns a pair of connected, zstd-wrapped streams and a counter of bytes received by the second.
fn wrapped_pair(sender: Zstd, receiver: Zstd) -> (IoBox, IoBox, Arc<AtomicUsize>) {
    let (a, b) = duplex(65_536);
    let count = Arc::new(AtomicUsize::new(0));

    let (a_read, a_write) = split(a);
    let a = sender.wrap(IoBox::new(a_read, a_write).into()).unwrap();
    let (b_read, b_write) = split(b);
    let b_read = CountingRead { inner: b_read, count: count.clone() };
    let b = receiver.wrap(IoBox::new(b_read, b_write).into()).unwrap();

    let (StreamBox::Io(a), StreamBox::Io(b)) = (a, b) else { panic!("expected IO streams") };
    (a, b, count)
}

fn compressible_data(len: usize) -> Vec<u8> {
    let text = b"Aggligator aggregates multiple links into one connection. ";
    text.iter()
        .copied()
        .cycle()
        .take(len)
        .enumerate()
        .map(|(i, b)| if i % 1000 == 0 { (i / 1000) as u8 } else { b })
        .collect()
}

#[tokio::test]
async fn compresses_and_flushes() {
    let (mut a, mut b, received) = wrapped_pair(Zstd::new(), Zstd::new());

    let data = compressible_data(1_000_000);
    let reader = tokio::spawn(async move {
        let mut buf = vec![0; 1_000_000];
        b.read_exact(&mut buf).await.unwrap();
        (b, buf)
    });

    a.write_all(&data).await.unwrap();
    a.flush().await.unwrap();

    let (mut b, buf) =
        timeout(Duration::from_secs(10), reader).await.expect("data not delivered on flush").unwrap();
    assert!(buf == data);

    let received = received.load(Ordering::Relaxed);
    assert!(received < data.len() / 10, "data was not compressed: {received} bytes for {} bytes", data.len());

    // Small message after flush must be delivered without closing the stream.
    a.write_all(b"ping").await.unwrap();
    a.flush().await.unwrap();
    let mut ping = [0; 4];
    timeout(Duration::from_secs(10), b.read_exact(&mut ping))
        .await
        .expect("small message not delivered")
        .unwrap();
    assert_eq!(&ping, b"ping");

    // Shutdown is seen as end of stream.
    a.shutdown().await.unwrap();
    let mut rest = Vec::new();
    timeout(Duration::from_secs(10), b.read_to_end(&mut rest)).await.unwrap().unwrap();
    assert!(rest.is_empty());
}

#[tokio::test]
async fn different_levels_and_window() {
    let mut sender = Zstd::with_level(-5);
    sender.set_window_log(Some(20));
    let mut receiver = Zstd::with_level(19);
    receiver.set_window_log(Some(20));
    let (mut a, mut b, _) = wrapped_pair(sender, receiver);

    let data = compressible_data(300_000);
    let reader = tokio::spawn(async move {
        let mut buf = vec![0; 300_000];
        b.read_exact(&mut buf).await.unwrap();
        buf
    });
    a.write_all(&data).await.unwrap();
    a.flush().await.unwrap();
    assert!(timeout(Duration::from_secs(10), reader).await.unwrap().unwrap() == data);
}

#[test]
fn rejects_packet_based_streams() {
    let tx = futures::sink::drain().sink_map_err(|_| Error::other("unreachable"));
    let rx = futures::stream::empty::<Result<Bytes>>();
    let res = Zstd::new().wrap(TxRxBox::new(tx, rx).into());
    assert_eq!(res.err().map(|err| err.kind()), Some(ErrorKind::Unsupported));
}

#[tokio::test]
async fn aggregated_connection() {
    let mut acceptor_builder = aggligator::transport::AcceptorBuilder::new(Default::default());
    acceptor_builder.wrap(Zstd::new());
    let acceptor: Acceptor = acceptor_builder.build();
    let tcp_acceptor = TcpAcceptor::new([SocketAddr::from((Ipv4Addr::LOCALHOST, 0))]).await.unwrap();
    let port = tcp_acceptor.to_string().rsplit(':').next().unwrap().parse::<u16>().unwrap();
    acceptor.add(tcp_acceptor);

    let mut connector_builder = aggligator::transport::ConnectorBuilder::new(Default::default());
    connector_builder.wrap(Zstd::new());
    let mut connector: Connector = connector_builder.build();
    let mut tcp_connector = TcpConnector::new([format!("127.0.0.1:{port}")], port).await.unwrap();
    tcp_connector.set_multi_interface(false);
    connector.add(tcp_connector);
    let outgoing = connector.channel().unwrap();

    let data = compressible_data(500_000);
    let expected = data.clone();
    let server = tokio::spawn(async move {
        let (ch, _control) = acceptor.accept().await.unwrap();
        let mut stream = ch.into_stream();
        let mut buf = vec![0; expected.len()];
        stream.read_exact(&mut buf).await.unwrap();
        assert!(buf == expected);
        stream.write_all(b"done").await.unwrap();
        stream.flush().await.unwrap();
        stream
    });

    let mut stream = outgoing.await.unwrap().into_stream();
    stream.write_all(&data).await.unwrap();
    stream.flush().await.unwrap();
    let mut done = [0; 4];
    timeout(Duration::from_secs(30), stream.read_exact(&mut done)).await.unwrap().unwrap();
    assert_eq!(&done, b"done");

    let _server_stream = server.await.unwrap();
}
