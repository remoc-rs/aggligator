use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncWrite, duplex};

use super::{IntegrityCodec, IntegrityError, IoRx, IoTx};

/// Writer that does not support vectored writes.
struct NonVectored<W>(W);

impl<W: AsyncWrite + Unpin> AsyncWrite for NonVectored<W> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().0).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().0).poll_shutdown(cx)
    }
}

fn packets() -> Vec<Bytes> {
    let sizes = [0, 1, 10, 100, 4_095, 4_096, 4_097, 8_181, 8_182, 8_192, 65_536, 100_000, 1_048_576];
    sizes
        .iter()
        .chain(sizes.iter().rev())
        .enumerate()
        .map(|(i, &size)| (0..size).map(|j| (i * 7 + j * 31) as u8).collect::<Vec<_>>().into())
        .collect()
}

async fn roundtrip(tx: impl AsyncWrite + Unpin + Send + 'static, rx: tokio::io::DuplexStream, capacity: usize) {
    let sent = packets();

    let sender = tokio::spawn({
        let sent = sent.clone();
        async move {
            let mut tx = IoTx::with_capacity(tx, capacity);
            for (i, packet) in sent.into_iter().enumerate() {
                tx.feed(packet).await.unwrap();
                if i % 5 == 0 {
                    tx.flush().await.unwrap();
                }
            }
            tx.close().await.unwrap();
        }
    });

    let mut rx = IoRx::with_capacity(rx, capacity);
    for (i, packet) in sent.iter().enumerate() {
        let received = rx.next().await.unwrap().unwrap();
        assert_eq!(&received, packet, "packet {i}");
    }
    assert!(rx.next().await.is_none());

    sender.await.unwrap();
}

#[tokio::test]
async fn packets_arrive_intact() {
    for (pipe, capacity) in [(7, 8_192), (1_000, 8_192), (65_536, 8_192), (65_536, 65_536), (1 << 20, 100)] {
        println!("pipe {pipe}, capacity {capacity}");

        let (tx, rx) = duplex(pipe);
        roundtrip(tx, rx, capacity).await;

        let (tx, rx) = duplex(pipe);
        roundtrip(NonVectored(tx), rx, capacity).await;
    }
}

#[tokio::test]
async fn corruption_is_detected() {
    for size in [100, 10_000, 100_000] {
        let mut data = Vec::new();
        let mut tx = IoTx::new(&mut data);
        tx.send(Bytes::from(vec![1; size])).await.unwrap();
        let last = data.len() - 1;
        data[last] ^= 1;

        let mut rx = IoRx::new(data.as_slice());
        let err = rx.next().await.unwrap().unwrap_err();
        assert!(matches!(err.get_ref().unwrap().downcast_ref(), Some(IntegrityError::DataCorrupted)), "{err}");
    }
}

#[tokio::test]
async fn oversized_packets_are_rejected() {
    let mut codec = IntegrityCodec::new();
    codec.set_max_packet_size(1_000);

    let mut data = Vec::new();
    let mut tx = IoTx::with_codec(&mut data, codec.clone());
    assert!(tx.send(Bytes::from(vec![0; 1_001])).await.is_err());

    let mut data = Vec::new();
    IoTx::new(&mut data).send(Bytes::from(vec![0; 1_001])).await.unwrap();
    let mut rx = IoRx::with_codec(data.as_slice(), codec);
    assert!(rx.next().await.unwrap().is_err());
}

#[tokio::test]
async fn truncated_stream_fails() {
    let mut data = Vec::new();
    IoTx::new(&mut data).send(Bytes::from(vec![0; 10_000])).await.unwrap();

    for len in [3, 10, 100, 9_000, data.len() - 1] {
        let mut rx = IoRx::new(&data[..len]);
        assert!(rx.next().await.unwrap().is_err(), "{len} bytes");
    }
}
