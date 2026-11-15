use std::path::{Path, PathBuf};
use std::process::Command;

/// 1. libvosk ve bağımlı DLL'leri exe'nin yanına kopyalar; Windows DLL'leri önce orada arar.
/// 2. Simgeleri (exe + tepsi) ve sürüm bilgisini Windows kaynağı olarak gömer: `.rc` OUT_DIR'a
///    yazılır, GNU `windres` ile COFF nesnesine derlenir ve yalnız `echora` ikilisine bağlanır.
fn main() {
    dll_kopyala();
    kaynak_derle();
    println!("cargo:rerun-if-changed=build.rs");
}

fn dll_kopyala() {
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
                "{} kopyalanamadı: {h}. Vosk kütüphanesi için modeller/OKU.md'ye bakın ya da VOSK_LIB_DIR verin. echora.exe çalışıyor olabilir: kapatıp yeniden deneyin.",
                k.display()
            );
        }
        println!("cargo:rerun-if-changed={}", k.display());
    }
    println!("cargo:rerun-if-env-changed=VOSK_LIB_DIR");
}

/// Kaynak kimlikleri `src/tepsi.rs`'deki `SIMGE_*` sabitleriyle aynı olmalı.
const SIMGELER: [(u16, &str); 4] =
    [(1, "echora.ico"), (101, "tepsi.ico"), (102, "tepsi-duraklat.ico"), (103, "tepsi-hata.ico")];

/// `"0.1.0"` → `0,1,0,0` (VERSIONINFO sayısal biçimi). Önsürüm eki (`-beta`) atılır.
fn surum_sayilari(surum: &str) -> String {
    let cekirdek = surum.split(['-', '+']).next().unwrap_or("0");
    let mut p: Vec<u16> = cekirdek.split('.').map(|s| s.parse().unwrap_or(0)).collect();
    p.resize(4, 0);
    p.iter().map(u16::to_string).collect::<Vec<_>>().join(",")
}

/// `.rc` dizgesi: ters bölü ve tırnak kaçışlı.
fn rc_dizgesi(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\"\"")
}

fn kaynak_derle() {
    let kok = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("kaynak");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let surum = env!("CARGO_PKG_VERSION");
    let sayilar = surum_sayilari(surum);

    let mut rc = String::from("#pragma code_page(65001)\n");
    for (no, ad) in SIMGELER {
        let yol = kok.join(ad);
        println!("cargo:rerun-if-changed={}", yol.display());
        // windres eğik çizgili yolları da kabul eder; kaçış derdi olmasın.
        rc.push_str(&format!("{no} ICON \"{}\"\n", rc_dizgesi(&yol.to_string_lossy().replace('\\', "/"))));
    }
    // VS_VERSION_INFO = 1; sayılar winver.h'ye gerek kalmasın diye açık yazıldı:
    // VS_FFI_FILEFLAGSMASK 0x3F, VOS_NT_WINDOWS32 0x40004, VFT_APP 1. 041F04B0 = Türkçe, UTF-16.
    let alanlar = [
        ("CompanyName", "Echora"),
        ("FileDescription", "Echora — sesli komutla Windows kontrolü"),
        ("FileVersion", surum),
        ("InternalName", "echora"),
        ("LegalCopyright", "Echora geliştiricileri"),
        ("OriginalFilename", "echora.exe"),
        ("ProductName", "Echora"),
        ("ProductVersion", surum),
    ];
    rc.push_str(&format!(
        "1 VERSIONINFO\nFILEVERSION {sayilar}\nPRODUCTVERSION {sayilar}\nFILEFLAGSMASK 0x3F\nFILEFLAGS 0x0\nFILEOS 0x40004\nFILETYPE 0x1\nFILESUBTYPE 0x0\nBEGIN\n  BLOCK \"StringFileInfo\"\n  BEGIN\n    BLOCK \"041F04B0\"\n    BEGIN\n"
    ));
    for (ad, deger) in alanlar {
        rc.push_str(&format!("      VALUE \"{ad}\", \"{}\"\n", rc_dizgesi(deger)));
    }
    rc.push_str("    END\n  END\n  BLOCK \"VarFileInfo\"\n  BEGIN\n    VALUE \"Translation\", 0x041F, 1200\n  END\nEND\n");

    let rc_yolu = out.join("echora.rc");
    std::fs::write(&rc_yolu, rc).expect("echora.rc yazılamadı");
    let nesne = out.join("echora-kaynak.o");
    let windres = std::env::var("WINDRES").unwrap_or_else(|_| "windres".into());
    println!("cargo:rerun-if-env-changed=WINDRES");
    calistir(&windres, &rc_yolu, &nesne);
    // Yalnız uygulama ikilisine: test ikilileri simgesiz kalabilir.
    println!("cargo:rustc-link-arg-bin=echora={}", nesne.display());
}

fn calistir(windres: &str, rc: &Path, nesne: &Path) {
    let sonuc = Command::new(windres)
        .arg("--codepage=65001")
        .arg("-i")
        .arg(rc)
        .arg("-O")
        .arg("coff")
        .arg("-o")
        .arg(nesne)
        .output();
    match sonuc {
        Ok(c) if c.status.success() => {}
        Ok(c) => panic!(
            "windres başarısız ({}): {}{}",
            c.status,
            String::from_utf8_lossy(&c.stdout),
            String::from_utf8_lossy(&c.stderr)
        ),
        Err(h) => panic!(
            "windres çalıştırılamadı ({windres}): {h}. MinGW-w64 windres PATH'te olmalı ya da WINDRES ortam değişkeniyle verilmeli."
        ),
    }
}
