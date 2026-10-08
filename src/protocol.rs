use std::fmt;

/// Requests accepted by the network server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Request {
    Put { key: Vec<u8>, value: Vec<u8> },
    Get { key: Vec<u8> },
    Delete { key: Vec<u8> },
}

/// Responses returned by the network server.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Response {
    Ok,
    Value(Option<Vec<u8>>),
    Error(String),
}

/// Errors returned while parsing client commands.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolError(String);

impl ProtocolError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProtocolError {}

/// Parse one newline-delimited request.
///
/// Commands use hex for keys and values so the wire format can carry arbitrary
/// bytes while remaining easy to type and inspect:
///
/// ```text
/// PUT <hex-key> <hex-value>
/// GET <hex-key>
/// DELETE <hex-key>
/// ```
pub fn parse_request(line: &str) -> Result<Request, ProtocolError> {
    let line = line.trim_end_matches(['\r', '\n']);
    let Some((command, rest)) = line.split_once(' ') else {
        return Err(ProtocolError::new("missing command arguments"));
    };
    let command = command.to_ascii_uppercase();

    match command.as_str() {
        "PUT" => {
            let Some((key, value)) = rest.split_once(' ') else {
                return Err(ProtocolError::new("missing value"));
            };
            let key = decode_hex(key)?;
            let value = decode_hex(value)?;
            Ok(Request::Put { key, value })
        }
        "GET" => {
            reject_spaces(rest)?;
            let key = decode_hex(rest)?;
            Ok(Request::Get { key })
        }
        "DELETE" => {
            reject_spaces(rest)?;
            let key = decode_hex(rest)?;
            Ok(Request::Delete { key })
        }
        _ => Err(ProtocolError::new(format!("unknown command: {command}"))),
    }
}

/// Format one response as a newline-delimited wire message.
pub fn format_response(response: &Response) -> String {
    match response {
        Response::Ok => "OK\n".to_owned(),
        Response::Value(Some(value)) => format!("VALUE {}\n", encode_hex(value)),
        Response::Value(None) => "NOT_FOUND\n".to_owned(),
        Response::Error(message) => format!("ERROR {}\n", message.replace('\n', " ")),
    }
}

/// Parse one response line from the server.
pub fn parse_response(line: &str) -> Result<Response, ProtocolError> {
    let line = line.trim_end_matches(['\r', '\n']);
    if line == "OK" {
        return Ok(Response::Ok);
    }
    if line == "NOT_FOUND" {
        return Ok(Response::Value(None));
    }
    if let Some(value) = line.strip_prefix("VALUE ") {
        return Ok(Response::Value(Some(decode_hex(value)?)));
    }
    if let Some(message) = line.strip_prefix("ERROR ") {
        return Ok(Response::Error(message.to_owned()));
    }
    Err(ProtocolError::new("invalid response"))
}

/// Encode bytes as lowercase hexadecimal.
pub fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn decode_hex(value: &str) -> Result<Vec<u8>, ProtocolError> {
    if value.len() % 2 != 0 {
        return Err(ProtocolError::new("hex value has odd length"));
    }

    let mut bytes = Vec::with_capacity(value.len() / 2);
    for pair in value.as_bytes().chunks_exact(2) {
        let high = hex_digit(pair[0])?;
        let low = hex_digit(pair[1])?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

fn hex_digit(byte: u8) -> Result<u8, ProtocolError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(ProtocolError::new("invalid hex digit")),
    }
}

fn reject_spaces(value: &str) -> Result<(), ProtocolError> {
    if value.contains(' ') {
        return Err(ProtocolError::new("too many fields"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_request_round_trips() {
        let request = parse_request("PUT 6b6579 76616c7565\n").unwrap();
        assert_eq!(
            request,
            Request::Put {
                key: b"key".to_vec(),
                value: b"value".to_vec()
            }
        );
    }

    #[test]
    fn response_round_trips() {
        let response = Response::Value(Some(b"value".to_vec()));
        assert_eq!(
            parse_response(&format_response(&response)).unwrap(),
            response
        );
    }

    #[test]
    fn empty_value_is_allowed() {
        let request = parse_request("PUT 6b \n").unwrap();
        assert_eq!(
            request,
            Request::Put {
                key: b"k".to_vec(),
                value: Vec::new()
            }
        );
    }

    #[test]
    fn malformed_hex_is_rejected() {
        assert!(parse_request("GET abc").is_err());
        assert!(parse_request("GET xx").is_err());
    }
}
