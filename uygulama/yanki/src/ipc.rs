//! Stüdyo → arka plan `WM_COPYDATA` yükünün çözümü (saf). Protokolün tanımı `studyo::ipc`'dedir.
//!
//! Alıcı pencere sınıfı gerçek kipte `Echora.Tepsi`, `--kuru` kipinde `Echora.Tepsi.Kuru`'dur (kuru
//! örnekten açılan Stüdyo `ECHORA_KURU=1` ortam değişkeniyle ikincisini arar).
//!
//! Gönderenin aynı kullanıcı oturumundan geldiği varsayılır (UIPI daha düşük bütünlüklü
//! süreçlerin iletisini zaten engeller). Yük en çok `EN_COK_BAYT`; geçersiz UTF-8 reddedilir.

use komut::{turkce_kucuk, Ayarlar, Komut};

pub use studyo::ipc::{GUNCELLEME_DENETLE, GUNCELLEME_KUR, KOMUT_CALISTIR, YENIDEN_YUKLE};

// Sözleşme (plan, madde 9): 1 = komut çalıştır, 2 = yeniden yükle; 3 = güncellemeleri denetle,
// 4 = hazır güncellemeyi kur (onay tepsi sürecinin kutusunda).
const _: () = assert!(KOMUT_CALISTIR == 1 && YENIDEN_YUKLE == 2 && GUNCELLEME_DENETLE == 3 && GUNCELLEME_KUR == 4);

/// Kabul edilen en büyük yük.
pub const EN_COK_BAYT: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcIstegi {
    /// Tetiksiz ifade, düzlenmiş (Türkçe küçük harf, tek boşluk).
    Komut(String),
    YenidenYukle,
    GuncellemeDenetle,
    /// Hazır güncellemeyi kur (onayı tepsi süreci kendi kutusunda sorar).
    GuncellemeKur,
}

/// `dwData` ve yük baytlarını çözer. Hata metni yükü İÇERMEZ.
pub fn coz(tur: usize, veri: &[u8]) -> Result<IpcIstegi, String> {
    if veri.len() > EN_COK_BAYT {
        return Err(format!("yük çok büyük ({} bayt, en çok {EN_COK_BAYT})", veri.len()));
    }
    match tur {
        KOMUT_CALISTIR => {
            let metin = std::str::from_utf8(veri).map_err(|_| "yük geçerli UTF-8 değil".to_string())?;
            let ifade = duzle(metin);
            if ifade.is_empty() {
                return Err("boş ifade".into());
            }
            Ok(IpcIstegi::Komut(ifade))
        }
        YENIDEN_YUKLE => Ok(IpcIstegi::YenidenYukle),
        GUNCELLEME_DENETLE => Ok(IpcIstegi::GuncellemeDenetle),
        GUNCELLEME_KUR => Ok(IpcIstegi::GuncellemeKur),
        _ => Err(format!("bilinmeyen ileti türü {tur}")),
    }
}

/// Komut tablosunun ifade biçimi: Türkçe küçük harf + tek boşluk.
fn duzle(s: &str) -> String {
    turkce_kucuk(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// İfadeyle BİREBİR eşleşen komut. `Ayarlar::esle` kullanılmaz: tetik kuralı uygulanmaz (riskli
/// komutun onayını Stüdyo aldı) ve tetikle başlayan metin aranmaz.
pub fn komut_bul<'a>(ayarlar: &'a Ayarlar, ifade: &str) -> Option<&'a Komut> {
    ayarlar.komutlar.iter().find(|k| k.ifade == ifade)
}

#[cfg(test)]
mod testler {
    use super::*;
    use komut::Eylem;

    fn ayarlar() -> Ayarlar {
        Ayarlar::coz(Ayarlar::varsayilan_toml()).unwrap()
    }

    #[test]
    fn gecerli_komut_yuku() {
        assert_eq!(coz(1, "krom aç".as_bytes()), Ok(IpcIstegi::Komut("krom aç".into())));
        // Düzlenir: büyük harf (Türkçe I) ve fazla boşluk.
        assert_eq!(coz(1, "  EKRANLARI   KAPAT ".as_bytes()), Ok(IpcIstegi::Komut("ekranları kapat".into())));
        assert_eq!(coz(2, &[]), Ok(IpcIstegi::YenidenYukle));
        assert_eq!(coz(3, &[]), Ok(IpcIstegi::GuncellemeDenetle));
        assert_eq!(coz(4, &[]), Ok(IpcIstegi::GuncellemeKur));
        assert!(coz(5, &[]).is_err());
    }

    #[test]
    fn gecersiz_utf8_ve_bos_reddedilir() {
        assert!(coz(1, &[0x6B, 0xFF, 0xFE]).is_err());
        // Yarım kalmış çok baytlı karakter ("ç" = C3 A7).
        assert!(coz(1, &[0x61, 0xC3]).is_err());
        assert!(coz(1, b"   ").is_err());
        assert!(coz(1, &[]).is_err());
    }

    #[test]
    fn boyut_siniri() {
        let tam = vec![b'a'; EN_COK_BAYT];
        assert!(coz(1, &tam).is_ok());
        let fazla = vec![b'a'; EN_COK_BAYT + 1];
        let h = coz(1, &fazla).unwrap_err();
        assert!(h.contains("çok büyük"));
        assert!(coz(2, &fazla).is_err());
    }

    #[test]
    fn bilinmeyen_tur_reddedilir_ve_hata_yuku_icermez() {
        let h = coz(7, b"gizli").unwrap_err();
        assert!(!h.contains("gizli"));
        let h = coz(1, b"qxqx\xFF").unwrap_err();
        assert!(!h.contains("qxqx"));
    }

    #[test]
    fn birebir_eslesme_tetik_kuralsiz() {
        let a = ayarlar();
        let k = komut_bul(&a, "krom aç").unwrap();
        assert!(matches!(&k.eylem, Eylem::UygulamaAc { hedef, .. } if hedef == "chrome"));
        // Riskli komut tetiksiz bulunur (esle TetikGerekli derdi).
        let k = komut_bul(&a, "ekranları kapat").unwrap();
        assert!(k.riskli);
        assert_eq!(k.eylem, Eylem::EkranlariKapat);
        // Tetikli metin birebir değildir.
        assert!(komut_bul(&a, "atlas ekranları kapat").is_none());
        assert!(komut_bul(&a, "olmayan ifade").is_none());
    }
}
