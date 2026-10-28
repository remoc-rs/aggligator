//! Integrity codec.

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::{fmt, io, mem::size_of};
use tokio_util::codec::{Decoder, Encoder};

/// CRC-32 (ISO-HDLC) checksum of the data.
///
/// crc-fast is faster on WebAssembly, while crc32fast is faster on other platforms.
fn checksum(data: &[u8]) -> u32 {
    #[cfg(target_family = "wasm")]
    {
        crc_fast::crc32_iso_hdlc(data)
    }

    #[cfg(not(target_family = "wasm"))]
    {
        crc32fast::hash(data)
    }
}

/// A packet decoding error.
#[derive(Debug, Clone)]
pub enum IntegrityError {
    /// A packet exceeds the maximum allowed size.
    PacketTooBig,
    /// A sequence number was skipped or corrupted.
    SeqSkipped,
    /// Data checksum verification failed.
    DataCorrupted,
}

impl fmt::Display for IntegrityError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::PacketTooBig => write!(f, "packet too big"),
            Self::SeqSkipped => write!(f, "sequence number skipped"),
            Self::DataCorrupted => write!(f, "data corrupted"),
        }
    }
}

impl std::error::Error for IntegrityError {}

/// A codec for frames delimited by a header specifying their lengths, sequence number and checksums.
///
/// The data integrity is verified using the CRC32 checksum.
#[derive(Debug, Clone)]
pub struct IntegrityCodec {
    /// Maximum frame length.
    max_frame_len: u32,
    /// Read state
    state: DecodeState,
    /// Next sequence number for decoding
    decode_seq: u16,
    /// Next sequence number for encoding
    encode_seq: u16,
    /// Decode buffer size.
    decode_buffer_size: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
enum DecodeState {
    Header,
    Data(Header),
}

/// Header of a packet.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Header {
    length: u32,
    checksum: u32,
}

impl Header {
    /// Length of the packet data.
    pub(crate) fn len(&self) -> usize {
        self.length as usize
    }

    /// Verifies the packet data against the checksum.
    pub(crate) fn verify(&self, data: &[u8]) -> io::Result<()> {
        if self.checksum != checksum(data) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, IntegrityError::DataCorrupted));
        }
        Ok(())
    }
}

impl IntegrityCodec {
    /// Length of a packet header.
    pub(crate) const HEADER_LEN: usize = size_of::<u32>() + size_of::<u16>() + size_of::<u32>();

    /// Creates a new `IntegrityCodec` with the default configuration values.
    pub fn new() -> Self {
        Self {
            max_frame_len: 8 * 1_024 * 1_024,
            state: DecodeState::Header,
            decode_seq: 0,
            encode_seq: 0,
            decode_buffer_size: None,
        }
    }

    /// Returns the maximum packet size.
    ///
    /// This is the largest size this codec will accept from the wire.
    /// Larger packets will be rejected.
    pub fn max_packet_size(&self) -> u32 {
        self.max_frame_len
    }

    /// Sets the maximum packet size.
    ///
    /// This is the largest size this codec will accept from and send to the wire.
    /// Larger packets will be rejected.
    pub fn set_max_packet_size(&mut self, max_packet_size: u32) {
        self.max_frame_len = max_packet_size;
    }

    /// Reserve at least `additional` space in buffer `buf`.
    ///
    /// When a reservation needs to be performed, at least
    /// [`Self::decode_buffer_size`] is reserved.
    fn reserve(&self, buf: &mut BytesMut, mut additional: usize) {
        let rem = buf.capacity() - buf.len();
        if additional <= rem {
            return;
        }

        if let Some(decode_buffer_size) = self.decode_buffer_size {
            additional = additional.max(decode_buffer_size);
        }

        buf.reserve(additional);
    }

    /// Appends the header of a packet containing `data` to `dst`.
    pub(crate) fn encode_header(&mut self, data: &[u8], dst: &mut BytesMut) -> io::Result<()> {
        if data.len() > self.max_frame_len as usize {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, IntegrityError::PacketTooBig));
        }

        dst.put_u32(data.len() as u32);

        dst.put_u16(self.encode_seq);
        self.encode_seq = self.encode_seq.wrapping_add(1);

        dst.put_u32(checksum(data));

        Ok(())
    }

    /// Decodes the packet header at the start of `src`, removing it.
    ///
    /// Returns `None` if `src` does not contain the complete header yet.
    pub(crate) fn decode_header(&mut self, src: &mut BytesMut) -> io::Result<Option<Header>> {
        if src.len() < Self::HEADER_LEN {
            return Ok(None);
        }

        let length = src.get_u32();
        if length > self.max_frame_len {
            return Err(io::Error::new(io::ErrorKind::InvalidData, IntegrityError::PacketTooBig));
        }

        let seq = src.get_u16();
        if seq != self.decode_seq {
            return Err(io::Error::new(io::ErrorKind::InvalidData, IntegrityError::SeqSkipped));
        }
        self.decode_seq = self.decode_seq.wrapping_add(1);

        let checksum = src.get_u32();

        Ok(Some(Header { length, checksum }))
    }
}

impl Decoder for IntegrityCodec {
    type Item = BytesMut;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> io::Result<Option<BytesMut>> {
        if self.decode_buffer_size.is_none() {
            self.decode_buffer_size = Some(src.capacity());
        }

        let header = match self.state {
            DecodeState::Header => match self.decode_header(src)? {
                Some(header) => {
                    self.reserve(src, header.len().saturating_sub(src.len()));
                    self.state = DecodeState::Data(header);
                    header
                }
                None => {
                    self.reserve(src, Self::HEADER_LEN);
                    return Ok(None);
                }
            },
            DecodeState::Data(header) => header,
        };

        if src.len() < header.len() {
            return Ok(None);
        }

        let data = src.split_to(header.len());
        header.verify(&data)?;

        self.state = DecodeState::Header;
        self.reserve(src, src.len().saturating_sub(Self::HEADER_LEN));
        Ok(Some(data))
    }
}

impl Encoder<Bytes> for IntegrityCodec {
    type Error = io::Error;

    fn encode(&mut self, data: Bytes, dst: &mut BytesMut) -> io::Result<()> {
        self.encode_header(&data, dst)?;
        dst.extend_from_slice(&data);
        Ok(())
    }
}

impl Default for IntegrityCodec {
    fn default() -> Self {
        Self::new()
    }
}
