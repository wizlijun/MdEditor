fn main() {
    // Embed Info.plist into the binary's __TEXT,__info_plist section so a bare
    // (non-.app) executable carries its location usage descriptions for TCC —
    // exactly how CoreLocationCLI does it.
    let plist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Info.plist");
    println!("cargo:rerun-if-changed=Info.plist");
    println!("cargo:rustc-link-arg=-Wl,-sectcreate,__TEXT,__info_plist,{}", plist.display());
}
