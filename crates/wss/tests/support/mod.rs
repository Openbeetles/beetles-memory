use bm_sdk::ProfileId;
use bm_wss::WssLocalSessionIdentity;

#[cfg(feature = "server-std")]
use std::io::{Read, Write};
#[cfg(feature = "server-std")]
use std::net::{TcpListener, TcpStream};

#[cfg(feature = "server-std")]
use bm_entry::{EntryAcceptedTcpStream, EntryRuntime};

#[allow(dead_code)]
pub fn trusted_auth(principal: &str) -> WssLocalSessionIdentity {
    WssLocalSessionIdentity::in_process(principal)
}

pub fn native_runtime_profile() -> ProfileId {
    #[cfg(feature = "nonproduction-replay-harness")]
    {
        ProfileId::native_dev_full().expect("native dev-full profile")
    }
    #[cfg(all(not(feature = "nonproduction-replay-harness"), target_os = "macos"))]
    {
        ProfileId::DesktopMacosEmbeddedSdk
    }
    #[cfg(all(not(feature = "nonproduction-replay-harness"), target_os = "windows"))]
    {
        ProfileId::DesktopWindowsEmbeddedSdk
    }
    #[cfg(all(not(feature = "nonproduction-replay-harness"), target_os = "linux"))]
    {
        ProfileId::ServerLinuxMemoryGateway
    }
}

#[cfg(feature = "server-std")]
#[allow(dead_code)]
pub fn serve_network_frame(
    runtime: EntryRuntime,
    authorization: Option<&str>,
    frame: &str,
) -> (bm_sdk::Result<()>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind WSS test listener");
    let mut client = TcpStream::connect(listener.local_addr().expect("WSS test address"))
        .expect("connect WSS test listener");
    let mut accepted = EntryAcceptedTcpStream::accept(&listener).expect("accept WSS test peer");
    let authorization = authorization
        .map(|value| format!("Authorization: {value}\r\n"))
        .unwrap_or_default();
    let frame = frame.to_string();
    let client_thread = std::thread::spawn(move || {
        let handshake = format!(
            "GET /memory/ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n{authorization}\r\n"
        );
        client
            .write_all(handshake.as_bytes())
            .expect("write WSS handshake");
        let mut header = Vec::new();
        read_until(&mut client, b"\r\n\r\n", &mut header);
        let header = String::from_utf8(header).expect("WSS handshake UTF-8");
        if !header.starts_with("HTTP/1.1 101 Switching Protocols") {
            return header;
        }
        write_masked_text_frame(&mut client, &frame);
        let response = read_unmasked_text_frame(&mut client);
        if !response.is_empty() {
            write_masked_control_frame(&mut client, 0x08, &[]);
            read_unmasked_control_frame(&mut client, 0x08);
        }
        format!("{header}{response}")
    });
    let result = bm_wss::serve_wss_accepted_stream(&runtime, &mut accepted, "wss-network-test");
    drop(accepted);
    (result, client_thread.join().expect("WSS test client"))
}

#[cfg(feature = "server-std")]
#[allow(dead_code)]
pub fn serve_network_sequence(
    runtime: EntryRuntime,
    authorization: &str,
    frames: &[&str],
) -> (bm_sdk::Result<()>, Vec<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind WSS sequence listener");
    let mut client = TcpStream::connect(listener.local_addr().expect("WSS sequence address"))
        .expect("connect WSS sequence listener");
    let mut accepted = EntryAcceptedTcpStream::accept(&listener).expect("accept WSS sequence peer");
    let authorization = authorization.to_string();
    let frames = frames
        .iter()
        .map(|frame| frame.to_string())
        .collect::<Vec<_>>();
    let client_thread = std::thread::spawn(move || {
        let handshake = format!(
            "GET /memory/ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nAuthorization: {authorization}\r\n\r\n"
        );
        client
            .write_all(handshake.as_bytes())
            .expect("write WSS sequence handshake");
        let mut header = Vec::new();
        read_until(&mut client, b"\r\n\r\n", &mut header);
        assert!(String::from_utf8(header)
            .expect("WSS sequence handshake UTF-8")
            .starts_with("HTTP/1.1 101 Switching Protocols"));
        let mut responses = Vec::new();
        for frame in frames {
            write_masked_text_frame(&mut client, &frame);
            responses.push(read_unmasked_text_frame(&mut client));
        }
        write_masked_control_frame(&mut client, 0x09, b"health");
        assert_eq!(read_unmasked_control_frame(&mut client, 0x0A), b"health");
        write_masked_control_frame(&mut client, 0x08, &[]);
        assert!(read_unmasked_control_frame(&mut client, 0x08).is_empty());
        responses
    });
    let result = bm_wss::serve_wss_accepted_stream(&runtime, &mut accepted, "wss-sequence-test");
    drop(accepted);
    (result, client_thread.join().expect("WSS sequence client"))
}

#[cfg(feature = "server-std")]
#[allow(dead_code)]
fn read_until(stream: &mut TcpStream, needle: &[u8], out: &mut Vec<u8>) {
    let mut byte = [0_u8; 1];
    while !out.ends_with(needle) {
        let read = stream.read(&mut byte).expect("read WSS handshake");
        if read == 0 {
            break;
        }
        out.push(byte[0]);
    }
}

#[cfg(feature = "server-std")]
#[allow(dead_code)]
fn write_masked_text_frame(stream: &mut TcpStream, text: &str) {
    let payload = text.as_bytes();
    let mask = [1_u8, 2, 3, 4];
    let mut frame = vec![0x81];
    if payload.len() < 126 {
        frame.push(0x80 | payload.len() as u8);
    } else {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    }
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % 4]),
    );
    stream.write_all(&frame).expect("write WSS frame");
}

#[cfg(feature = "server-std")]
fn write_masked_control_frame(stream: &mut TcpStream, opcode: u8, payload: &[u8]) {
    assert!(payload.len() <= 125);
    let mask = [5_u8, 6, 7, 8];
    let mut frame = vec![0x80 | opcode, 0x80 | payload.len() as u8];
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % 4]),
    );
    stream.write_all(&frame).expect("write WSS control frame");
}

#[cfg(feature = "server-std")]
fn read_unmasked_control_frame(stream: &mut TcpStream, opcode: u8) -> Vec<u8> {
    let mut header = [0_u8; 2];
    stream
        .read_exact(&mut header)
        .expect("read WSS control frame header");
    assert_eq!(header[0], 0x80 | opcode);
    assert_eq!(header[1] & 0x80, 0);
    let len = usize::from(header[1] & 0x7f);
    assert!(len <= 125);
    let mut payload = vec![0_u8; len];
    stream
        .read_exact(&mut payload)
        .expect("read WSS control frame body");
    payload
}

#[cfg(feature = "server-std")]
#[allow(dead_code)]
fn read_unmasked_text_frame(stream: &mut TcpStream) -> String {
    let mut header = [0_u8; 2];
    match stream.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return String::new(),
        Err(error) => panic!("read WSS frame header: {error}"),
    }
    assert_eq!(header[0], 0x81);
    assert_eq!(header[1] & 0x80, 0);
    let len = match header[1] & 0x7f {
        126 => {
            let mut extended = [0_u8; 2];
            stream
                .read_exact(&mut extended)
                .expect("read WSS extended frame length");
            usize::from(u16::from_be_bytes(extended))
        }
        len => usize::from(len),
    };
    let mut payload = vec![0_u8; len];
    stream
        .read_exact(&mut payload)
        .expect("read WSS frame body");
    String::from_utf8(payload).expect("WSS frame UTF-8")
}
