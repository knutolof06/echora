//! `Ayarlar` için eski kurallar (davranış değişmedi; yalnız varsayılan dosyanın boyutu güncellendi).

use crate::{Ayarlar, Eslesme, Eylem};
use std::path::Path;

/// Varsayılan dosyadaki komut sayısı (şablon açılımları dahil).
const VARSAYILAN_KOMUT_SAYISI: usize = 228;


fn coz_ok(metin: &str) -> Ayarlar {
    Ayarlar::coz(metin).unwrap_or_else(|h| panic!("beklenmedik hata: {h}"))
}

fn hata(metin: &str) -> String {
    Ayarlar::coz(metin).expect_err("hata bekleniyordu")
}

fn kok(govde: &str) -> String {
    format!("tetik = \"atlas\"\n{govde}")
}

fn komut_ornegi(ifade: &str) -> String {
    kok(&format!("[[komut]]\nifadeler = [\"{ifade}\"]\neylem = \"ekranlari_ac\"\n"))
}

const ORNEK: &str = r#"
tetik = "Atlas"
[[komut]]
ifadeler = ["Ekranı  Kapat", "hepsini kapat"]
eylem = "ekranlari_kapat"
riskli = true
[[komut]]
ifadeler = ["ekranı aç"]
eylem = "ekranlari_ac"
"#;

// --- varsayılan dosya ---

#[test]
fn varsayilan_dosya_gecerli_ve_beklenen_komutlari_uretir() {
    let a = coz_ok(Ayarlar::varsayilan_toml());
    assert_eq!(a.tetik, "atlas");
    assert_eq!(a.guven_esigi, 0.7);
    assert_eq!(a.model_yolu, None);
    assert_eq!(a.komutlar.len(), VARSAYILAN_KOMUT_SAYISI);
    assert!(a.komutlar.iter().all(|k| !k.ifade.is_empty()));
}

#[test]
fn varsayilan_dosyada_riskli_komutlar() {
    let a = coz_ok(Ayarlar::varsayilan_toml());
    let riskli = |i: &str| a.komutlar.iter().find(|k| k.ifade == i).unwrap().riskli;
    assert!(riskli("ekranları kapat"));
    assert!(riskli("ana ekranı kapat"));
    assert!(riskli("dinlemeyi durdur"));
    assert!(!riskli("ekranları aç"));
    assert!(!riskli("krom aç"));
}

// --- temel okuma ---

#[test]
fn coz_duzler_ve_alanlari_doldurur() {
    let a = coz_ok(ORNEK);
    assert_eq!(a.tetik, "atlas");
    assert_eq!(a.guven_esigi, 0.7);
    assert_eq!(a.komutlar.len(), 3);
    assert_eq!(a.komutlar[0].ifade, "ekranı kapat");
    assert_eq!(a.komutlar[0].gosterim, "ekranı kapat");
    assert_eq!(a.komutlar[0].eylem, Eylem::EkranlariKapat);
    assert!(a.komutlar[0].riskli);
    assert!(!a.komutlar[2].riskli);
}

#[test]
fn model_yolu_ve_esik_okunur() {
    let a = coz_ok(&format!("model_yolu = \"m/tr\"\nguven_esigi = 0.5\n{}", komut_ornegi("ekranı aç")));
    assert_eq!(a.model_yolu.as_deref(), Some("m/tr"));
    assert_eq!(a.guven_esigi, 0.5);
}

#[test]
fn tamsayi_esik_kabul_edilir() {
    let a = coz_ok(&format!("guven_esigi = 1\n{}", komut_ornegi("ekranı aç")));
    assert_eq!(a.guven_esigi, 1.0);
}

#[test]
fn cik_eylemi_okunur() {
    let a = coz_ok(&kok("[[komut]]\nifadeler = [\"uygulamadan çık\"]\neylem = \"cik\""));
    assert_eq!(a.komutlar[0].eylem, Eylem::Cik);
}

// --- guven_esigi ---

#[test]
fn aralik_disi_esik_reddedilir() {
    for e in ["1.5", "-0.1", "nan", "inf"] {
        let m = format!("guven_esigi = {e}\n{}", komut_ornegi("ekranı aç"));
        assert!(hata(&m).contains("guven_esigi"), "kabul edildi: {e}");
    }
    for e in ["0.0", "1.0", "0.7"] {
        let m = format!("guven_esigi = {e}\n{}", komut_ornegi("ekranı aç"));
        assert!(Ayarlar::coz(&m).is_ok(), "reddedildi: {e}");
    }
}

// --- tetik ---

#[test]
fn gecersiz_tetik_reddedilir() {
    for t in ["", "  ", "at las", "atlas1", "at-las"] {
        let m = format!("tetik = \"{t}\"\n[[komut]]\nifadeler = [\"ekranı aç\"]\neylem = \"ekranlari_ac\"");
        assert!(hata(&m).contains("tetik"), "kabul edildi: {t:?}");
    }
    // Büyük harf düzlenir.
    let m = "tetik = \"ATLAS\"\n[[komut]]\nifadeler = [\"ekranı aç\"]\neylem = \"ekranlari_ac\"";
    assert_eq!(coz_ok(m).tetik, "atlas");
}

#[test]
fn tetik_zorunludur() {
    assert!(Ayarlar::coz("[[komut]]\nifadeler = [\"a b\"]\neylem = \"cik\"").is_err());
}

// --- bilinmeyen alan / eylem ---

#[test]
fn yazim_hatali_alan_reddedilir() {
    // "riskly" sessizce yok sayılsaydı komut tetiksiz çalışırdı.
    let m = kok("[[komut]]\nifadeler = [\"ekranı kapat\"]\neylem = \"ekranlari_kapat\"\nriskly = true");
    assert!(Ayarlar::coz(&m).is_err());
    assert!(Ayarlar::coz(&komut_ornegi("ekranı aç").replace("tetik = ", "tetikk = \"x\"\ntetik = ")).is_err());
    let u = kok("[[uygulama]]\nsoylenis = \"krom\"\nad = \"C\"\nhedef = \"c\"\nxx = 1\n[[komut]]\nifadeler = [\"a b\"]\neylem = \"cik\"");
    assert!(Ayarlar::coz(&u).is_err());
}

#[test]
fn bilinmeyen_eylem_reddedilir() {
    let m = kok("[[komut]]\nifadeler = [\"ekranı aç\"]\neylem = \"patlat\"");
    let h = hata(&m);
    assert!(h.contains("patlat"), "{h}");
}

#[test]
fn bos_ifade_listesi_ve_bos_ifade_reddedilir() {
    assert!(hata(&kok("[[komut]]\nifadeler = []\neylem = \"cik\"")).contains("en az bir ifade"));
    assert!(hata(&komut_ornegi("   ")).contains("boş"));
}

#[test]
fn hic_komut_yoksa_hata() {
    assert!(Ayarlar::coz("tetik = \"atlas\"").is_err());
}

// --- ifade karakter denetimi ---

#[test]
fn grameri_bozan_karakterler_reddedilir() {
    for kotu in ["krom \\\\\\\" aç", "sesi %20 yap", "google'ı aç", "aç!", "kro\\u0307m", "ses 5", "a.b"] {
        let m = komut_ornegi(kotu);
        assert!(Ayarlar::coz(&m).is_err(), "kabul edildi: {kotu}");
    }
    assert!(Ayarlar::coz(&komut_ornegi("çok şık ığdır")).is_ok());
}

#[test]
fn sablon_sonrasi_sonuc_da_denetlenir() {
    let m = kok(
        "[[uygulama]]\nsoylenis = \"krom2\"\nad = \"C\"\nhedef = \"c\"\n\
         [[komut]]\nifadeler = [\"{uygulama} aç\"]\neylem = \"uygulama_ac\"",
    );
    assert!(hata(&m).contains("soylenis"));
}

// --- tetikle başlama ---

#[test]
fn tetikle_baslayan_ifade_reddedilir() {
    assert!(hata(&komut_ornegi("atlas krom aç")).contains("tetik"));
    assert!(hata(&komut_ornegi("Atlas")).contains("tetik"));
    // Kelime sınırı: "atlasım aç" serbest, tetik "at" iken "atlas aç" serbest.
    assert!(Ayarlar::coz(&komut_ornegi("atlasım aç")).is_ok());
    let m = "tetik = \"at\"\n[[komut]]\nifadeler = [\"atlas aç\"]\neylem = \"ekranlari_ac\"";
    assert!(Ayarlar::coz(m).is_ok());
}

#[test]
fn sablondan_acilan_ifade_tetikle_baslayamaz() {
    let m = kok(
        "[[uygulama]]\nsoylenis = \"atlas\"\nad = \"A\"\nhedef = \"a\"\n\
         [[komut]]\nifadeler = [\"{uygulama} aç\"]\neylem = \"uygulama_ac\"",
    );
    assert!(hata(&m).contains("tetik"));
}

// --- yineleme ---

#[test]
fn yinelenen_ifade_iki_kaynagi_soyler() {
    let m = kok(
        "[[komut]]\nifadeler = [\"ekranı aç\"]\neylem = \"ekranlari_ac\"\n\
         [[komut]]\nifadeler = [\"EKRANI  AÇ\"]\neylem = \"ekranlari_kapat\"",
    );
    let h = hata(&m);
    assert!(h.contains("yinelenen"), "{h}");
    assert!(h.contains("komut #1") && h.contains("komut #2"), "{h}");
}

#[test]
fn ayni_komut_icinde_yineleme_de_reddedilir() {
    let m = kok("[[komut]]\nifadeler = [\"ekranı aç\", \"ekranı  aç\"]\neylem = \"ekranlari_ac\"");
    assert!(hata(&m).contains("yinelenen"));
}

#[test]
fn sablon_acilimi_ile_dogrudan_ifade_cakisirsa_hata() {
    let m = kok(
        "[[uygulama]]\nsoylenis = \"krom\"\nad = \"Chrome\"\nhedef = \"chrome\"\n\
         [[komut]]\nifadeler = [\"{uygulama} aç\"]\neylem = \"uygulama_ac\"\n\
         [[komut]]\nifadeler = [\"krom aç\"]\neylem = \"ekranlari_ac\"",
    );
    assert!(hata(&m).contains("yinelenen"));
}

// --- riskli varsayılanı eyleme göre ---

#[test]
fn riskli_yazilmazsa_eyleme_gore_belirlenir() {
    // `riskli` satırını unutmak ekran kapatmayı tetiksiz çalışır hale getirmemeli.
    for (eylem, beklenen) in [
        ("ekranlari_kapat", true),
        ("ana_ekrani_kapat", true),
        ("dinlemeyi_duraklat", true),
        ("cik", true),
        ("ekranlari_ac", false),
    ] {
        let a = coz_ok(&kok(&format!("[[komut]]\nifadeler = [\"şunu bunu yap\"]\neylem = \"{eylem}\"")));
        assert_eq!(a.komutlar[0].riskli, beklenen, "{eylem}");
    }
    let a = coz_ok(&kok(
        "[[uygulama]]\nsoylenis = \"krom\"\nad = \"Chrome\"\nhedef = \"chrome\"\n\
         [[komut]]\nifadeler = [\"{uygulama} aç\"]\neylem = \"uygulama_ac\"",
    ));
    assert!(!a.komutlar[0].riskli);
}

#[test]
fn riskli_acikca_yazilirsa_kullanicinin_secimi_gecerli() {
    let a = coz_ok(&kok(
        "[[komut]]\nifadeler = [\"ekranı kapat\"]\neylem = \"ekranlari_kapat\"\nriskli = false\n\
         [[komut]]\nifadeler = [\"ekranları aç\"]\neylem = \"ekranlari_ac\"\nriskli = true",
    ));
    assert!(!a.komutlar[0].riskli);
    assert!(a.komutlar[1].riskli);
    assert_eq!(a.esle("ekranı kapat"), Eslesme::Komut(&a.komutlar[0]));
    assert_eq!(a.esle("ekranları aç"), Eslesme::TetikGerekli(&a.komutlar[1]));
}

#[test]
fn riskli_unutulan_ekran_kapatma_tetiksiz_calismaz() {
    let a = coz_ok(&kok("[[komut]]\nifadeler = [\"ekranı kapat\"]\neylem = \"ekranlari_kapat\""));
    assert_eq!(a.esle("ekranı kapat"), Eslesme::TetikGerekli(&a.komutlar[0]));
    assert_eq!(a.esle("atlas ekranı kapat"), Eslesme::Komut(&a.komutlar[0]));
}

// --- tek kelime riskli ---

#[test]
fn tek_kelimelik_ifade_otomatik_riskli() {
    let m = kok(
        "[[komut]]\nifadeler = [\"kapat\", \"ekranı aç\"]\neylem = \"ekranlari_kapat\"\nriskli = false",
    );
    let a = coz_ok(&m);
    assert!(a.komutlar[0].riskli);
    assert!(!a.komutlar[1].riskli);
}

#[test]
fn acilmis_tek_kelime_riskli_olur() {
    let m = kok(
        "[[uygulama]]\nsoylenis = \"krom\"\nad = \"Chrome\"\nhedef = \"chrome\"\n\
         [[komut]]\nifadeler = [\"{uygulama}\", \"{uygulama} aç\"]\neylem = \"uygulama_ac\"",
    );
    let a = coz_ok(&m);
    let k = |i: &str| a.komutlar.iter().find(|k| k.ifade == i).unwrap();
    assert!(k("krom").riskli);
    assert!(!k("krom aç").riskli);
}

#[test]
fn tek_kelime_tetiksiz_calismaz() {
    let a = coz_ok(&kok("[[komut]]\nifadeler = [\"kapat\"]\neylem = \"cik\""));
    assert_eq!(a.esle("kapat"), Eslesme::TetikGerekli(&a.komutlar[0]));
    assert_eq!(a.esle("atlas kapat"), Eslesme::Komut(&a.komutlar[0]));
}

// --- uygulama_ac / şablon ---

#[test]
fn iki_sablon_uc_uygulama_alti_komut() {
    let m = kok(
        "[[uygulama]]\nsoylenis = \"krom\"\nad = \"Chrome\"\nhedef = \"chrome\"\nargumanlar = [\"--yeni\"]\n\
         [[uygulama]]\nsoylenis = \"not defteri\"\nad = \"Not Defteri\"\nhedef = \"notepad\"\n\
         [[uygulama]]\nsoylenis = \"youtube\"\nad = \"YouTube\"\nhedef = \"https://www.youtube.com\"\n\
         [[komut]]\nifadeler = [\"{uygulama} aç\", \"{uygulama} başlat\"]\neylem = \"uygulama_ac\"",
    );
    let a = coz_ok(&m);
    assert_eq!(a.komutlar.len(), 6);
    assert_eq!(a.komutlar[0].ifade, "krom aç");
    assert_eq!(a.komutlar[0].gosterim, "Chrome aç");
    assert_eq!(
        a.komutlar[0].eylem,
        Eylem::UygulamaAc { ad: "Chrome".into(), hedef: "chrome".into(), argumanlar: vec!["--yeni".into()] }
    );
    assert_eq!(a.komutlar[1].ifade, "not defteri aç");
    assert_eq!(a.komutlar[1].gosterim, "Not Defteri aç");
    assert_eq!(a.komutlar[2].ifade, "youtube aç");
    assert_eq!(a.komutlar[3].ifade, "krom başlat");
    assert_eq!(a.komutlar[3].gosterim, "Chrome başlat");
    assert_eq!(
        a.komutlar[5].eylem,
        Eylem::UygulamaAc {
            ad: "YouTube".into(),
            hedef: "https://www.youtube.com".into(),
            argumanlar: vec![]
        }
    );
}

#[test]
fn uygulama_ac_yer_tutucusuz_veya_cift_reddedilir() {
    let taban = "[[uygulama]]\nsoylenis = \"krom\"\nad = \"C\"\nhedef = \"c\"\n[[komut]]\neylem = \"uygulama_ac\"\n";
    let yok = kok(&format!("{taban}ifadeler = [\"krom aç\"]"));
    assert!(hata(&yok).contains("{uygulama}"));
    let cift = kok(&format!("{taban}ifadeler = [\"{{uygulama}} {{uygulama}} aç\"]"));
    assert!(hata(&cift).contains("{uygulama}"));
}

#[test]
fn yer_tutucu_baska_eylemde_yasak() {
    let m = kok("[[komut]]\nifadeler = [\"{uygulama} kapat\"]\neylem = \"ekranlari_kapat\"");
    assert!(hata(&m).contains("{uygulama}"));
}

#[test]
fn uygulama_ac_var_uygulama_yoksa_hata_degil() {
    let m = kok(
        "[[komut]]\nifadeler = [\"{uygulama} aç\"]\neylem = \"uygulama_ac\"\n\
         [[komut]]\nifadeler = [\"ekranı aç\"]\neylem = \"ekranlari_ac\"",
    );
    let a = coz_ok(&m);
    assert_eq!(a.komutlar.len(), 1);
    // Yalnız şablonlu komut ve uygulama yoksa hiç komut üretilmez: hata.
    let tek = kok("[[komut]]\nifadeler = [\"{uygulama} aç\"]\neylem = \"uygulama_ac\"");
    assert!(Ayarlar::coz(&tek).is_err());
}

#[test]
fn gecersiz_uygulama_alanlari_reddedilir() {
    let kalip = |s: &str, ad: &str, h: &str| {
        kok(&format!(
            "[[uygulama]]\nsoylenis = \"{s}\"\nad = \"{ad}\"\nhedef = \"{h}\"\n\
             [[komut]]\nifadeler = [\"{{uygulama}} aç\"]\neylem = \"uygulama_ac\""
        ))
    };
    assert!(Ayarlar::coz(&kalip("", "A", "a")).is_err());
    assert!(Ayarlar::coz(&kalip("krom", " ", "a")).is_err());
    assert!(Ayarlar::coz(&kalip("krom", "A", "")).is_err());
    assert!(Ayarlar::coz(&kalip("kr0m", "A", "a")).is_err());
    assert!(Ayarlar::coz(&kalip("krom", "A", "a")).is_ok());
}

// --- gramer ---

#[test]
fn gramer_yinelenmesiz_ve_iki_bicimi_icerir() {
    let a = coz_ok(ORNEK);
    let g = a.gramer_ifadeleri();
    assert_eq!(
        g,
        vec![
            "ekranı kapat", "atlas ekranı kapat", "hepsini kapat", "atlas hepsini kapat",
            "ekranı aç", "atlas ekranı aç"
        ]
    );
    let benzersiz: std::collections::HashSet<_> = g.iter().collect();
    assert_eq!(benzersiz.len(), g.len());
}

#[test]
fn varsayilan_gramer_komut_sayisinin_iki_kati() {
    let a = coz_ok(Ayarlar::varsayilan_toml());
    let g = a.gramer_ifadeleri();
    assert_eq!(g.len(), VARSAYILAN_KOMUT_SAYISI * 2);
    assert!(g.contains(&"krom aç".to_string()));
    assert!(g.contains(&"atlas krom aç".to_string()));
}

// --- eşleme ---

#[test]
fn riskli_komut_tetiksiz_calismaz() {
    let a = coz_ok(ORNEK);
    assert_eq!(a.esle("ekranı kapat"), Eslesme::TetikGerekli(&a.komutlar[0]));
    assert_eq!(a.esle("atlas ekranı kapat"), Eslesme::Komut(&a.komutlar[0]));
}

#[test]
fn zararsiz_komut_tetikli_de_tetiksiz_de_calisir() {
    let a = coz_ok(ORNEK);
    assert_eq!(a.esle("ekranı aç"), Eslesme::Komut(&a.komutlar[2]));
    assert_eq!(a.esle("atlas ekranı aç"), Eslesme::Komut(&a.komutlar[2]));
}

#[test]
fn eslesmeyen_metin_yok_doner() {
    let a = coz_ok(ORNEK);
    assert_eq!(a.esle(""), Eslesme::Yok);
    assert_eq!(a.esle("   "), Eslesme::Yok);
    assert_eq!(a.esle("[unk]"), Eslesme::Yok);
    assert_eq!(a.esle("atlas"), Eslesme::Yok);
    assert_eq!(a.esle("atlas [unk]"), Eslesme::Yok);
    assert_eq!(a.esle("ekranı"), Eslesme::Yok);
    // Tetik başka bir kelimenin ön eki olduğunda tetik sayılmaz.
    assert_eq!(a.esle("atlasekranı kapat"), Eslesme::Yok);
}

#[test]
fn eslemede_turkce_buyuk_kucuk_harf() {
    let a = coz_ok(ORNEK);
    assert_eq!(a.esle("ATLAS EKRANI KAPAT"), Eslesme::Komut(&a.komutlar[0]));
    assert_eq!(a.esle("  Atlas   Hepsini  Kapat "), Eslesme::Komut(&a.komutlar[1]));
    // Harf çevirisini sınar; tetik mantığı karışmasın diye zararsız bir eylem kullanılır.
    let ik = coz_ok(&kok("[[komut]]\nifadeler = [\"ışık aç\", \"işık kapat\"]\neylem = \"ekranlari_ac\""));
    assert_eq!(ik.esle("IŞIK AÇ"), Eslesme::Komut(&ik.komutlar[0]));
    assert_eq!(ik.esle("İŞIK KAPAT"), Eslesme::Komut(&ik.komutlar[1]));
    let tek = coz_ok(&kok("[[komut]]\nifadeler = [\"ışık\"]\neylem = \"cik\""));
    assert_eq!(tek.esle("IŞIK"), Eslesme::TetikGerekli(&tek.komutlar[0]));
    assert_eq!(tek.esle("ATLAS IŞIK"), Eslesme::Komut(&tek.komutlar[0]));
}

#[test]
fn varsayilan_dosyada_esleme() {
    let a = coz_ok(Ayarlar::varsayilan_toml());
    assert!(matches!(a.esle("krom aç"), Eslesme::Komut(k) if k.gosterim == "Chrome aç"));
    assert!(matches!(a.esle("ekranları kapat"), Eslesme::TetikGerekli(_)));
    assert!(matches!(a.esle("atlas ekranları kapat"), Eslesme::Komut(_)));
}

// --- yükleme ---

#[test]
fn yukle_olmayan_dosyada_yol_iceren_hata_verir() {
    let yol = Path::new("yok_boyle_bir_klasor/ayar.toml");
    let h = Ayarlar::yukle(yol).unwrap_err();
    assert!(h.contains("yok_boyle_bir_klasor"), "{h}");
    assert!(h.contains("ayar.toml"), "{h}");
}

#[test]
fn yukle_gecerli_dosyayi_okur_gecersizde_yolu_soyler() {
    let klasor = std::env::temp_dir().join(format!("komut_test_{}", std::process::id()));
    std::fs::create_dir_all(&klasor).unwrap();
    let iyi = klasor.join("iyi.toml");
    std::fs::write(&iyi, Ayarlar::varsayilan_toml()).unwrap();
    assert_eq!(Ayarlar::yukle(&iyi).unwrap().komutlar.len(), VARSAYILAN_KOMUT_SAYISI);
    let kotu = klasor.join("kotu.toml");
    std::fs::write(&kotu, "tetik = \"\"").unwrap();
    let h = Ayarlar::yukle(&kotu).unwrap_err();
    assert!(h.contains("kotu.toml"), "{h}");
    let _ = std::fs::remove_dir_all(&klasor);
}
