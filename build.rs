fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        winres::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .compile()
            .expect("Failed to compile Windows resources");
    }
}
