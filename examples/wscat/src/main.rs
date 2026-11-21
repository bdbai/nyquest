//! A minimal `wscat` built on the blocking WebSocket API.
//!
//! Usage: `wscat <ws://url>`
//!
//! - Data read from stdin is sent to the server, one message per line.
//! - Messages from the server are written to stdout as-is.
//! - Connection progress and errors are written to stderr.

use std::{
    env,
    io::{self, BufRead as _, Write as _},
    process,
    sync::mpsc,
    thread,
};

use nyquest::{
    blocking::{WebSocket, WebSocketSender},
    ClientBuilder, WebSocketMessage,
};

fn main() {
    nyquest_preset::register();

    let url = env::args()
        .nth(1)
        .expect("Usage: wscat <ws://url> [PROTOCOL1 PROTOCOL2 ...]");
    let protocols = env::args().skip(2).map(From::from).collect::<Vec<_>>();

    let client = ClientBuilder::default()
        .build_blocking()
        .expect("Failed to build client");

    eprintln!("Connecting to {url}...");
    let ws = match client.connect_websocket(url.clone(), protocols) {
        Ok(ws) => ws,
        Err(err) => {
            eprintln!("Failed to connect to {url}: {err}");
            process::exit(1);
        }
    };
    match &ws.info.protocol {
        Some(protocol) => eprintln!("Connected to {url} (subprotocol: {protocol})"),
        None => eprintln!("Connected to {url}"),
    }

    let WebSocket {
        sender, receiver, ..
    } = ws;

    let (upstream_tx, upstream_rx) = mpsc::sync_channel::<WebSocketMessage>(2);

    // Forwards everything from the channel to the server.
    let upstream_thread = thread::spawn(move || upstream_loop(sender, upstream_rx));

    // Reads stdin and feeds the channel.
    let stdin_tx = upstream_tx.clone();
    thread::spawn(move || stdin_loop(stdin_tx));

    // Keeps receiving and writing to stdout until the server closes or fails.
    let mut stdout = io::stdout();
    for message in receiver {
        match message {
            Ok(WebSocketMessage::Utf8(text)) => {
                if let Err(err) = stdout
                    .write_all(text.as_bytes())
                    .and_then(|()| stdout.flush())
                {
                    eprintln!("Failed to write to stdout: {err}");
                    break;
                }
            }
            Ok(WebSocketMessage::Bytes(bytes)) => {
                if let Err(err) = stdout.write_all(&bytes).and_then(|()| stdout.flush()) {
                    eprintln!("Failed to write to stdout: {err}");
                    break;
                }
            }
            Ok(WebSocketMessage::Close { code, reason }) => {
                eprintln!("Connection closed by peer: code {code}, reason {reason:?}");
            }
            Err(err) => eprintln!("Failed to receive message: {err}"),
        }
    }

    // Tell the sending thread to shut down, then wait for it before exiting.
    let _ = upstream_tx.send(WebSocketMessage::Close {
        code: 1000,
        reason: "connection finished".into(),
    });
    drop(upstream_tx);
    if upstream_thread.join().is_err() {
        eprintln!("Upstream thread panicked");
    }
    eprintln!("Disconnected");
}

fn stdin_loop(upstream_tx: mpsc::SyncSender<WebSocketMessage>) {
    let mut stdin = io::stdin().lock();
    loop {
        let mut buf = String::new();
        let read = stdin.read_line(&mut buf);
        match read {
            Ok(0) => break, // EOF
            Ok(_) => {
                if upstream_tx.send(WebSocketMessage::text(buf)).is_err() {
                    // The upstream thread is gone; nothing else to do.
                    return;
                }
                buf = String::new();
            }
            Err(err) => {
                eprintln!("Failed to read stdin: {err}");
                break;
            }
        }
    }

    eprintln!("stdin closed");
    let _ = upstream_tx.send(WebSocketMessage::Close {
        code: 1000,
        reason: "stdin closed".into(),
    });
}

fn upstream_loop(mut sender: WebSocketSender, upstream_rx: mpsc::Receiver<WebSocketMessage>) {
    while let Ok(msg) = upstream_rx.recv() {
        if let WebSocketMessage::Close { code, reason } = &msg {
            eprintln!("Closing connection: code {code}, reason {reason:?}");
            if let Err(err) = sender.close() {
                eprintln!("Failed to close connection: {err}");
            }
            return;
        }
        if let Err(err) = sender.send(msg) {
            eprintln!("Failed to send message: {err}");
            return;
        }
    }
    eprintln!("Upstream channel closed");
}
