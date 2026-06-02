use std::env;
use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpStream;
use std::process;

use bitcask_rs::server::protocol::{encode_hex, parse_response, Request, Response};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let command = Command::from_args(env::args().skip(1))
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    let response = send_request(&command.addr, command.request)?;

    match response {
        Response::Ok => println!("OK"),
        Response::Value(Some(value)) => println!("{}", String::from_utf8_lossy(&value)),
        Response::Value(None) => println!("NOT_FOUND"),
        Response::Error(message) => {
            return Err(io::Error::new(io::ErrorKind::Other, message).into());
        }
    }

    Ok(())
}

fn send_request(addr: &str, request: Request) -> Result<Response, Box<dyn std::error::Error>> {
    let mut stream = TcpStream::connect(addr)?;
    let wire_request = match request {
        Request::Put { key, value } => format!(
            "PUT {} {}\n",
            encode_hex(&key),
            encode_hex(&value)
        ),
        Request::Get { key } => format!("GET {}\n", encode_hex(&key)),
        Request::Delete { key } => format!("DELETE {}\n", encode_hex(&key)),
    };

    stream.write_all(wire_request.as_bytes())?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(parse_response(&line)?)
}

struct Command {
    addr: String,
    request: Request,
}

impl Command {
    fn from_args(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut addr = "127.0.0.1:4000".to_owned();
        let mut rest = Vec::new();

        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--addr" => {
                    addr = args.next().ok_or("--addr requires a value")?;
                }
                "--help" | "-h" => return Err(usage()),
                _ => {
                    rest.push(arg);
                    rest.extend(args);
                    break;
                }
            }
        }

        let request = parse_command(rest)?;
        Ok(Self { addr, request })
    }
}

fn parse_command(args: Vec<String>) -> Result<Request, String> {
    match args.as_slice() {
        [command, key, value] if command == "put" => Ok(Request::Put {
            key: key.as_bytes().to_vec(),
            value: value.as_bytes().to_vec(),
        }),
        [command, key] if command == "get" => Ok(Request::Get {
            key: key.as_bytes().to_vec(),
        }),
        [command, key] if command == "delete" => Ok(Request::Delete {
            key: key.as_bytes().to_vec(),
        }),
        _ => Err(usage()),
    }
}

fn usage() -> String {
    "usage: bitcask-cli [--addr host:port] put <key> <value>\n       bitcask-cli [--addr host:port] get <key>\n       bitcask-cli [--addr host:port] delete <key>".to_owned()
}
