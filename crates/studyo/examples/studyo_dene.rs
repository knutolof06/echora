//! Stüdyo'yu geçici bir klasördeki varsayılan ayarlarla açar. Gerçek `%APPDATA%\Echora`
//! dosyalarına, kayıt defterine ve arka plan sürecine dokunmaz (hiçbir düğmeye basılmaz).
//!
//! `cargo run -p studyo --example studyo_dene -- --sure 4`
//! `cargo run -p studyo --example studyo_dene -- --hosgeldin --sure 4` (kurulum sihirbazı; örnek
//! kipinde kurulum, kısayol ve kayıt defteri yazımı kapalıdır)
//! `cargo run -p studyo --example studyo_dene -- --bolum kisayollar --kisayol-ornegi --sure 4`
//! (`--bolum`: açılış bölümü; `--kisayol-ornegi`: geçici ayara örnek `[[kisayol]]` satırları ekler)

use std::time::Duration;

fn main() {
    let arg: Vec<String> = std::env::args().skip(1).collect();
    let mut sure: Option<u64> = None;
    let mut hosgeldin = false;
    let mut bolum: Option<String> = None;
    let mut kisayol_ornegi = false;
    let mut i = 0;
    while i < arg.len() {
        match arg[i].as_str() {
            "--sure" => {
                i += 1;
                sure = Some(arg.get(i).and_then(|s| s.parse().ok()).expect("--sure saniye ister"));
            }
            "--hosgeldin" => hosgeldin = true,
            "--bolum" => {
                i += 1;
                bolum = Some(arg.get(i).cloned().expect("--bolum ad ister"));
            }
            "--kisayol-ornegi" => kisayol_ornegi = true,
            d => panic!("bilinmeyen seçenek: {d}"),
        }
        i += 1;
    }

    let klasor = std::env::temp_dir().join(format!("yanki-studyo-dene-{}", std::process::id()));
    std::fs::create_dir_all(&klasor).expect("geçici klasör");
    let ayar_yolu = klasor.join("ayarlar.toml");
    let mut ayar = komut::Ayarlar::varsayilan_toml().to_string();
    if kisayol_ornegi {
        ayar.push_str("
[[kisayol]]
tuslar = \"ctrl+alt+1\"
ifade = \"krom aç\"

[[kisayol]]
tuslar = \"ctrl+alt+5\"
ifade = \"sesi yüzde elli yap\"

[[kisayol]]
tuslar = \"ctrl+alt+k\"
ifade = \"otuz dakika sonra bilgisayarı uyut\"
");
    }
    std::fs::write(&ayar_yolu, ayar).expect("ayar yazılamadı");
    let anlasilmayanlar_yolu = klasor.join("anlasilmayanlar.txt");
    std::fs::write(&anlasilmayanlar_yolu, "krom kapat\nmüziği aç\nekranı karart\n").expect("yazılamadı");

    let s = studyo::StudyoSecenekleri {
        ayar_yolu: ayar_yolu.clone(),
        anlasilmayanlar_yolu,
        // Sözlük denetimi `echora.exe --sozluk-denetle` ister; örnekte o exe yok, düğme kapalı kalır.
        model_yolu: None,
        // Geliştirme yolu: "Windows ile başlat" kutusu ayrıca bu yüzden de devre dışıdır. Hoş geldin
        // kipinde sihirbaz "kurulmamış" görünümünü göstersin diye geçici klasörde var olmayan bir
        // exe yolu verilir (kurulum örnek kipinde zaten kapalıdır).
        exe_yolu: if hosgeldin { klasor.join("echora.exe") } else { std::env::current_exe().expect("exe yolu") },
        // Güncelleme durumu da geçici klasörde (gerçek %LOCALAPPDATA%\Echora okunmaz).
        guncelleme_klasoru: klasor.join("guncelleme"),
        ilk_kurulum: hosgeldin,
    };
    println!("ayar dosyası: {}", ayar_yolu.display());
    // Örnek kipi: kayıt defterine yazma, kurulum ve Echora'ya ileti gönderme kapalı.
    let sonuc = studyo::calistir_ornek_bolum(s, sure.map(Duration::from_secs), bolum.as_deref());
    let _ = std::fs::remove_dir_all(&klasor);
    match sonuc {
        Ok(()) => println!("Stüdyo kapandı."),
        Err(h) => {
            eprintln!("HATA: {h}");
            std::process::exit(1);
        }
    }
}
