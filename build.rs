//! On Windows, embed the app icon and version info in the .exe.

fn main() {
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
