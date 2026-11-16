//! On Windows, embed the app icon and version info in the .exe.

fn main() {
    // Windows gives the main thread a 1 MB stack (Linux: 8 MB). Match Linux so a deep
    // frame can't overflow it there only; an overflow kills the app without a word.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }
    println!("cargo:rerun-if-changed=assets/icon/app.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/app.ico")
            .set("ProductName", "Drop-in Badminton")
            .set("FileDescription", "Drop-in Badminton");
        if let Err(err) = res.compile() {
            println!("cargo:warning=couldn't embed the app icon: {err}");
        }
    }
}
