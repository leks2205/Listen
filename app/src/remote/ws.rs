//! A small RFC 6455 WebSocket: the handshake and the frames a remote control
//! client needs (text, ping, pong, close, fragmented messages).

use base64::Engine;
use sha1::{Digest, Sha1};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
/// Longest message accepted from a client.
pub const MAX_MESSAGE: usize = 1 << 20;

/// The `Sec-WebSocket-Accept` value for a client key.
pub fn accept_key(key: &str) -> String {
    let mut h = Sha1::new();
    h.update(key.trim().as_bytes());
    h.update(GUID.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(h.finalize())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opcode {
    Continuation,
    Text,
    Binary,
    Close,
    Ping,
    Pong,
}

impl Opcode {
    fn from(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::Continuation,
            1 => Self::Text,
            2 => Self::Binary,
            8 => Self::Close,
            9 => Self::Ping,
            10 => Self::Pong,
            _ => return None,
        })
    }

    fn code(self) -> u8 {
        match self {
            Self::Continuation => 0,
            Self::Text => 1,
            Self::Binary => 2,
            Self::Close => 8,
            Self::Ping => 9,
            Self::Pong => 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub fin: bool,
    pub opcode: Opcode,
    pub payload: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Protocol violation or too large: close the connection.
    Invalid(&'static str),
}

/// Takes one complete client frame off the front of `buf`; `Ok(None)` = more bytes needed.
pub fn parse(buf: &mut Vec<u8>) -> Result<Option<Frame>, ParseError> {
    if buf.len() < 2 {
        return Ok(None);
    }
    let (b0, b1) = (buf[0], buf[1]);
    if b0 & 0x70 != 0 {
        return Err(ParseError::Invalid("reserved bits"));
    }
    let opcode = Opcode::from(b0 & 0x0f).ok_or(ParseError::Invalid("opcode"))?;
    if b1 & 0x80 == 0 {
        return Err(ParseError::Invalid("client frames must be masked"));
    }
    let mut len = u64::from(b1 & 0x7f);
    let mut at = 2;
    if len == 126 {
        if buf.len() < 4 {
            return Ok(None);
        }
        len = u64::from(u16::from_be_bytes([buf[2], buf[3]]));
        at = 4;
    } else if len == 127 {
        if buf.len() < 10 {
            return Ok(None);
        }
        len = u64::from_be_bytes(buf[2..10].try_into().expect("8 bytes"));
        at = 10;
    }
    if len > MAX_MESSAGE as u64 {
        return Err(ParseError::Invalid("message too large"));
    }
    let len = len as usize;
    let control = matches!(opcode, Opcode::Close | Opcode::Ping | Opcode::Pong);
    if control && (len > 125 || b0 & 0x80 == 0) {
        return Err(ParseError::Invalid("bad control frame"));
    }
    if buf.len() < at + 4 + len {
        return Ok(None);
    }
    let mask: [u8; 4] = buf[at..at + 4].try_into().expect("4 bytes");
    let payload: Vec<u8> =
        buf[at + 4..at + 4 + len].iter().enumerate().map(|(i, b)| b ^ mask[i % 4]).collect();
    buf.drain(..at + 4 + len);
    Ok(Some(Frame { fin: b0 & 0x80 != 0, opcode, payload }))
}

/// A server frame (never masked).
pub fn encode(opcode: Opcode, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0x80 | opcode.code()];
    match payload.len() {
        n if n < 126 => out.push(n as u8),
        n if n <= usize::from(u16::MAX) => {
            out.push(126);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            out.push(127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    out.extend_from_slice(payload);
    out
}

pub fn text(s: &str) -> Vec<u8> {
    encode(Opcode::Text, s.as_bytes())
}

pub fn close(code: u16, reason: &str) -> Vec<u8> {
    let mut p = code.to_be_bytes().to_vec();
    p.extend_from_slice(&reason.as_bytes()[..reason.len().min(100)]);
    encode(Opcode::Close, &p)
}

/// Reassembles fragmented text messages.
#[derive(Default)]
pub struct Assembler {
    partial: Vec<u8>,
    active: bool,
}

pub enum Event {
    Message(String),
    Ping(Vec<u8>),
    Pong,
    Close,
}

impl Assembler {
    pub fn push(&mut self, f: Frame) -> Result<Option<Event>, ParseError> {
        match f.opcode {
            Opcode::Ping => Ok(Some(Event::Ping(f.payload))),
            Opcode::Pong => Ok(Some(Event::Pong)),
            Opcode::Close => Ok(Some(Event::Close)),
            Opcode::Text | Opcode::Binary | Opcode::Continuation => {
                let first = f.opcode != Opcode::Continuation;
                if first == self.active {
                    return Err(ParseError::Invalid("fragment order"));
                }
                self.partial.extend_from_slice(&f.payload);
                if self.partial.len() > MAX_MESSAGE {
                    return Err(ParseError::Invalid("message too large"));
                }
                self.active = !f.fin;
                if !f.fin {
                    return Ok(None);
                }
                let data = std::mem::take(&mut self.partial);
                if f.opcode == Opcode::Binary {
                    return Ok(None); // the protocol is text only
                }
                String::from_utf8(data)
                    .map(|s| Some(Event::Message(s)))
                    .map_err(|_| ParseError::Invalid("utf-8"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn masked(opcode: u8, fin: bool, payload: &[u8]) -> Vec<u8> {
        let mask = [1u8, 2, 3, 4];
        let mut v = vec![if fin { 0x80 } else { 0 } | opcode];
        match payload.len() {
            n if n < 126 => v.push(0x80 | n as u8),
            n => {
                v.push(0x80 | 126);
                v.extend_from_slice(&(n as u16).to_be_bytes());
            }
        }
        v.extend_from_slice(&mask);
        v.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        v
    }

    #[test]
    fn rfc_accept_key() {
        // The example of RFC 6455 section 1.3.
        assert_eq!(accept_key("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn parses_masked_frames_incrementally() {
        let full = masked(1, true, b"hello");
        let mut buf = full[..4].to_vec();
        assert_eq!(parse(&mut buf), Ok(None));
        buf.extend_from_slice(&full[4..]);
        buf.extend_from_slice(&masked(9, true, b"p"));
        let f = parse(&mut buf).unwrap().unwrap();
        assert_eq!((f.opcode, f.payload.as_slice()), (Opcode::Text, &b"hello"[..]));
        let f = parse(&mut buf).unwrap().unwrap();
        assert_eq!(f.opcode, Opcode::Ping);
        assert!(buf.is_empty());
        let long = vec![b'x'; 300];
        let mut buf = masked(1, true, &long);
        assert_eq!(parse(&mut buf).unwrap().unwrap().payload, long);
    }

    #[test]
    fn rejects_unmasked_and_oversized_frames() {
        let mut unmasked = vec![0x81, 0x01, b'x'];
        assert!(parse(&mut unmasked).is_err());
        let mut huge = vec![0x81, 0x80 | 127];
        huge.extend_from_slice(&(MAX_MESSAGE as u64 + 1).to_be_bytes());
        assert!(parse(&mut huge).is_err());
        let mut bad_control = masked(9, false, b"x");
        assert!(parse(&mut bad_control).is_err());
    }

    #[test]
    fn fragments_are_joined() {
        let mut a = Assembler::default();
        let parse_one = |bytes: Vec<u8>| parse(&mut bytes.clone()).unwrap().unwrap();
        assert!(a.push(parse_one(masked(1, false, b"he"))).unwrap().is_none());
        let Some(Event::Message(m)) = a.push(parse_one(masked(0, true, b"llo"))).unwrap() else {
            panic!("message")
        };
        assert_eq!(m, "hello");
        assert!(a.push(parse_one(masked(0, true, b"x"))).is_err());
    }

    #[test]
    fn encodes_lengths() {
        assert_eq!(&text("hi")[..2], &[0x81, 2]);
        let big = text(&"x".repeat(200));
        assert_eq!(&big[..4], &[0x81, 126, 0, 200]);
        assert_eq!(&close(4004, "no")[..4], &[0x88, 4, 0x0f, 0xa4]);
    }
}
