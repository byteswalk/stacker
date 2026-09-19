//! Chrome native messaging framing: a 4-byte little-endian length, then UTF-8 JSON.
use std::io::{self, Read, Write};

/// Largest message either side may send (Chrome caps host → extension at 1 MB).
pub const MAX_MESSAGE: usize = 1024 * 1024;

/// Reads one message; `Ok(None)` when the browser closed the pipe between messages.
pub fn read_message(reader: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut header = [0u8; 4];
    let mut got = 0;
    while got < header.len() {
        match reader.read(&mut header[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    let len = u32::from_le_bytes(header) as usize;
    if len > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message too large",
        ));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

pub fn write_message(writer: &mut impl Write, body: &[u8]) -> io::Result<()> {
    if body.len() > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "message too large",
        ));
    }
    writer.write_all(&(body.len() as u32).to_le_bytes())?;
    writer.write_all(body)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn framed(body: &[u8]) -> Vec<u8> {
        let mut out = (body.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn reads_messages_until_the_pipe_closes() {
        let mut input = framed(br#"{"a":1}"#);
        input.extend(framed(b"{}"));
        let mut reader = Cursor::new(input);
        assert_eq!(read_message(&mut reader).unwrap().unwrap(), br#"{"a":1}"#);
        assert_eq!(read_message(&mut reader).unwrap().unwrap(), b"{}");
        assert!(read_message(&mut reader).unwrap().is_none());
    }

    #[test]
    fn a_cut_off_message_is_an_error() {
        let mut half_header = Cursor::new(vec![5u8, 0]);
        assert!(read_message(&mut half_header).is_err());
        let mut short_body = Cursor::new(framed(b"hello")[..6].to_vec());
        assert!(read_message(&mut short_body).is_err());
    }

    #[test]
    fn refuses_oversized_messages_both_ways() {
        let mut huge = Cursor::new(((MAX_MESSAGE + 1) as u32).to_le_bytes().to_vec());
        let err = read_message(&mut huge).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        let mut out = Vec::new();
        assert!(write_message(&mut out, &vec![b'x'; MAX_MESSAGE + 1]).is_err());
        assert!(out.is_empty(), "nothing is written for a refused message");
    }

    #[test]
    fn writes_little_endian_length_then_body() {
        let mut out = Vec::new();
        write_message(&mut out, b"hi").unwrap();
        assert_eq!(out, vec![2, 0, 0, 0, b'h', b'i']);
    }
}
