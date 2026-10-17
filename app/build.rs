//! Windows: embeds the icon and the version info into listen.exe.

fn main() {
    println!("cargo::rerun-if-changed=../packaging/windows/listen.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../packaging/windows/listen.ico");
        res.set("ProductName", "Listen");
        res.set("FileDescription", "Listen");
        res.set("InternalName", "listen");
        res.set("OriginalFilename", "listen.exe");
        res.set("CompanyName", "Listen contributors");
        res.set("LegalCopyright", "Copyright (c) Listen contributors. AGPL-3.0-or-later");
        if let Err(e) = res.compile() {
            println!("cargo::warning=could not embed the Windows resources: {e}");
        }
    }
}
