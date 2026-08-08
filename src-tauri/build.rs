fn main() {
    // libgit2 (via git2 crate with vendored-libgit2) links against zlib and
    // iconv. On macOS host they're auto-discovered via pkg-config; on iOS
    // the linker doesn't find them unless we explicitly link the system
    // dylibs.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios") {
        println!("cargo:rustc-link-lib=z");
        println!("cargo:rustc-link-lib=iconv");
    }
    // Windows: give the *test* binaries a Common-Controls v6 manifest.
    //
    // tauri-build embeds a manifest into the shipped executable, but not into
    // test harnesses. The lib links tao/muda, which import SetWindowSubclass /
    // DefSubclassProc / RemoveWindowSubclass / TaskDialogIndirect — comctl32 v6
    // exports. With no manifest the loader binds the v5 comctl32 from System32
    // and every `cargo test` binary dies at load with STATUS_ENTRYPOINT_NOT_FOUND
    // (0xc0000139) before running a single test.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("windows-tests.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        // Scope note: `rustc-link-arg-tests` covers only `tests/*.rs`
        // integration targets. The `--lib` unit-test harness — where nearly all
        // of the suite lives — is the lib target rebuilt with `--test` and does
        // NOT receive it (verified: its PE had no resource directory at all).
        // So the flag goes on `rustc-link-arg`, which reaches every artifact…
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        // …and is then cancelled for `bins`, because tauri-build already links
        // a `resource.lib` carrying RT_MANIFEST id 1 there. Two manifests is a
        // hard error, not a merge: `CVT1100: duplicate resource. type:
        // MANIFEST, name: 1` → `LNK1123`. `/MANIFEST:NO` only stops link.exe
        // from generating its own; the shipped exe keeps tauri's, unchanged.
        println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
    }

    tauri_build::build()
}
