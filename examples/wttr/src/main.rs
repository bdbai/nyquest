fn main() {
    #[cfg(windows)]
    nyquest_backend_winrt::register();

    let text = nyquest::ClientBuilder::default()
        .build_blocking()
        .expect("Failed to build client")
        .get_string("https://wttr.in")
        .expect("Failed to get response");
    println!("{text}");
}
