fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rerun-if-changed=src/aep/adobe_host.m");
        cc::Build::new()
            .file("src/aep/adobe_host.m")
            .flag("-fobjc-arc")
            .compile("neoflash_adobe_host");
        println!("cargo:rustc-link-lib=framework=Cocoa");
        println!("cargo:rustc-link-lib=framework=ApplicationServices");
    }
}
