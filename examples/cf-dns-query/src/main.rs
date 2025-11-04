use std::borrow::Cow;

use futures::future::join;
use nyquest::{AsyncClient, Request};

fn main() {
    nyquest_preset::register();

    futures::executor::block_on(async {
        async_main().await.unwrap();
    });
}

async fn async_main() -> nyquest::Result<()> {
    let client = nyquest::ClientBuilder::default()
        .base_url("https://cloudflare-dns.com/")
        .user_agent("curl/7.68.0 nyquest/0")
        .with_header("accept", "application/dns-json")
        .build_async()
        .await
        .expect("Failed to build client");

    let (res1, res2) = join(
        query_address(&client, "example.com"),
        query_address(&client, "cloudflare.com"),
    )
    .await;
    println!("IP address of example.com: {}", res1?);
    println!("IP address of cloudflare.com: {}", res2?);

    Ok(())
}

async fn query_address(client: &AsyncClient, domain_name: &str) -> nyquest::Result<String> {
    eprintln!("Querying IP address of {domain_name}");
    let res = client
        .request(Request::get(format!("dns-query?name={domain_name}&type=A")))
        .await?
        .text()
        .await?;
    eprintln!("Finished querying IP address of {domain_name}");

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Response<'a> {
        answer: Vec<Answer<'a>>,
    }
    #[derive(serde::Deserialize)]
    struct Answer<'a> {
        data: Cow<'a, str>,
    }
    let Response { answer } = match serde_json::from_str::<Response>(&res) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to parse response: {e}");
            return Ok(res);
        }
    };
    let first_answer = answer
        .into_iter()
        .next()
        .map(|a| a.data.into_owned())
        .unwrap_or_default();
    Ok(first_answer)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_connectivity() {
        // Since `auto-register` feature is enabled, the backend should be registered automatically.

        let client = nyquest::ClientBuilder::default()
            .build_blocking()
            .expect("Failed to build client");
        let res = client
            .request(nyquest::Request::get(
                "http://cp.cloudflare.com/generate_204",
            ))
            .expect("Failed to build request");
        res.with_successful_status().expect("Request failed");
    }
}
