//! `[yakinlik]` bölümünün testleri: varsayılanlar, sınırlar, efekt adı, gidiş-dönüş.

use crate::efekt::Efekt;
use crate::{Ayarlar, HamAyarlar, YakinlikAyari};

fn coz_ok(metin: &str) -> Ayarlar {
    Ayarlar::coz(metin).unwrap_or_else(|h| panic!("beklenmedik hata: {h}"))
}

fn hata(metin: &str) -> String {
    Ayarlar::coz(metin).expect_err("hata bekleniyordu")
}

const KOMUTLAR: &str = "[[komut]]\nifadeler = [\"sesi aç\"]\neylem = \"sesi_ac\"\n";

fn dosya(yakinlik: &str) -> String {
    format!("tetik = \"atlas\"\n{yakinlik}\n{KOMUTLAR}")
}

fn varsayilanlar_dogru(y: &YakinlikAyari) {
    assert!(!y.acik);
    assert_eq!((y.bekleme_sn, y.sayac_sn), (120, 10));
    assert!(y.oyun_kolu && y.mikrofon && y.tam_ekran);
    assert!(!y.medya_caliyorken);
    assert!(y.medya_duraklat && y.sensor_izle && y.karsilama);
    assert_eq!(y.karsilama_efekti, Some(Efekt::Aurora));
}

#[test]
fn varsayilanlar_ve_bolumsuz_dosya() {
    varsayilanlar_dogru(&YakinlikAyari::default());
    varsayilanlar_dogru(&coz_ok(&dosya("")).yakinlik);
    varsayilanlar_dogru(&coz_ok(&dosya("[yakinlik]")).yakinlik);
    let ham = HamAyarlar::default();
    assert!(!ham.yakinlik.acik);
    assert_eq!(ham.yakinlik.karsilama_efekti, "aurora");
}

#[test]
fn kismi_bolum_digerlerini_varsayilan_birakir() {
    let y = coz_ok(&dosya("[yakinlik]\nacik = true\nbekleme_sn = 300")).yakinlik;
    assert!(y.acik);
    assert_eq!(y.bekleme_sn, 300);
    assert_eq!(y.sayac_sn, 10);
    assert!(y.medya_duraklat);
}

#[test]
fn bilinmeyen_alan_reddedilir() {
    assert!(Ayarlar::coz(&dosya("[yakinlik]\nbeklme_sn = 60")).is_err());
}

#[test]
fn bekleme_sn_sinirlari() {
    for iyi in [30, 3600] {
        let y = coz_ok(&dosya(&format!("[yakinlik]\nbekleme_sn = {iyi}"))).yakinlik;
        assert_eq!(y.bekleme_sn, iyi);
    }
    for kotu in [29, 3601] {
        let h = hata(&dosya(&format!("[yakinlik]\nbekleme_sn = {kotu}")));
        assert!(h.contains("yakinlik.bekleme_sn") && h.contains("30 ile 3600"), "{h}");
    }
}

#[test]
fn sayac_sn_sinirlari() {
    for iyi in [3, 60] {
        let y = coz_ok(&dosya(&format!("[yakinlik]\nsayac_sn = {iyi}"))).yakinlik;
        assert_eq!(y.sayac_sn, iyi);
    }
    for kotu in [2, 61] {
        let h = hata(&dosya(&format!("[yakinlik]\nsayac_sn = {kotu}")));
        assert!(h.contains("yakinlik.sayac_sn") && h.contains("3 ile 60"), "{h}");
    }
}

#[test]
fn karsilama_efekti_yok_gecerli_ve_gecersiz() {
    let y = coz_ok(&dosya("[yakinlik]\nkarsilama_efekti = \"yok\"")).yakinlik;
    assert_eq!(y.karsilama_efekti, None);
    let y = coz_ok(&dosya("[yakinlik]\nkarsilama_efekti = \"kenar_nabzi\"")).yakinlik;
    assert_eq!(y.karsilama_efekti, Some(Efekt::KenarNabzi));
    for e in Efekt::HEPSI {
        let y = coz_ok(&dosya(&format!("[yakinlik]\nkarsilama_efekti = \"{}\"", e.anahtar()))).yakinlik;
        assert_eq!(y.karsilama_efekti, Some(e));
    }
    for kotu in ["yokk", "", "Aurora", "olmayan_efekt"] {
        let h = hata(&dosya(&format!("[yakinlik]\nkarsilama_efekti = \"{kotu}\"")));
        assert!(h.contains("yakinlik.karsilama_efekti"), "{h}");
    }
}

#[test]
fn gidis_donus_ham_yazim_okuma() {
    let d = HamAyarlar::default();
    assert_eq!(HamAyarlar::coz(&d.toml_yaz()).unwrap(), d);

    let mut ham = HamAyarlar::coz(&dosya("[yakinlik]\nacik = true\nbekleme_sn = 600\nsayac_sn = 20\nmedya_caliyorken = true"))
        .unwrap();
    ham.yakinlik.karsilama_efekti = "yok".into();
    ham.yakinlik.oyun_kolu = false;
    let yazilan = ham.toml_yaz_dene().unwrap();
    let geri = HamAyarlar::coz(&yazilan).unwrap_or_else(|h| panic!("{h}\n{yazilan}"));
    assert_eq!(geri, ham, "{yazilan}");
    let a = Ayarlar::hamdan(&geri).unwrap();
    assert_eq!(a, Ayarlar::hamdan(&ham).unwrap());
    assert!(a.yakinlik.acik && a.yakinlik.medya_caliyorken && !a.yakinlik.oyun_kolu);
    assert_eq!((a.yakinlik.bekleme_sn, a.yakinlik.sayac_sn), (600, 20));
    assert_eq!(a.yakinlik.karsilama_efekti, None);
}

#[test]
fn gomulu_varsayilan_dosya_yakinlik() {
    let a = coz_ok(Ayarlar::varsayilan_toml());
    varsayilanlar_dogru(&a.yakinlik);
    let ham = Ayarlar::ham_coz(Ayarlar::varsayilan_toml()).unwrap();
    assert_eq!(ham.yakinlik, HamAyarlar::default().yakinlik);
}
