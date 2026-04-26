//! Telnet line codec: IAC stripping, line splitting, overflow protection.

use bytes::BytesMut;
use liaozhai_core::constants;
use std::borrow::Cow;
use tokio_util::codec::Decoder;
use tracing::warn;

// Telnet protocol bytes
const IAC: u8 = 0xFF;
const SB: u8 = 0xFA;
const SE: u8 = 0xF0;
const WILL: u8 = 0xFB;
const WONT: u8 = 0xFC;
const DO: u8 = 0xFD;
const DONT: u8 = 0xFE;

/// Errors produced by the telnet line codec.
///
/// Only fatal errors that require disconnecting. Recoverable conditions
/// (line too long) are signaled through [`CodecItem`] instead, so that
/// `FramedRead` continues delivering items after a non-fatal event.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TelnetCodecError {
    /// An underlying I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The per-connection buffer has exceeded its cap.
    #[error("buffer overflow (max {max} bytes)")]
    BufferOverflow { max: usize },
}

impl From<TelnetCodecError> for liaozhai_core::error::Error {
    fn from(e: TelnetCodecError) -> Self {
        match e {
            TelnetCodecError::Io(io_err) => Self::Io(io_err),
            other => Self::Codec(other.to_string()),
        }
    }
}

/// Items produced by the telnet line codec.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodecItem {
    /// A complete line of input.
    Line(String),
    /// A line exceeded the maximum length and was discarded.
    LineTooLong,
}

/// A tokio codec that reads telnet-framed, line-oriented input.
///
/// Strips IAC sequences, splits on line endings (CRLF, LF, or lone CR),
/// and returns lines as UTF-8 strings (lossy).
#[derive(Debug)]
pub struct TelnetLineCodec {
    max_line_length: usize,
    max_buffer_size: usize,
    discarding: bool,
    in_subnegotiation: bool,
    subneg_iac_pending: bool,
}

impl TelnetLineCodec {
    pub fn new() -> Self {
        Self {
            max_line_length: constants::MAX_LINE_LENGTH,
            max_buffer_size: constants::MAX_BUFFER_SIZE,
            discarding: false,
            in_subnegotiation: false,
            subneg_iac_pending: false,
        }
    }

    pub fn with_limits(max_line_length: usize, max_buffer_size: usize) -> Self {
        Self {
            max_line_length,
            max_buffer_size,
            discarding: false,
            in_subnegotiation: false,
            subneg_iac_pending: false,
        }
    }

    /// Strip telnet IAC sequences from `buf` in-place.
    fn strip_iac(&mut self, buf: &mut BytesMut) {
        let mut read = 0;
        let mut write = 0;
        let len = buf.len();

        while read < len {
            if self.in_subnegotiation {
                if self.subneg_iac_pending {
                    self.subneg_iac_pending = false;
                    if buf[read] == SE {
                        self.in_subnegotiation = false;
                        read += 1;
                        continue;
                    }
                    if buf[read] != IAC {
                        read += 1;
                        continue;
                    }
                }

                if buf[read] == IAC {
                    self.subneg_iac_pending = true;
                    read += 1;
                    continue;
                }

                read += 1;
                continue;
            }

            // Normal mode
            if buf[read] != IAC {
                if write != read {
                    buf[write] = buf[read];
                }
                write += 1;
                read += 1;
                continue;
            }

            // IAC at buf[read]
            if read + 1 >= len {
                if write != read {
                    buf[write] = buf[read];
                }
                write += 1;
                break;
            }

            let cmd = buf[read + 1];

            match cmd {
                IAC => {
                    buf[write] = IAC;
                    write += 1;
                    read += 2;
                }
                WILL | WONT | DO | DONT => {
                    if read + 2 >= len {
                        let remaining = len - read;
                        for i in 0..remaining {
                            buf[write + i] = buf[read + i];
                        }
                        write += remaining;
                        break;
                    }
                    read += 3;
                }
                SB => {
                    self.in_subnegotiation = true;
                    self.subneg_iac_pending = false;
                    read += 2;
                }
                _ => {
                    read += 2;
                }
            }
        }

        buf.truncate(write);
    }
}

impl Default for TelnetLineCodec {
    fn default() -> Self {
        Self::new()
    }
}

fn find_line_terminator(buf: &BytesMut) -> Option<usize> {
    buf.iter().position(|&b| b == b'\n' || b == b'\r')
}

fn consume_terminator(buf: &BytesMut, pos: usize) -> usize {
    if buf[pos] == b'\r' && pos + 1 < buf.len() && buf[pos + 1] == b'\n' {
        pos + 2
    } else {
        pos + 1
    }
}

fn lossy_decode_with_warning(bytes: &[u8]) -> String {
    let line = String::from_utf8_lossy(bytes);
    if matches!(line, Cow::Owned(_)) {
        warn!(
            byte_count = bytes.len(),
            "invalid UTF-8 in input line; replaced with U+FFFD"
        );
    }
    line.into_owned()
}

impl Decoder for TelnetLineCodec {
    type Item = CodecItem;
    type Error = TelnetCodecError;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<CodecItem>, TelnetCodecError> {
        if buf.len() > self.max_buffer_size {
            return Err(TelnetCodecError::BufferOverflow {
                max: self.max_buffer_size,
            });
        }

        self.strip_iac(buf);

        if let Some(term_pos) = find_line_terminator(buf) {
            if self.discarding {
                let consume_len = consume_terminator(buf, term_pos);
                let _ = buf.split_to(consume_len);
                self.discarding = false;
                return self.decode(buf);
            }

            let line_bytes = buf[..term_pos].to_vec();
            let consume_len = consume_terminator(buf, term_pos);
            let _ = buf.split_to(consume_len);

            let line = lossy_decode_with_warning(&line_bytes);
            return Ok(Some(CodecItem::Line(line)));
        }

        if self.discarding {
            buf.clear();
            return Ok(None);
        }

        if buf.len() > self.max_line_length {
            buf.clear();
            self.discarding = true;
            return Ok(Some(CodecItem::LineTooLong));
        }

        Ok(None)
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<CodecItem>, TelnetCodecError> {
        match self.decode(buf)? {
            Some(item) => Ok(Some(item)),
            None => {
                if buf.is_empty() || self.discarding {
                    buf.clear();
                    self.discarding = false;
                    Ok(None)
                } else if self.in_subnegotiation || (!buf.is_empty() && buf[0] == IAC) {
                    buf.clear();
                    self.in_subnegotiation = false;
                    self.subneg_iac_pending = false;
                    Ok(None)
                } else {
                    // Strip any trailing incomplete IAC sequence before emitting
                    // the final line. At EOF, no further bytes are coming.
                    let mut end = buf.len();
                    if end >= 2
                        && buf[end - 2] == IAC
                        && matches!(buf[end - 1], WILL | WONT | DO | DONT)
                    {
                        end -= 2;
                    } else if end >= 1 && buf[end - 1] == IAC {
                        end -= 1;
                    }

                    if end == 0 {
                        buf.clear();
                        return Ok(None);
                    }

                    let line_bytes = buf[..end].to_vec();
                    buf.clear();
                    let line = lossy_decode_with_warning(&line_bytes);
                    Ok(Some(CodecItem::Line(line)))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_one(
        codec: &mut TelnetLineCodec,
        data: &[u8],
    ) -> Result<Option<CodecItem>, TelnetCodecError> {
        let mut buf = BytesMut::from(data);
        codec.decode(&mut buf)
    }

    fn decode_line(codec: &mut TelnetLineCodec, data: &[u8]) -> Option<String> {
        match decode_one(codec, data).unwrap() {
            Some(CodecItem::Line(s)) => Some(s),
            _ => None,
        }
    }

    // --- Line splitting ---

    #[test]
    fn line_ending_crlf() {
        let mut codec = TelnetLineCodec::new();
        assert_eq!(decode_line(&mut codec, b"hello\r\n"), Some("hello".into()));
    }

    #[test]
    fn line_ending_lf() {
        let mut codec = TelnetLineCodec::new();
        assert_eq!(decode_line(&mut codec, b"hello\n"), Some("hello".into()));
    }

    #[test]
    fn line_ending_lone_cr() {
        let mut codec = TelnetLineCodec::new();
        assert_eq!(decode_line(&mut codec, b"hello\r"), Some("hello".into()));
    }

    #[test]
    fn empty_line_crlf() {
        let mut codec = TelnetLineCodec::new();
        assert_eq!(decode_line(&mut codec, b"\r\n"), Some(String::new()));
    }

    #[test]
    fn empty_line_lf() {
        let mut codec = TelnetLineCodec::new();
        assert_eq!(decode_line(&mut codec, b"\n"), Some(String::new()));
    }

    #[test]
    fn multiple_lines_in_one_buffer() {
        let mut codec = TelnetLineCodec::new();
        let mut buf = BytesMut::from(&b"one\r\ntwo\r\n"[..]);
        assert_eq!(
            codec.decode(&mut buf).unwrap(),
            Some(CodecItem::Line("one".into()))
        );
        assert_eq!(
            codec.decode(&mut buf).unwrap(),
            Some(CodecItem::Line("two".into()))
        );
    }

    #[test]
    fn partial_line_waits() {
        let mut codec = TelnetLineCodec::new();
        let mut buf = BytesMut::from(&b"hel"[..]);
        assert_eq!(codec.decode(&mut buf).unwrap(), None);
        buf.extend_from_slice(b"lo\r\n");
        assert_eq!(
            codec.decode(&mut buf).unwrap(),
            Some(CodecItem::Line("hello".into()))
        );
    }

    // --- IAC stripping ---

    #[test]
    fn iac_iac_emits_literal_0xff() {
        let mut codec = TelnetLineCodec::new();
        let mut buf = BytesMut::from(&b"\xFF\xFF\n"[..]);
        let result = codec.decode(&mut buf).unwrap().unwrap();
        if let CodecItem::Line(s) = result {
            assert!(s.contains('\u{FFFD}'));
        } else {
            panic!("expected Line");
        }
    }

    #[test]
    fn iac_will_discarded() {
        let mut codec = TelnetLineCodec::new();
        assert_eq!(
            decode_line(&mut codec, b"\xFF\xFB\x01hello\r\n"),
            Some("hello".into())
        );
    }

    #[test]
    fn iac_wont_discarded() {
        let mut codec = TelnetLineCodec::new();
        assert_eq!(
            decode_line(&mut codec, b"\xFF\xFC\x01hello\r\n"),
            Some("hello".into())
        );
    }

    #[test]
    fn iac_do_discarded() {
        let mut codec = TelnetLineCodec::new();
        assert_eq!(
            decode_line(&mut codec, b"\xFF\xFD\x01hello\r\n"),
            Some("hello".into())
        );
    }

    #[test]
    fn iac_dont_discarded() {
        let mut codec = TelnetLineCodec::new();
        assert_eq!(
            decode_line(&mut codec, b"\xFF\xFE\x01hello\r\n"),
            Some("hello".into())
        );
    }

    #[test]
    fn iac_two_byte_command_discarded() {
        let mut codec = TelnetLineCodec::new();
        assert_eq!(
            decode_line(&mut codec, b"\xFF\xF4hello\r\n"),
            Some("hello".into())
        );
    }

    #[test]
    fn iac_subnegotiation_discarded() {
        let mut codec = TelnetLineCodec::new();
        assert_eq!(
            decode_line(&mut codec, b"\xFF\xFA\x1F\x00\x50\x00\x28\xFF\xF0hello\r\n"),
            Some("hello".into())
        );
    }

    #[test]
    fn iac_subnegotiation_across_calls() {
        let mut codec = TelnetLineCodec::new();
        let mut buf = BytesMut::from(&b"\xFF\xFA\x1F\x00"[..]);
        assert_eq!(codec.decode(&mut buf).unwrap(), None);
        buf.extend_from_slice(b"\x50\x00\x28\xFF\xF0hello\r\n");
        assert_eq!(
            codec.decode(&mut buf).unwrap(),
            Some(CodecItem::Line("hello".into()))
        );
    }

    #[test]
    fn iac_at_end_of_buffer_waits() {
        let mut codec = TelnetLineCodec::new();
        let mut buf = BytesMut::from(&b"hello\xFF"[..]);
        assert_eq!(codec.decode(&mut buf).unwrap(), None);
        buf.extend_from_slice(b"\xFB\x01\r\n");
        assert_eq!(
            codec.decode(&mut buf).unwrap(),
            Some(CodecItem::Line("hello".into()))
        );
    }

    #[test]
    fn iac_incomplete_3byte_at_end() {
        let mut codec = TelnetLineCodec::new();
        let mut buf = BytesMut::from(&b"\xFF\xFB"[..]);
        assert_eq!(codec.decode(&mut buf).unwrap(), None);
        buf.extend_from_slice(b"\x01hello\r\n");
        assert_eq!(
            codec.decode(&mut buf).unwrap(),
            Some(CodecItem::Line("hello".into()))
        );
    }

    #[test]
    fn iac_iac_inside_subnegotiation() {
        let mut codec = TelnetLineCodec::new();
        assert_eq!(
            decode_line(&mut codec, b"\xFF\xFA\x00\xFF\xFF\xFF\xF0hi\r\n"),
            Some("hi".into())
        );
    }

    #[test]
    fn multiple_iac_sequences_in_one_line() {
        let mut codec = TelnetLineCodec::new();
        assert_eq!(
            decode_line(&mut codec, b"\xFF\xFB\x01hel\xFF\xFD\x03lo\r\n"),
            Some("hello".into())
        );
    }

    // --- Overflow ---

    #[test]
    fn line_too_long_triggers_discard_then_recovery() {
        let mut codec = TelnetLineCodec::with_limits(10, 1024);
        let mut buf = BytesMut::from(&vec![b'a'; 16][..]);

        let result = codec.decode(&mut buf).unwrap();
        assert_eq!(result, Some(CodecItem::LineTooLong));
        assert!(buf.is_empty());

        buf.extend_from_slice(b"more_garbage\r\n");
        assert_eq!(codec.decode(&mut buf).unwrap(), None);

        buf.extend_from_slice(b"ok\r\n");
        assert_eq!(
            codec.decode(&mut buf).unwrap(),
            Some(CodecItem::Line("ok".into()))
        );
    }

    #[test]
    fn buffer_overflow_disconnects() {
        let mut codec = TelnetLineCodec::with_limits(4096, 100);
        let mut buf = BytesMut::from(&vec![b'x'; 101][..]);

        let err = codec.decode(&mut buf).unwrap_err();
        assert!(matches!(err, TelnetCodecError::BufferOverflow { max: 100 }));
    }

    // --- UTF-8 ---

    #[test]
    fn valid_utf8_passes_through() {
        let mut codec = TelnetLineCodec::new();
        assert_eq!(
            decode_line(&mut codec, b"hello world\r\n"),
            Some("hello world".into())
        );
    }

    #[test]
    fn invalid_utf8_replaced() {
        let mut codec = TelnetLineCodec::new();
        let result = decode_line(&mut codec, b"\x80\x81hello\r\n").unwrap();
        assert!(result.contains('\u{FFFD}'));
        assert!(result.contains("hello"));
    }

    // --- Edge cases ---

    #[test]
    fn empty_buffer_returns_none() {
        let mut codec = TelnetLineCodec::new();
        let mut buf = BytesMut::new();
        assert_eq!(codec.decode(&mut buf).unwrap(), None);
    }

    #[test]
    fn decode_eof_with_trailing_data() {
        let mut codec = TelnetLineCodec::new();
        let mut buf = BytesMut::from(&b"hello"[..]);
        assert_eq!(
            codec.decode_eof(&mut buf).unwrap(),
            Some(CodecItem::Line("hello".into()))
        );
    }

    #[test]
    fn decode_eof_with_empty_buffer() {
        let mut codec = TelnetLineCodec::new();
        let mut buf = BytesMut::new();
        assert_eq!(codec.decode_eof(&mut buf).unwrap(), None);
    }

    #[test]
    fn decode_eof_while_discarding() {
        let mut codec = TelnetLineCodec::with_limits(10, 1024);
        let mut buf = BytesMut::from(&vec![b'a'; 16][..]);
        let _ = codec.decode(&mut buf);
        buf.extend_from_slice(b"trailing");
        assert_eq!(codec.decode_eof(&mut buf).unwrap(), None);
    }

    #[test]
    fn decode_eof_with_incomplete_iac() {
        let mut codec = TelnetLineCodec::new();
        let mut buf = BytesMut::from(&b"\xFF"[..]);
        assert_eq!(codec.decode_eof(&mut buf).unwrap(), None);
    }

    #[test]
    fn decode_eof_strips_trailing_lone_iac() {
        let mut codec = TelnetLineCodec::new();
        let mut buf = BytesMut::from(&b"hello\xFF"[..]);
        assert_eq!(
            codec.decode_eof(&mut buf).unwrap(),
            Some(CodecItem::Line("hello".into()))
        );
    }

    #[test]
    fn decode_eof_strips_trailing_iac_will() {
        let mut codec = TelnetLineCodec::new();
        let mut buf = BytesMut::from(&b"hello\xFF\xFB"[..]);
        assert_eq!(
            codec.decode_eof(&mut buf).unwrap(),
            Some(CodecItem::Line("hello".into()))
        );
    }

    #[test]
    fn byte_at_a_time_feeding() {
        let mut codec = TelnetLineCodec::new();
        let input = b"hello\n";
        let mut buf = BytesMut::new();

        for (i, &byte) in input.iter().enumerate() {
            buf.extend_from_slice(&[byte]);
            let result = codec.decode(&mut buf).unwrap();
            if i < input.len() - 1 {
                assert!(result.is_none(), "unexpected result at byte {i}");
            } else {
                assert_eq!(result, Some(CodecItem::Line("hello".into())));
            }
        }
    }
}
