fn main() {
    #[cfg(windows)]
    nyquest_backend_winrt::register();

    let client = nyquest::ClientBuilder::default().build_async();
    futures::executor::block_on(async move {
        let response = client.get("https://wttr.in").send().await.unwrap();
        println!("{}", response.text().await.unwrap());
    });
}
