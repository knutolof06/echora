use std::path::PathBuf;

/// libvosk ve bağımlı DLL'leri exe'nin yanına kopyalar; Windows DLL'leri önce orada arar.
fn main() {
    let kaynak = std::env::var_os("VOSK_LIB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("modeller")
                .join("vosk-win64-0.3.45")
        });
    // OUT_DIR = target/<profil>/build/<paket>-<hash>/out  →  üç üst klasör = target/<profil>
    let hedef = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"))
        .ancestors()
        .nth(3)
        .expect("target profil klasörü")
        .to_path_buf();

    for ad in [
        "libvosk.dll",
        "libstdc++-6.dll",
        "libgcc_s_seh-1.dll",
        "libwinpthread-1.dll",
    ] {
        let k = kaynak.join(ad);
        // Eksik DLL ile derleme geçerse hata çalışma zamanında anlaşılmaz bir biçimde çıkar.
        if let Err(h) = std::fs::copy(&k, hedef.join(ad)) {
            panic!(
                "{} kopyalanamadı: {h}. Vosk kütüphanesi için modeller/OKU.md'ye bakın ya da VOSK_LIB_DIR verin.",
                k.display()
            );
        }
        println!("cargo:rerun-if-changed={}", k.display());
    }
    println!("cargo:rerun-if-env-changed=VOSK_LIB_DIR");
    println!("cargo:rerun-if-changed=build.rs");
}
