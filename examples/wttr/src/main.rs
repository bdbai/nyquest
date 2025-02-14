use nyquest::blocking::Request;

fn main() {
    nyquest_preset_rich::register();

    let client = nyquest::ClientBuilder::default()
        .user_agent("curl/7.68.0 nyquest/0")
        .build_blocking()
        .expect("Failed to build client");
    let response = client
        .request(Request::get("https://wttr.in/nrt"))
        .expect("Failed to get response");
    let status = response.status();
    let text = response.text().expect("Failed to get text");
    if status != 200 {
        panic!("wttr.in returned non-success response {status}: \n{text}");
    }
    println!("{text}");
}
