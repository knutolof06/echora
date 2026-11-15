use std::path::PathBuf;

/// libvosk'un bulunduğu klasörü bağlayıcıya bildirir.
/// `VOSK_LIB_DIR` ortam değişkeni verilmişse o, yoksa depodaki `modeller/` kullanılır.
fn main() {
    let dizin = std::env::var_os("VOSK_LIB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("modeller")
                .join("vosk-win64-0.3.45")
        });
    println!("cargo:rustc-link-search=native={}", dizin.display());
    println!("cargo:rerun-if-env-changed=VOSK_LIB_DIR");
    println!("cargo:rerun-if-changed=build.rs");
}
