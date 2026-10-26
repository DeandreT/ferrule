use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpListener},
    thread,
};

fn server(
    payload: &'static [u8],
    completion_ok: bool,
) -> (SocketAddr, thread::JoinHandle<Vec<String>>) {
    server_with_peer(payload, completion_ok, [127, 0, 0, 1])
}

fn server_with_peer(
    payload: &'static [u8],
    completion_ok: bool,
    passive_peer: [u8; 4],
) -> (SocketAddr, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut input = BufReader::new(stream.try_clone().unwrap());
        stream.write_all(b"220 local fixture ready\r\n").unwrap();
        let mut data = None;
        let mut commands = Vec::new();
        for _ in 0..32 {
            let mut line = String::new();
            if input.read_line(&mut line).unwrap() == 0 {
                break;
            }
            assert!(line.len() < 8192);
            let (command, argument) = line
                .trim_end_matches(['\r', '\n'])
                .split_once(' ')
                .unwrap_or((line.trim(), ""));
            // Record command names and the authored RETR path, never credentials.
            commands.push(if command == "RETR" {
                format!("RETR {argument}")
            } else {
                command.into()
            });
            match command {
                "USER" => stream.write_all(b"331 anonymous password\r\n").unwrap(),
                "PASS" => stream.write_all(b"230 logged in\r\n").unwrap(),
                "TYPE" => stream.write_all(b"200 binary\r\n").unwrap(),
                "PASV" | "EPSV" => {
                    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
                    let port = socket.local_addr().unwrap().port();
                    let reply = if command == "PASV" {
                        format!(
                            "227 passive ({},{},{},{},{},{})\r\n",
                            passive_peer[0],
                            passive_peer[1],
                            passive_peer[2],
                            passive_peer[3],
                            port / 256,
                            port % 256
                        )
                    } else {
                        format!("229 passive (|||{port}|)\r\n")
                    };
                    stream.write_all(reply.as_bytes()).unwrap();
                    data = Some(socket);
                }
                "RETR" => {
                    stream.write_all(b"150 opening data\r\n").unwrap();
                    let (mut socket, _) = data.take().unwrap().accept().unwrap();
                    socket
                        .set_write_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    socket.write_all(payload).unwrap();
                    drop(socket);
                    stream
                        .write_all(if completion_ok {
                            b"226 done\r\n"
                        } else {
                            b"426 transfer failed\r\n"
                        })
                        .unwrap();
                }
                "QUIT" => {
                    stream.write_all(b"221 goodbye\r\n").unwrap();
                    break;
                }
                _ => panic!("unexpected fixture command {command}"),
            }
        }
        commands
    });
    (address, worker)
}

#[test]
fn anonymous_binary_read_preserves_identity_and_decodes_the_command_path() {
    let (address, worker) = server(b"abc", true);
    let selected = format!(" ftp://{address}/with%20space.txt ");
    let reader = DirectSourceReader::new(3, Duration::from_secs(2)).unwrap();
    let result = reader.read(&selected).unwrap();
    assert_eq!(result.identity, selected.trim());
    assert_eq!(result.bytes, b"abc");
    let commands = worker.join().unwrap();
    assert!(commands.contains(&"RETR /with space.txt".to_string()));
    assert_eq!(commands.last().map(String::as_str), Some("QUIT"));
}

#[test]
fn byte_limit_and_parser_failures_still_finish_the_transfer() {
    let (address, worker) = server(b"abcd", true);
    let reader = DirectSourceReader::new(3, Duration::from_secs(2)).unwrap();
    assert!(matches!(
        reader.read(&format!("ftp://{address}/large")),
        Err(SourceReadError::Bytes)
    ));
    assert_eq!(
        worker.join().unwrap().last().map(String::as_str),
        Some("QUIT")
    );
    let (address, worker) = server(b"abc", true);
    let result = reader.read_with(&format!("ftp://{address}/parse"), |_, _| {
        Err::<(), _>("authored parse error")
    });
    assert!(matches!(
        result,
        Err(InputParseError::Parse("authored parse error"))
    ));
    assert_eq!(
        worker.join().unwrap().last().map(String::as_str),
        Some("QUIT")
    );
}

#[test]
fn failed_completion_and_command_injection_are_not_success() {
    let (address, worker) = server(b"abc", false);
    let reader = DirectSourceReader::new(3, Duration::from_secs(2)).unwrap();
    assert!(matches!(
        reader.read(&format!("ftp://{address}/failure")),
        Err(SourceReadError::Ftp)
    ));
    assert_eq!(
        worker.join().unwrap().last().map(String::as_str),
        Some("QUIT")
    );
    assert!(matches!(
        reader.read("ftp://127.0.0.1:1/bad%0d%0apath"),
        Err(SourceReadError::Protocol)
    ));
}

#[test]
fn passive_reply_cannot_redirect_to_a_different_host() {
    let (address, worker) = server_with_peer(b"abc", true, [127, 0, 0, 2]);
    let reader = DirectSourceReader::new(3, Duration::from_secs(2)).unwrap();
    assert!(matches!(
        reader.read(&format!("ftp://{address}/file")),
        Err(SourceReadError::Ftp)
    ));
    assert!(
        !worker
            .join()
            .unwrap()
            .iter()
            .any(|command| command.starts_with("RETR"))
    );
}
