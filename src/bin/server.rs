use std::env;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process;
use std::sync::{Arc, RwLock};
use std::thread;

use bitcask_rs::compaction::{CompactionScheduler, SchedulerOptions};
use bitcask_rs::protocol::{format_response, parse_request, Request, Response};
use bitcask_rs::Db;

/// Longest request line accepted, including the trailing newline.
///
/// Keys and values travel as hex, so this allows roughly 512 KiB of payload
/// per request while stopping one client from exhausting server memory.
const MAX_REQUEST_LINE_LEN: usize = 1024 * 1024;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_args(env::args().skip(1))
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    let db = Arc::new(RwLock::new(Db::open(&config.path)?));
    let listener = TcpListener::bind(&config.addr)?;
    let _scheduler = CompactionScheduler::start(
        Arc::clone(&db),
        SchedulerOptions {
            stale_ratio_threshold: config.compact_threshold,
            ..SchedulerOptions::default()
        },
    );

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

fn handle_client(stream: TcpStream, db: Arc<RwLock<Db>>) -> io::Result<()> {
    let reader = BufReader::new(stream.try_clone()?);
    serve(reader, stream, &db)
}

/// Answer newline-delimited requests until the client disconnects.
///
/// A line longer than [`MAX_REQUEST_LINE_LEN`] gets an error response and
/// ends the connection, since the rest of the stream can no longer be framed.
fn serve(mut reader: impl BufRead, mut writer: impl Write, db: &Arc<RwLock<Db>>) -> io::Result<()> {
    let mut line = Vec::new();

    loop {
        line.clear();
        let limit = MAX_REQUEST_LINE_LEN as u64 + 1;
        if reader.by_ref().take(limit).read_until(b'\n', &mut line)? == 0 {
            return Ok(());
        }

        if line.len() > MAX_REQUEST_LINE_LEN {
            let response = Response::Error("request too large".to_owned());
            writer.write_all(format_response(&response).as_bytes())?;
            writer.flush()?;
            return Ok(());
        }

        let response = match std::str::from_utf8(&line) {
            Ok(text) => match parse_request(text) {
                Ok(request) => execute(request, db),
                Err(error) => Response::Error(error.to_string()),
            },
            Err(_) => Response::Error("request is not valid UTF-8".to_owned()),
        };
        writer.write_all(format_response(&response).as_bytes())?;
        writer.flush()?;
    }
}

/// Run one request. `GET` shares the read lock so reads from different
/// clients run concurrently; `PUT` and `DELETE` take the write lock.
fn execute(request: Request, db: &Arc<RwLock<Db>>) -> Response {
    const POISONED: &str = "database lock is poisoned";

    let result = match request {
        Request::Get { key } => match db.read() {
            Ok(guard) => guard.get(key).map(Response::Value),
            Err(_) => return Response::Error(POISONED.to_owned()),
        },
        Request::Put { key, value } => match db.write() {
            Ok(mut guard) => guard.put(key, value).map(|_| Response::Ok),
            Err(_) => return Response::Error(POISONED.to_owned()),
        },
        Request::Delete { key } => match db.write() {
            Ok(mut guard) => guard.delete(key).map(|_| Response::Ok),
            Err(_) => return Response::Error(POISONED.to_owned()),
        },
    };

    match result {
        Ok(response) => response,
        Err(error) => Response::Error(error.to_string()),
    }
}

struct Config {
    path: String,
    addr: String,
    compact_threshold: f64,
}

impl Config {
    fn from_args(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut path = "data".to_owned();
        let mut addr = "127.0.0.1:4000".to_owned();
        let mut compact_threshold = 0.3;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--path" => {
                    path = args.next().ok_or("--path requires a value")?;
                }
                "--addr" => {
                    addr = args.next().ok_or("--addr requires a value")?;
                }
                "--compact-threshold" => {
                    let value = args.next().ok_or("--compact-threshold requires a value")?;
                    compact_threshold = parse_threshold(&value)?;
                }
                "--help" | "-h" => {
                    return Err(usage());
                }
                _ => return Err(format!("unknown argument: {arg}\n{}", usage())),
            }
        }

        Ok(Self {
            path,
            addr,
            compact_threshold,
        })
    }
}

/// Parse a stale-record ratio in the range `0.0..=1.0`.
fn parse_threshold(value: &str) -> Result<f64, String> {
    match value.parse::<f64>() {
        Ok(threshold) if (0.0..=1.0).contains(&threshold) => Ok(threshold),
        _ => Err(format!(
            "--compact-threshold must be a number between 0.0 and 1.0, got {value}"
        )),
    }
}

fn usage() -> String {
    "usage: bitcask-server [--path data-dir] [--addr host:port] [--compact-threshold 0.0-1.0]"
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db(name: &str) -> Arc<RwLock<Db>> {
        let path = format!("target/test-data/server-{name}");
        let _ = std::fs::remove_dir_all(&path);
        Arc::new(RwLock::new(Db::open(&path).expect("open database")))
    }

    fn serve_input(db: &Arc<RwLock<Db>>, input: &[u8]) -> String {
        let mut output = Vec::new();
        serve(input, &mut output, db).expect("serve");
        String::from_utf8(output).expect("utf-8 response")
    }

    #[test]
    fn serves_requests_line_by_line() {
        let db = test_db("lines");
        let output = serve_input(&db, b"PUT 6b 76\nGET 6b\nDELETE 6b\nGET 6b\n");
        assert_eq!(output, "OK\nVALUE 76\nOK\nNOT_FOUND\n");
    }

    #[test]
    fn oversized_request_line_is_rejected_and_closes_the_connection() {
        let db = test_db("oversized");
        let mut input = b"PUT 6b ".to_vec();
        input.resize(MAX_REQUEST_LINE_LEN + 1, b'a');
        input.extend_from_slice(b"\nGET 6b\n");

        let output = serve_input(&db, &input);

        assert_eq!(output, "ERROR request too large\n");
        assert!(db.read().unwrap().is_empty());
    }

    #[test]
    fn request_line_at_the_limit_is_accepted() {
        let db = test_db("at-limit");
        let mut input = b"PUT 6b ".to_vec();
        input.resize(MAX_REQUEST_LINE_LEN - 1, b'a');
        input.push(b'\n');

        assert_eq!(serve_input(&db, &input), "OK\n");
    }

    #[test]
    fn get_runs_while_another_reader_holds_the_lock() {
        let db = test_db("shared-read");
        serve_input(&db, b"PUT 6b 76\n");
        let _reader = db.read().unwrap();

        let (sender, receiver) = std::sync::mpsc::channel();
        let db_for_client = Arc::clone(&db);
        std::thread::spawn(move || {
            sender
                .send(serve_input(&db_for_client, b"GET 6b\n"))
                .unwrap();
        });

        let output = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("GET should not wait for other readers");
        assert_eq!(output, "VALUE 76\n");
    }

    #[test]
    fn invalid_utf8_gets_an_error_without_closing() {
        let db = test_db("utf8");
        let output = serve_input(&db, b"GET \xff\nGET 6b\n");
        assert_eq!(output, "ERROR request is not valid UTF-8\nNOT_FOUND\n");
    }

    fn parse(args: &[&str]) -> Result<Config, String> {
        Config::from_args(args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn compact_threshold_defaults_to_point_three() {
        assert_eq!(parse(&[]).unwrap().compact_threshold, 0.3);
    }

    #[test]
    fn compact_threshold_flag_is_parsed() {
        let config = parse(&["--compact-threshold", "0.5"]).unwrap();
        assert_eq!(config.compact_threshold, 0.5);
    }

    #[test]
    fn compact_threshold_outside_unit_range_is_rejected() {
        assert!(parse(&["--compact-threshold", "1.5"]).is_err());
        assert!(parse(&["--compact-threshold", "-0.1"]).is_err());
        assert!(parse(&["--compact-threshold", "NaN"]).is_err());
        assert!(parse(&["--compact-threshold"]).is_err());
    }
}
