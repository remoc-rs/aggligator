//! Wrapper types for stream-based links.
//!
//! These wrapper types turn stream-based links into packet-based links
//! by applying the [integrity codec](IntegrityCodec).
//!
//! They are applied by the [`Server::add_incoming_io`](crate::connect::Server::add_incoming_io)
//! and [`Control::add_io`](crate::control::Control::add_io) methods to stream-based links,
//! using the default configuration of the integrity codec.
//!

mod codec;

use bytes::{Buf, BufMut, Bytes, BytesMut, buf::Limit};
use futures::{Sink, SinkExt, Stream, StreamExt, ready};
use std::{
    collections::VecDeque,
    fmt,
    io::{self, IoSlice},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_util::io::poll_read_buf;

pub use codec::*;

#[cfg(test)]
mod tests;

/// An [`AsyncRead`] that can be used by the connection task.
///
/// This is automatically implemented for every type fulfilling the bounds.
pub trait DynRead: AsyncRead + Send + Sync + 'static {}
impl<T> DynRead for T where T: AsyncRead + Send + Sync + 'static + ?Sized {}

/// An [`AsyncWrite`] that can be used by the connection task.
///
/// This is automatically implemented for every type fulfilling the bounds.
pub trait DynWrite: AsyncWrite + Send + Sync + 'static {}
impl<T> DynWrite for T where T: AsyncWrite + Send + Sync + 'static + ?Sized {}

/// A packet [`Sink`] that can be used by the connection task.
///
/// This is automatically implemented for every type fulfilling the bounds.
pub trait DynSink: Sink<Bytes, Error = io::Error> + Send + Sync + 'static {}
impl<T> DynSink for T where T: Sink<Bytes, Error = io::Error> + Send + Sync + 'static + ?Sized {}

/// A packet [`Stream`] that can be used by the connection task.
///
/// This is automatically implemented for every type fulfilling the bounds.
pub trait DynStream: Stream<Item = io::Result<Bytes>> + Send + Sync + 'static {}
impl<T> DynStream for T where T: Stream<Item = io::Result<Bytes>> + Send + Sync + 'static + ?Sized {}

/// Default capacity of the buffers of [`IoTx`] and [`IoRx`].
const DEFAULT_CAPACITY: usize = 8_192;

/// Minimum size of a packet that is written directly from its buffer or
/// received into a buffer of its own.
const DIRECT_MIN: usize = 4_096;

/// Maximum number of buffers passed to a vectored write.
const MAX_SLICES: usize = 64;

/// Transmit wrapper for using an IO-stream-based link.
///
/// Packet headers and small packets are collected in a buffer, while large packets
/// are written directly from their own buffers using vectored writes, so that their
/// data is not copied. The underlying writer is only flushed when the sink is flushed
/// or closed.
pub struct IoTx<W> {
    inner: W,
    codec: IntegrityCodec,
    /// Data to write, in order, followed by the data in `buf`.
    queue: VecDeque<Bytes>,
    /// Bytes in `queue`.
    queued: usize,
    /// Collects packet headers and small packets.
    buf: BytesMut,
    /// Amount of pending data from which no further packets are accepted
    /// until it has been written.
    backpressure: usize,
}

impl<W> fmt::Debug for IoTx<W>
where
    W: fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_tuple("IoTx").field(&self.inner).finish()
    }
}

impl<W> IoTx<W>
where
    W: AsyncWrite,
{
    /// Wraps an IO writer using the default configuration of the integrity codec.
    pub fn new(write: W) -> Self {
        Self::with_codec(write, IntegrityCodec::new())
    }

    /// Wraps and IO writer using the default configuration of the integrity codec and
    /// the specified write buffer capacity.
    pub fn with_capacity(write: W, capacity: usize) -> Self {
        Self::with_codec_and_capacity(write, IntegrityCodec::new(), capacity)
    }

    /// Wraps an IO writer using a customized integrity codec.
    pub fn with_codec(write: W, codec: IntegrityCodec) -> Self {
        Self::with_codec_and_capacity(write, codec, DEFAULT_CAPACITY)
    }

    /// Wraps an IO writer using a customized integrity codec and write buffer capacity.
    pub fn with_codec_and_capacity(write: W, codec: IntegrityCodec, capacity: usize) -> Self {
        Self {
            inner: write,
            codec,
            queue: VecDeque::new(),
            queued: 0,
            buf: BytesMut::with_capacity(capacity),
            backpressure: capacity,
        }
    }
}

impl<W> IoTx<W>
where
    W: AsyncWrite + Unpin,
{
    /// Amount of data not written yet.
    fn pending(&self) -> usize {
        self.queued + self.buf.len()
    }

    /// Writes all pending data to the underlying writer.
    fn poll_write_pending(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        if !self.buf.is_empty() {
            self.queued += self.buf.len();
            self.queue.push_back(self.buf.split().freeze());
        }

        while self.queued > 0 {
            let mut slices = [IoSlice::new(&[]); MAX_SLICES];
            let cnt = self.queue.len().min(MAX_SLICES);
            for (slice, data) in slices.iter_mut().zip(&self.queue) {
                *slice = IoSlice::new(data);
            }

            let mut n = ready!(Pin::new(&mut self.inner).poll_write_vectored(cx, &slices[..cnt]))?;
            if n == 0 {
                return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
            }

            self.queued -= n;
            while n > 0 {
                let front = self.queue.front_mut().unwrap();
                if n < front.len() {
                    front.advance(n);
                    n = 0;
                } else {
                    n -= front.len();
                    self.queue.pop_front();
                }
            }
        }

        Poll::Ready(Ok(()))
    }
}

impl<W> Sink<Bytes> for IoTx<W>
where
    W: AsyncWrite + Unpin,
{
    type Error = io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        let this = Pin::into_inner(self);
        if this.pending() >= this.backpressure {
            ready!(this.poll_write_pending(cx))?;
        }
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: Bytes) -> Result<(), Self::Error> {
        let this = Pin::into_inner(self);
        this.codec.encode_header(&item, &mut this.buf)?;

        if item.len() >= DIRECT_MIN && this.inner.is_write_vectored() {
            this.queued += this.buf.len() + item.len();
            this.queue.push_back(this.buf.split().freeze());
            this.queue.push_back(item);
        } else {
            this.buf.extend_from_slice(&item);
        }

        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        let this = Pin::into_inner(self);
        ready!(this.poll_write_pending(cx))?;
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        let this = Pin::into_inner(self);
        ready!(this.poll_write_pending(cx))?;
        ready!(Pin::new(&mut this.inner).poll_flush(cx))?;
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

/// Receive wrapper for using an IO-stream-based link.
///
/// Small packets that have been received completely while reading are copied out of
/// the read buffer, so that it can be reused. The rest of a large packet that has not
/// been received completely is read directly into its own buffer, so that its data is
/// not copied.
pub struct IoRx<R> {
    inner: R,
    codec: IntegrityCodec,
    /// Size of reads while no packet is being received.
    read_size: usize,
    /// Received data not decoded yet.
    buf: BytesMut,
    /// Header of the packet whose rest is being read directly into its buffer,
    /// limited to its remaining length.
    packet: Option<(Header, Limit<BytesMut>)>,
}

impl<R> fmt::Debug for IoRx<R>
where
    R: fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_tuple("IoRx").field(&self.inner).finish()
    }
}

impl<R> IoRx<R>
where
    R: AsyncRead,
{
    /// Wraps an IO reader using the default configuration of the integrity codec.
    pub fn new(read: R) -> Self {
        Self::with_codec(read, IntegrityCodec::new())
    }

    /// Wraps and IO reader using the default configuration of the integrity codec and
    /// the specified read buffer capacity.
    pub fn with_capacity(read: R, capacity: usize) -> Self {
        Self::with_codec_and_capacity(read, IntegrityCodec::new(), capacity)
    }

    /// Wraps an IO reader using a customized integrity codec.
    pub fn with_codec(read: R, codec: IntegrityCodec) -> Self {
        Self::with_codec_and_capacity(read, codec, DEFAULT_CAPACITY)
    }

    /// Wraps an IO reader using a customized integrity codec and read buffer capacity.
    pub fn with_codec_and_capacity(read: R, codec: IntegrityCodec, capacity: usize) -> Self {
        let read_size = capacity.max(IntegrityCodec::HEADER_LEN);
        Self { inner: read, codec, read_size, buf: BytesMut::with_capacity(read_size), packet: None }
    }
}

impl<R> Stream for IoRx<R>
where
    R: AsyncRead + Unpin,
{
    type Item = Result<Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let this = Pin::into_inner(self);

        loop {
            match &mut this.packet {
                Some((_, data)) if data.remaining_mut() > 0 => {
                    if ready!(poll_read_buf(Pin::new(&mut this.inner), cx, data))? == 0 {
                        return Poll::Ready(Some(Err(io::ErrorKind::UnexpectedEof.into())));
                    }
                }
                Some(_) => {
                    let (header, data) = this.packet.take().unwrap();
                    let data = data.into_inner().freeze();
                    header.verify(&data)?;
                    return Poll::Ready(Some(Ok(data)));
                }
                None => match this.codec.decode_header(&mut this.buf)? {
                    Some(header) if this.buf.len() >= header.len() => {
                        let data = if header.len() >= DIRECT_MIN {
                            this.buf.split_to(header.len()).freeze()
                        } else {
                            let data = Bytes::copy_from_slice(&this.buf[..header.len()]);
                            this.buf.advance(header.len());
                            data
                        };
                        header.verify(&data)?;
                        return Poll::Ready(Some(Ok(data)));
                    }
                    Some(header) => {
                        let mut data = BytesMut::with_capacity(header.len());
                        data.extend_from_slice(&this.buf);
                        this.buf.clear();
                        let remaining = header.len() - data.len();
                        this.packet = Some((header, data.limit(remaining)));
                    }
                    None => {
                        if this.buf.capacity() - this.buf.len() < this.read_size {
                            this.buf.reserve(this.read_size);
                        }
                        if ready!(poll_read_buf(Pin::new(&mut this.inner), cx, &mut this.buf))? == 0 {
                            if this.buf.is_empty() {
                                return Poll::Ready(None);
                            }
                            return Poll::Ready(Some(Err(io::Error::new(
                                io::ErrorKind::UnexpectedEof,
                                "connection closed within a packet",
                            ))));
                        }
                    }
                },
            }
        }
    }
}

/// Type-neutral transmit wrapper for using an IO-stream-based link.
///
/// Useful if a connection consists of different types of links.
pub type IoTxBox = IoTx<Pin<Box<dyn DynWrite>>>;

/// Type-neutral receive wrapper for using an IO-stream-based link.
///
/// Useful if a connection consists of different types of links.
pub type IoRxBox = IoRx<Pin<Box<dyn DynRead>>>;

/// A stream, either packet-based or IO-based.
pub enum StreamBox {
    /// Packet-based stream.
    TxRx(TxRxBox),
    /// IO-based stream.
    Io(IoBox),
}

impl StreamBox {
    /// Make stream packet-based.
    ///
    /// A packet-based stream is unaffacted.
    /// An IO-based stream is wrapped in the integrity codec.
    pub fn into_tx_rx(self) -> TxRxBox {
        match self {
            Self::TxRx(tx_rx) => tx_rx,
            Self::Io(IoBox { read, write }) => {
                let tx = IoTxBox::new(write);
                let rx = IoRxBox::new(read);
                TxRxBox::new(tx, rx)
            }
        }
    }

    /// Make stream packet-based with specified buffer capacity.
    ///
    /// A packet-based stream is unaffacted.
    /// An IO-based stream is wrapped in the integrity codec with the specified buffer capacity.    
    pub fn into_tx_rx_with_capacity(self, capacity: usize) -> TxRxBox {
        match self {
            Self::TxRx(tx_rx) => tx_rx,
            Self::Io(IoBox { read, write }) => {
                let tx = IoTxBox::with_capacity(write, capacity);
                let rx = IoRxBox::with_capacity(read, capacity);
                TxRxBox::new(tx, rx)
            }
        }
    }
}

impl From<TxRxBox> for StreamBox {
    fn from(value: TxRxBox) -> Self {
        Self::TxRx(value)
    }
}

impl From<IoBox> for StreamBox {
    fn from(value: IoBox) -> Self {
        Self::Io(value)
    }
}

pub(crate) type TxBox = Pin<Box<dyn DynSink>>;
pub(crate) type RxBox = Pin<Box<dyn DynStream>>;

/// A boxed packet-based stream.
pub struct TxRxBox {
    /// Sender.
    pub tx: TxBox,
    /// Receiver.
    pub rx: RxBox,
}

impl TxRxBox {
    /// Creates a new instance.
    pub fn new(tx: impl DynSink, rx: impl DynStream) -> Self {
        Self { tx: Box::pin(tx), rx: Box::pin(rx) }
    }

    /// Splits this into boxed transmitter and receiver.
    pub fn into_split(self) -> (TxBox, RxBox) {
        let Self { tx, rx } = self;
        (tx, rx)
    }
}

impl Sink<Bytes> for TxRxBox {
    type Error = io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        self.get_mut().tx.poll_ready_unpin(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Bytes) -> io::Result<()> {
        self.get_mut().tx.start_send_unpin(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        self.get_mut().tx.poll_flush_unpin(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        self.get_mut().tx.poll_close_unpin(cx)
    }
}

impl Stream for TxRxBox {
    type Item = io::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.get_mut().rx.poll_next_unpin(cx)
    }
}

pub(crate) type ReadBox = Pin<Box<dyn DynRead>>;
pub(crate) type WriteBox = Pin<Box<dyn DynWrite>>;

/// A boxed IO stream.
pub struct IoBox {
    /// Reader.
    pub read: ReadBox,
    /// Writer.
    pub write: WriteBox,
}

impl IoBox {
    /// Creates a new instance.
    pub fn new(read: impl DynRead, write: impl DynWrite) -> Self {
        Self { read: Box::pin(read), write: Box::pin(write) }
    }

    /// Splits this into boxed reader and writer.
    pub fn into_split(self) -> (ReadBox, WriteBox) {
        let Self { read, write } = self;
        (read, write)
    }
}

impl AsyncRead for IoBox {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context, buf: &mut ReadBuf) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().read).poll_read(cx, buf)
    }
}

impl AsyncWrite for IoBox {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().write).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().write).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().write).poll_shutdown(cx)
    }
}
