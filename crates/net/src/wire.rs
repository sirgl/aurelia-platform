//! The byte codec of the protocol: little-endian integers, length-prefixed strings and byte
//! blobs, and nothing else. Every length is checked against the bytes that are left, so a
//! hostile frame cannot make the reader allocate more than the frame holds.
//!
//! A frame on the wire is one WebSocket binary message: `[flags: u8][kind: u8][payload]`.
//! Flag bit 0 says the payload is deflate-compressed (`miniz_oxide`, pure Rust).

use std::fmt;

/// Longest text a message carries (names, chat lines, reasons).
pub const MAX_TEXT: usize = 2048;
/// Largest frame accepted or sent, compressed or not (a full snapshot of a large map is well
/// below it).
pub const MAX_FRAME: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    Truncated,
    Invalid(&'static str),
    TooLong,
    Compression,
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("the frame ends inside a value"),
            Self::Invalid(what) => write!(f, "invalid {what}"),
            Self::TooLong => f.write_str("a length is beyond the limit"),
            Self::Compression => f.write_str("the payload does not inflate"),
        }
    }
}

impl std::error::Error for WireError {}

/// A value that has a place on the wire.
pub trait Wire: Sized {
    fn put(&self, w: &mut Writer);
    fn take(r: &mut Reader<'_>) -> Result<Self, WireError>;
}

#[derive(Debug, Default)]
pub struct Writer {
    pub bytes: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn u8(&mut self, v: u8) {
        self.bytes.push(v);
    }
    pub fn bool(&mut self, v: bool) {
        self.bytes.push(v as u8);
    }
    pub fn u16(&mut self, v: u16) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    pub fn str(&mut self, v: &str) {
        let v = truncate(v, MAX_TEXT);
        self.u16(v.len() as u16);
        self.bytes.extend_from_slice(v.as_bytes());
    }
    pub fn blob(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.bytes.extend_from_slice(v);
    }
    pub fn opt<T: Wire>(&mut self, v: &Option<T>) {
        match v {
            None => self.u8(0),
            Some(v) => {
                self.u8(1);
                v.put(self);
            }
        }
    }
    pub fn list<T: Wire>(&mut self, v: &[T]) {
        self.u32(v.len() as u32);
        for item in v {
            item.put(self);
        }
    }
}

/// `v` cut to at most `max` bytes at a character boundary.
pub fn truncate(v: &str, max: usize) -> &str {
    if v.len() <= max {
        return v;
    }
    let mut end = max;
    while !v.is_char_boundary(end) {
        end -= 1;
    }
    &v[..end]
}

#[derive(Debug)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }
    pub fn is_at_end(&self) -> bool {
        self.at == self.bytes.len()
    }
    fn slice(&mut self, n: usize) -> Result<&'a [u8], WireError> {
        if self.remaining() < n {
            return Err(WireError::Truncated);
        }
        let s = &self.bytes[self.at..self.at + n];
        self.at += n;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8, WireError> {
        Ok(self.slice(1)?[0])
    }
    pub fn bool(&mut self) -> Result<bool, WireError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(WireError::Invalid("bool")),
        }
    }
    pub fn u16(&mut self) -> Result<u16, WireError> {
        Ok(u16::from_le_bytes(
            self.slice(2)?.try_into().expect("two bytes"),
        ))
    }
    pub fn u32(&mut self) -> Result<u32, WireError> {
        Ok(u32::from_le_bytes(
            self.slice(4)?.try_into().expect("four bytes"),
        ))
    }
    pub fn u64(&mut self) -> Result<u64, WireError> {
        Ok(u64::from_le_bytes(
            self.slice(8)?.try_into().expect("eight bytes"),
        ))
    }
    pub fn str(&mut self) -> Result<String, WireError> {
        let n = self.u16()? as usize;
        if n > MAX_TEXT {
            return Err(WireError::TooLong);
        }
        String::from_utf8(self.slice(n)?.to_vec()).map_err(|_| WireError::Invalid("utf-8"))
    }
    pub fn blob(&mut self) -> Result<Vec<u8>, WireError> {
        let n = self.u32()? as usize;
        if n > MAX_FRAME {
            return Err(WireError::TooLong);
        }
        Ok(self.slice(n)?.to_vec())
    }
    pub fn opt<T: Wire>(&mut self) -> Result<Option<T>, WireError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(T::take(self)?)),
            _ => Err(WireError::Invalid("option tag")),
        }
    }
    /// A list whose items take at least one byte each: the count cannot exceed what is left.
    pub fn list<T: Wire>(&mut self) -> Result<Vec<T>, WireError> {
        let n = self.u32()? as usize;
        if n > self.remaining() {
            return Err(WireError::TooLong);
        }
        (0..n).map(|_| T::take(self)).collect()
    }
}

macro_rules! plain {
    ($($t:ty => $put:ident / $take:ident),*) => {$(
        impl Wire for $t {
            fn put(&self, w: &mut Writer) { w.$put(*self) }
            fn take(r: &mut Reader<'_>) -> Result<Self, WireError> { r.$take() }
        }
    )*};
}
plain!(u8 => u8 / u8, u16 => u16 / u16, u32 => u32 / u32, u64 => u64 / u64, bool => bool / bool);

impl Wire for String {
    fn put(&self, w: &mut Writer) {
        w.str(self)
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, WireError> {
        r.str()
    }
}

/// Flag of a frame: the payload is deflated.
const DEFLATED: u8 = 1;
/// Payloads shorter than this are sent as they are.
const COMPRESS_FROM: usize = 256;

/// A message as one frame: `[flags][kind][payload]`, the payload deflated when that makes it
/// shorter.
pub fn frame(kind: u8, payload: &[u8]) -> Vec<u8> {
    if payload.len() >= COMPRESS_FROM {
        let packed = miniz_oxide::deflate::compress_to_vec(payload, 6);
        if packed.len() < payload.len() {
            let mut out = Vec::with_capacity(packed.len() + 2);
            out.extend_from_slice(&[DEFLATED, kind]);
            out.extend_from_slice(&packed);
            return out;
        }
    }
    let mut out = Vec::with_capacity(payload.len() + 2);
    out.extend_from_slice(&[0, kind]);
    out.extend_from_slice(payload);
    out
}

/// The kind and the (inflated) payload of a frame.
pub fn unframe(bytes: &[u8]) -> Result<(u8, Vec<u8>), WireError> {
    if bytes.len() < 2 {
        return Err(WireError::Truncated);
    }
    if bytes.len() > MAX_FRAME {
        return Err(WireError::TooLong);
    }
    let (flags, kind, body) = (bytes[0], bytes[1], &bytes[2..]);
    if flags & !DEFLATED != 0 {
        return Err(WireError::Invalid("frame flags"));
    }
    if flags & DEFLATED == 0 {
        return Ok((kind, body.to_vec()));
    }
    let payload = miniz_oxide::inflate::decompress_to_vec_with_limit(body, MAX_FRAME)
        .map_err(|_| WireError::Compression)?;
    Ok((kind, payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip() {
        let mut w = Writer::new();
        w.u8(7);
        w.u16(513);
        w.u32(70_000);
        w.u64(u64::MAX - 3);
        w.str("héllo");
        w.blob(&[1, 2, 3]);
        w.opt(&Some(5u32));
        w.opt::<u32>(&None);
        w.list(&[1u16, 2, 3]);
        let mut r = Reader::new(&w.bytes);
        assert_eq!(r.u8(), Ok(7));
        assert_eq!(r.u16(), Ok(513));
        assert_eq!(r.u32(), Ok(70_000));
        assert_eq!(r.u64(), Ok(u64::MAX - 3));
        assert_eq!(r.str().as_deref(), Ok("héllo"));
        assert_eq!(r.blob(), Ok(vec![1, 2, 3]));
        assert_eq!(r.opt::<u32>(), Ok(Some(5)));
        assert_eq!(r.opt::<u32>(), Ok(None));
        assert_eq!(r.list::<u16>(), Ok(vec![1, 2, 3]));
        assert!(r.is_at_end());
    }

    #[test]
    fn a_hostile_length_cannot_allocate() {
        // A list that claims four billion items in a six-byte frame.
        let mut w = Writer::new();
        w.u32(u32::MAX);
        w.u16(0);
        assert_eq!(Reader::new(&w.bytes).list::<u16>(), Err(WireError::TooLong));
        let mut w = Writer::new();
        w.u32(u32::MAX);
        assert_eq!(Reader::new(&w.bytes).blob(), Err(WireError::TooLong));
        assert_eq!(Reader::new(&[1, 0]).str(), Err(WireError::Truncated));
    }

    #[test]
    fn frames_compress_what_gains_and_inflate_back() {
        let payload = vec![7u8; 4000];
        let framed = frame(9, &payload);
        assert!(
            framed.len() < 100,
            "a run of one byte deflates well ({})",
            framed.len()
        );
        assert_eq!(unframe(&framed), Ok((9, payload)));
        let small = frame(3, b"abc");
        assert_eq!(small, vec![0, 3, b'a', b'b', b'c']);
        assert_eq!(unframe(&small), Ok((3, b"abc".to_vec())));
        assert_eq!(unframe(&[0]), Err(WireError::Truncated));
        assert_eq!(unframe(&[4, 1]), Err(WireError::Invalid("frame flags")));
        assert_eq!(unframe(&[1, 1, 0xff, 0xff]), Err(WireError::Compression));
    }

    #[test]
    fn a_long_text_is_cut_at_a_character() {
        let long = "\u{44f}".repeat(MAX_TEXT);
        let mut w = Writer::new();
        w.str(&long);
        let back = Reader::new(&w.bytes).str().expect("reads");
        assert!(back.len() <= MAX_TEXT && back.chars().all(|c| c == '\u{44f}'));
    }
}
