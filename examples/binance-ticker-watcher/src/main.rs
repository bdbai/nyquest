//! Watch average prices from Binance over the async WebSocket API.
//!
//! Usage: `binance-ticker-watcher <SYMBOL> [SYMBOL ...]`
//!
//! One connection is opened per symbol to
//! `wss://stream.binance.com:9443/ws/<SYMBOL>@avgPrice`.
//! Connection progress and errors are written to stderr, while each update is
//! written to stdout as `<SYMBOL>: <AVERAGE PRICE>`.

use std::{borrow::Cow, env};

use futures::{future::join_all, StreamExt as _};
use nyquest::{AsyncClient, WebSocketMessage};
use serde::Deserialize;

fn main() {
    nyquest_preset::register();

    futures::executor::block_on(async_main());
}

async fn async_main() {
    let symbols = env::args().skip(1).collect::<Vec<_>>();
    if symbols.is_empty() {
        eprintln!("Usage: binance-ticker-watcher <SYMBOL> [SYMBOL ...]");
        std::process::exit(1);
    }

    let client = nyquest::ClientBuilder::default()
        .build_async()
        .await
        .expect("Failed to build client");

    join_all(
        symbols
            .into_iter()
            .map(|symbol| watch_symbol(&client, symbol)),
    )
    .await;
}

async fn watch_symbol(client: &AsyncClient, symbol: String) {
    let url = format!(
        "wss://stream.binance.com:9443/ws/{}@avgPrice",
        symbol.to_lowercase()
    );
    eprintln!("Connecting to {url}...");
    let ws = match client
        .connect_websocket(url.clone(), Vec::<Cow<'static, str>>::new())
        .await
    {
        Ok(ws) => ws,
        Err(err) => {
            eprintln!("Failed to connect to {url}: {err}");
            return;
        }
    };
    eprintln!("Connected to {url}");

    let mut receiver = ws.receiver;
    while let Some(message) = receiver.next().await {
        match message {
            Ok(WebSocketMessage::Utf8(text)) => match parse_avg_price(&text) {
                Ok((symbol, price)) => println!("{symbol}: {price:.2}"),
                Err(err) => eprintln!("Failed to parse message {text:?}: {err}"),
            },
            Ok(_) => {}
            Err(err) => eprintln!("Failed to receive message: {err}"),
        }
    }
    eprintln!("Connection to {url} finished");
}

#[derive(Deserialize)]
struct AvgPrice<'a> {
    /// The symbol the price belongs to.
    s: Cow<'a, str>,
    /// The average price over the interval.
    w: Cow<'a, str>,
}

fn parse_avg_price(text: &str) -> Result<(Cow<'_, str>, f64), String> {
    let AvgPrice { s, w } = serde_json::from_str(text).map_err(|err| err.to_string())?;
    let price = w
        .parse::<f64>()
        .map_err(|err| format!("invalid average price {w:?}: {err}"))?;
    Ok((s, price))
}
