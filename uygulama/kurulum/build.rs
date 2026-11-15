use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

/// 1. `ECHORA_PAKET_ZIP` (mutlak yol) verilmişse zip'i OUT_DIR'a kopyalar, SHA-256'sını hesaplayıp
///    `ECHORA_PAKET_SHA256` derleme sabiti olarak gömer ve `paketli` yapılandırmasını açar
///    (main.rs zip'i `include_bytes!` ile gömer). Verilmemişse paketsiz "geliştirme" derlemesi olur.
/// 2. Simgeyi ve sürüm bilgisini Windows kaynağı olarak gömer (windres; yanki/build.rs ile aynı yöntem).
fn main() {
    println!("cargo:rustc-check-cfg=cfg(paketli)");
    println!("cargo:rerun-if-env-changed=ECHORA_PAKET_ZIP");
    println!("cargo:rerun-if-changed=build.rs");
    paketi_hazirla();
    kaynak_derle();
}

fn paketi_hazirla() {
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let Some(zip) = std::env::var_os("ECHORA_PAKET_ZIP").filter(|v| !v.is_empty()) else {
        return;
    };
    let zip = PathBuf::from(zip);
    if !zip.is_absolute() {
        panic!("ECHORA_PAKET_ZIP mutlak yol olmalı: {}", zip.display());
    }
    let bayt = std::fs::read(&zip).unwrap_or_else(|h| panic!("{} okunamadı: {h}", zip.display()));
    if bayt.len() < 4 || &bayt[..4] != b"PK\x03\x04" {
        panic!("{} bir zip dosyası değil", zip.display());
    }
    let ozet: String = Sha256::digest(&bayt).iter().map(|b| format!("{b:02x}")).collect();
    std::fs::write(out.join("paket.zip"), &bayt).expect("paket.zip OUT_DIR'a yazılamadı");
    println!("cargo:rerun-if-changed={}", zip.display());
    println!("cargo:rustc-cfg=paketli");
    println!("cargo:rustc-env=ECHORA_PAKET_SHA256={ozet}");
}

/// `"0.1.0"` → `0,1,0,0` (VERSIONINFO sayısal biçimi). Önsürüm eki (`-beta`) atılır.
fn surum_sayilari(surum: &str) -> String {
    let cekirdek = surum.split(['-', '+']).next().unwrap_or("0");
    let mut p: Vec<u16> = cekirdek.split('.').map(|s| s.parse().unwrap_or(0)).collect();
    p.resize(4, 0);
    p.iter().map(u16::to_string).collect::<Vec<_>>().join(",")
}

fn rc_dizgesi(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\"\"")
}

fn kaynak_derle() {
    let ico = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("yanki").join("kaynak").join("echora.ico");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let surum = env!("CARGO_PKG_VERSION");
    let sayilar = surum_sayilari(surum);
    println!("cargo:rerun-if-changed={}", ico.display());

    let mut rc = String::from("#pragma code_page(65001)\n");
    rc.push_str(&format!("1 ICON \"{}\"\n", rc_dizgesi(&ico.to_string_lossy().replace('\\', "/"))));
    let alanlar = [
        ("CompanyName", "Echora"),
        ("FileDescription", "Echora Kurulum"),
        ("FileVersion", surum),
        ("InternalName", "echora-kurulum"),
        ("LegalCopyright", "Echora geliştiricileri"),
        ("OriginalFilename", "Echora-Kurulum.exe"),
        ("ProductName", "Echora Kurulum"),
        ("ProductVersion", surum),
    ];
    rc.push_str(&format!(
        "1 VERSIONINFO\nFILEVERSION {sayilar}\nPRODUCTVERSION {sayilar}\nFILEFLAGSMASK 0x3F\nFILEFLAGS 0x0\nFILEOS 0x40004\nFILETYPE 0x1\nFILESUBTYPE 0x0\nBEGIN\n  BLOCK \"StringFileInfo\"\n  BEGIN\n    BLOCK \"041F04B0\"\n    BEGIN\n"
    ));
    for (ad, deger) in alanlar {
        rc.push_str(&format!("      VALUE \"{ad}\", \"{}\"\n", rc_dizgesi(deger)));
    }
    rc.push_str("    END\n  END\n  BLOCK \"VarFileInfo\"\n  BEGIN\n    VALUE \"Translation\", 0x041F, 1200\n  END\nEND\n");

    let rc_yolu = out.join("kurulum.rc");
    std::fs::write(&rc_yolu, rc).expect("kurulum.rc yazılamadı");
    let nesne = out.join("kurulum-kaynak.o");
    let windres = std::env::var("WINDRES").unwrap_or_else(|_| "windres".into());
    println!("cargo:rerun-if-env-changed=WINDRES");
    calistir(&windres, &rc_yolu, &nesne);
    println!("cargo:rustc-link-arg-bin=echora-kurulum={}", nesne.display());
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
