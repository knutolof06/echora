//! Komut paletini geçici bir klasördeki varsayılan ayarlarla açar. Echora'ya ileti gönderilmez
//! (örnek kipi); soğuk açılış süresi standart çıktıya yazılır.
//!
//! `cargo run -p studyo --example palet_dene -- --sure 3`
//! `cargo run -p studyo --example palet_dene -- --sure 3 --ara "uyut"`

use std::time::Duration;

fn main() {
    let arg: Vec<String> = std::env::args().skip(1).collect();
    let mut sure: Option<u64> = None;
    let mut ara: Option<String> = None;
    let mut i = 0;
    while i < arg.len() {
        match arg[i].as_str() {
            "--sure" => {
                i += 1;
                sure = Some(arg.get(i).and_then(|s| s.parse().ok()).expect("--sure saniye ister"));
            }
            "--ara" => {
                i += 1;
                ara = Some(arg.get(i).cloned().expect("--ara metin ister"));
            }
            d => panic!("bilinmeyen seçenek: {d}"),
        }
        i += 1;
    }
    let klasor = std::env::temp_dir().join(format!("yanki-palet-dene-{}", std::process::id()));
    std::fs::create_dir_all(&klasor).expect("geçici klasör");
    let ayar_yolu = klasor.join("ayarlar.toml");
    std::fs::write(&ayar_yolu, komut::Ayarlar::varsayilan_toml()).expect("ayar yazılamadı");
    let sonuc = studyo::palet_calistir_ornek(studyo::PaletSecenekleri { ayar_yolu }, sure.map(Duration::from_secs), ara);
    let _ = std::fs::remove_dir_all(&klasor);
    if let Err(h) = sonuc {
        eprintln!("HATA: {h}");
        std::process::exit(1);
    }
}
