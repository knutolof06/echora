//! "Sözlüğü denetle": `echora.exe --sozluk-denetle --model <klasör>` alt sürecine gramer ifadelerini
//! stdin'den (satır başına bir) verir; Vosk'un stderr uyarılarından modelde olmayan kelimeleri
//! çıkarır. Protokol `uygulama/yanki/src/dinleyici.rs` içindeki `sozluk_denetle` ile aynıdır.

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Alt sürecin en uzun çalışma süresi; dolunca öldürülür.
const SINIR: Duration = Duration::from_secs(15);
/// Vosk'un sözlükte olmayan kelime uyarısı.
const UYARI_ONEKI: &str = "Ignoring word missing in vocabulary: '";
/// Bu satır görülmediyse "eksik kelime yok" sonucuna güvenilmez.
const GRAMER_ISARETI: &str = "UpdateGrammarFst";

/// Vosk stderr metninden eksik kelimeleri (yinelemesiz, görülme sırasıyla) ayıklar.
pub fn eksikleri_ayikla(stderr: &str) -> Vec<String> {
    let mut eksikler: Vec<String> = Vec::new();
    for satir in stderr.lines() {
        let Some(bas) = satir.find(UYARI_ONEKI) else { continue };
        let kalan = &satir[bas + UYARI_ONEKI.len()..];
        let Some(son) = kalan.rfind('\'') else { continue };
        let kelime = kalan[..son].trim();
        if !kelime.is_empty() && !eksikler.iter().any(|e| e == kelime) {
            eksikler.push(kelime.to_string());
        }
    }
    eksikler
}

/// Alt süreci çalıştırır (bloklar; çağıran ayrı iş parçacığında çalıştırmalı).
pub fn denetle(exe: &Path, model: &Path, ifadeler: &[String]) -> Result<Vec<String>, String> {
    let mut komut = Command::new(exe);
    komut
        .args(["--sozluk-denetle", "--model"])
        .arg(model)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    {
        use std::os::windows::process::CommandExt;
        komut.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut cocuk = komut.spawn().map_err(|h| format!("{} başlatılamadı: {h}", exe.display()))?;
    // Bundan sonra her hata yolunda alt süreç öldürülür ve beklenir (yetim süreç kalmaz).
    let oldur = |c: &mut std::process::Child| {
        let _ = c.kill();
        let _ = c.wait();
    };
    let (Some(mut girdi), Some(mut boru)) = (cocuk.stdin.take(), cocuk.stderr.take()) else {
        oldur(&mut cocuk);
        return Err("alt sürecin girdi/çıktı boruları alınamadı".into());
    };
    // stdin ayrı iş parçacığında yazılır: alt süreç okumazsa boru dolar ve yazım bloklar; süre
    // sınırı yazımı da kapsar (süre dolunca süreç öldürülür, boru kırılır, yazım hatayla biter).
    let metin = ifadeler.join("\n");
    let yazici = std::thread::spawn(move || girdi.write_all(metin.as_bytes()).map_err(|h| h.to_string()));
    // girdi iş parçacığında düşer: stdin kapanır, alt süreç okumayı bitirir.
    let okuyucu = std::thread::spawn(move || {
        let mut t = Vec::new();
        let _ = boru.read_to_end(&mut t);
        t
    });
    let bitis = Instant::now() + SINIR;
    let durum = loop {
        match cocuk.try_wait() {
            Ok(Some(d)) => break d,
            Ok(None) if Instant::now() >= bitis => {
                oldur(&mut cocuk);
                let _ = yazici.join();
                let _ = okuyucu.join();
                return Err(format!("{} sn içinde bitmedi", SINIR.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(h) => {
                oldur(&mut cocuk);
                return Err(h.to_string());
            }
        }
    };
    let yazim = yazici.join().unwrap_or_else(|_| Err("yazım iş parçacığı çöktü".into()));
    let tampon = okuyucu.join().unwrap_or_default();
    let metin = String::from_utf8_lossy(&tampon);
    if !durum.success() {
        return Err(format!("denetim başarısız: {}", metin.lines().last().unwrap_or("model yüklenemedi")));
    }
    // Süreç başarıyla bittiyse ifadelerin tamamı yazılmış olmalı; yarım girdiyle sonuç güvenilmez.
    yazim.map_err(|h| format!("ifadeler alt sürece yazılamadı: {h}"))?;
    if !metin.contains(GRAMER_ISARETI) {
        return Err("Vosk'un gramer günlüğü görülmedi; sonuç bilinmiyor".into());
    }
    Ok(eksikleri_ayikla(&metin))
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn uyarilardan_kelime_ayiklama() {
        let stderr = "LOG (VoskAPI:ReadDataFiles():model.cc:213) Decoding params\n\
WARNING (VoskAPI:UpdateGrammarFst():recognizer.cc:308) Ignoring word missing in vocabulary: 'krom'\n\
WARNING (VoskAPI:UpdateGrammarFst():recognizer.cc:308) Ignoring word missing in vocabulary: 'küçült'\n\
WARNING (VoskAPI:UpdateGrammarFst():recognizer.cc:308) Ignoring word missing in vocabulary: 'krom'";
        assert_eq!(eksikleri_ayikla(stderr), vec!["krom", "küçült"]);
        assert!(eksikleri_ayikla("").is_empty());
    }
}
