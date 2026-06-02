use std::env;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process;
use std::sync::{Arc, Mutex};
use std::thread;

use bitcask_rs::server::protocol::{format_response, parse_request, Request, Response};
use bitcask_rs::Db;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_args(env::args().skip(1))
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    let db = Arc::new(Mutex::new(Db::open(&config.path)?));
    let listener = TcpListener::bind(&config.addr)?;

    eprintln!(
        "bitcask-server listening on {} with data at {}",
        config.addr, config.path
    );

    for stream in listener.incoming() {
        let stream = stream?;
        let db = Arc::clone(&db);
        thread::spawn(move || {
            if let Err(error) = handle_client(stream, db) {
                eprintln!("client error: {error}");
            }
        });
    }

    Ok(())
}

fn handle_client(stream: TcpStream, db: Arc<Mutex<Db>>) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let mut line = String::new();

    while reader.read_line(&mut line)? != 0 {
        let response = match parse_request(&line) {
            Ok(request) => execute(request, &db),
            Err(error) => Response::Error(error.to_string()),
        };
        writer.write_all(format_response(&response).as_bytes())?;
        writer.flush()?;
        line.clear();
    }

    Ok(())
}

fn execute(request: Request, db: &Arc<Mutex<Db>>) -> Response {
    let mut guard = match db.lock() {
        Ok(guard) => guard,
        Err(_) => return Response::Error("database lock is poisoned".to_owned()),
    };

    match request {
        Request::Put { key, value } => guard
            .put(key, value)
            .map(|_| Response::Ok)
            .unwrap_or_else(|error| Response::Error(error.to_string())),
        Request::Get { key } => guard
            .get(key)
            .map(Response::Value)
            .unwrap_or_else(|error| Response::Error(error.to_string())),
        Request::Delete { key } => guard
            .delete(key)
            .map(|_| Response::Ok)
            .unwrap_or_else(|error| Response::Error(error.to_string())),
    }
}

struct Config {
    path: String,
    addr: String,
}

impl Config {
    fn from_args(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut path = "data".to_owned();
        let mut addr = "127.0.0.1:4000".to_owned();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--path" => {
                    path = args.next().ok_or("--path requires a value")?;
                }
                "--addr" => {
                    addr = args.next().ok_or("--addr requires a value")?;
                }
                "--help" | "-h" => {
                    return Err(usage());
                }
                _ => return Err(format!("unknown argument: {arg}\n{}", usage())),
            }
        }

        Ok(Self { path, addr })
    }
}

fn usage() -> String {
    "usage: bitcask-server [--path data-dir] [--addr host:port]".to_owned()
}
