use nyquest::Request;

fn main() {
    nyquest_preset_rich::register();

    let client = nyquest::ClientBuilder::default()
        .user_agent("curl/7.68.0 nyquest/0")
        .build_blocking()
        .expect("Failed to build client");
    let text = client
        .request(Request::get("https://wttr.in/nrt"))
        .expect("Failed to get response")
        .text()
        .expect("Failed to get text");
    println!("{text}");
}
