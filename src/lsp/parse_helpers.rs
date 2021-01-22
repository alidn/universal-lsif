use anyhow::Context;

use crate::{ret_error, Result};
use std::io::BufRead;

const HEADER_CONTENT_LENGTH: &str = "content-length";
const HEADER_CONTENT_TYPE: &str = "content-type";

pub enum LspHeader {
    ContentType,
    ContentLength(usize),
}

/// parse header from the incoming input string
fn parse_header(s: &str) -> Result<LspHeader> {
    let split: Vec<String> = s.splitn(2, ": ").map(|s| s.trim().to_lowercase()).collect();
    if split.len() != 2 {
        ret_error!("Malfored");
    }
    match split[0].as_ref() {
        HEADER_CONTENT_TYPE => Ok(LspHeader::ContentType),
        HEADER_CONTENT_LENGTH => Ok(LspHeader::ContentLength(usize::from_str_radix(
            &split[1], 10,
        )?)),
        _ => ret_error!("Uknown parse error occured"),
    }
}

/// Blocking call to read a message from the provided BufRead
pub fn read_message<T: BufRead>(reader: &mut T) -> Result<String> {
    let mut buffer = String::new();
    let mut content_length: Option<usize> = None;

    loop {
        buffer.clear();
        let _result = reader.read_line(&mut buffer);

        match &buffer {
            s if s.trim().is_empty() => break,
            s => {
                match parse_header(s)? {
                    LspHeader::ContentLength(len) => content_length = Some(len),
                    LspHeader::ContentType => (),
                };
            }
        };
    }

    // let content_length =
    //     content_length.with_context(|| format!("missing content-length header: {}", buffer))?;
    let content_length = content_length.unwrap_or(0);

    let mut body_buffer = vec![0; content_length];
    reader.read_exact(&mut body_buffer)?;

    let body = String::from_utf8(body_buffer)?;
    Ok(body)
}
