use serde_json::Value;
use std::io::{self, BufRead, Write};

const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn read_message(input: &mut impl BufRead) -> io::Result<Option<Value>> {
    let mut line = String::new();
    let mut length = None;
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return match length {
                None => Ok(None),
                Some(_) => Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "incomplete LSP header",
                )),
            };
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("Content-Length")
        {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let length = length
        .filter(|&length| length <= MAX_MESSAGE_BYTES)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid LSP Content-Length"))?;
    let mut body = vec![0; length];
    input.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(io::Error::other)
}

pub(crate) fn write_message(output: &mut impl Write, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message).map_err(io::Error::other)?;
    write!(output, "Content-Length: {}\r\n\r\n", body.len())?;
    output.write_all(&body)?;
    output.flush()
}
