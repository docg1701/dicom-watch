fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let result = winres::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .compile();

        if let Err(e) = result {
            eprintln!(
                "build.rs: failed to embed icon ({}). \
                 Install mingw-w64 or build on native Windows for .exe icon.",
                e
            );
        }
    }
}
