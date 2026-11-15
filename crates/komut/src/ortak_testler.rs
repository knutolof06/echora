//! Ortak efekt ayarı (`[efekt_ortak]`), kısmi efekt istisnası (`[efektler.<ad>]`) ve efektleri
//! genel olarak açıp kapatan anahtarların testleri.

use crate::efekt::{Efekt, EfektAyari, EfektEkrani, Olay, Renk};
use crate::{Ayarlar, EfektOrtak, EfektOzel, HamAyarlar};

fn coz_ok(metin: &str) -> Ayarlar {
    Ayarlar::coz(metin).unwrap_or_else(|h| panic!("beklenmedik hata: {h}"))
}

fn hata(metin: &str) -> String {
    Ayarlar::coz(metin).expect_err("hata bekleniyordu")
}

/// Genel ayarları ve tabloları tek komutlu geçerli bir dosyanın önüne koyar.
fn dosya(genel: &str, tablolar: &str) -> String {
    format!(
        "tetik = \"atlas\"\n{genel}\n{tablolar}\n\
         [[komut]]\nifadeler = [\"ekranları kapat\"]\neylem = \"ekranlari_kapat\"\n\
         [[komut]]\nifadeler = [\"ekranları aç\"]\neylem = \"ekranlari_ac\"\n"
    )
}

const KIRMIZI: Renk = Renk::yeni(0xFF, 0, 0);
const YESIL: Renk = Renk::yeni(0, 0xFF, 0);

// --- ortak ayar ---

#[test]
fn ortak_ayar_varsayilani_eski_varsayilanla_ayni() {
    let o = EfektOrtak::default();
    let d = EfektAyari::default();
    assert_eq!((o.renk1, o.renk2), (Renk::yeni(0x4F, 0xC3, 0xF7), Renk::yeni(0xB3, 0x88, 0xFF)));
    assert_eq!((o.renk1, o.renk2, o.siddet, o.ekran, o.sese_tepki), (d.renk1, d.renk2, d.siddet, d.ekran, d.sese_tepki));
    assert_eq!(o.hiz, 1.0);
    // Hız 1.0 ve istisna yokken etkin ayar eski katalog varsayılanının aynısı.
    for e in Efekt::HEPSI {
        assert_eq!(o.efekt_ayari(e, None), EfektAyari::varsayilan(e), "{e:?}");
    }
    // Dosyada hiç yazılmazsa da aynı.
    let a = coz_ok(&dosya("", ""));
    assert_eq!(a.efekt_ortak, o);
    assert!(a.efektler_acik && a.olay_efektleri_acik && a.guncelleme_otomatik);
}

#[test]
fn ortak_ayar_istisnasi_olmayan_tum_efektlere_isler() {
    let a = coz_ok(&dosya(
        "",
        "[efekt_ortak]\nrenk1 = \"#FF0000\"\nrenk2 = \"#00FF00\"\nsiddet = 0.25\nekran = \"imlec\"\nsese_tepki = false\nhiz = 1.0",
    ));
    for e in Efekt::HEPSI {
        let ayar = a.efekt_ayari(e);
        assert_eq!((ayar.renk1, ayar.renk2), (KIRMIZI, YESIL), "{e:?}");
        assert_eq!(ayar.siddet, 0.25);
        assert_eq!(ayar.ekran, EfektEkrani::Imlec);
        assert!(!ayar.sese_tepki);
        assert_eq!(ayar.sure_ms, e.varsayilan_sure_ms());
    }
    // Komutun çözülen efekti de ortak ayarı alır.
    let (e, ayar) = a.komutlar[0].efekt_coz(&a, 0);
    assert_eq!(e, Efekt::TupKapanisi);
    assert_eq!(ayar.renk1, KIRMIZI);
}

#[test]
fn ortak_ayar_dogrulanir() {
    for (alan, h) in [("siddet = 1.5", "siddet"), ("hiz = 0.4", "hiz"), ("hiz = 2.1", "hiz"), ("hiz = nan", "hiz")] {
        let m = hata(&dosya("", &format!("[efekt_ortak]\n{alan}")));
        assert!(m.contains("efekt_ortak") && m.contains(h), "{alan}: {m}");
    }
    for ok in ["hiz = 0.5", "hiz = 2.0", "hiz = 1", "siddet = 0.0"] {
        assert!(Ayarlar::coz(&dosya("", &format!("[efekt_ortak]\n{ok}"))).is_ok(), "{ok}");
    }
    // Bilinmeyen alan ve süre alanı (ortakta süre yok, hız var) reddedilir.
    assert!(Ayarlar::coz(&dosya("", "[efekt_ortak]\nrenkk = \"#FF0000\"")).is_err());
    assert!(Ayarlar::coz(&dosya("", "[efekt_ortak]\nsure_ms = 500")).is_err());
}

// --- hız → süre ---

#[test]
fn hiz_sureyi_boler_ve_kenetlenir() {
    let o = |hiz: f32| EfektOrtak { hiz, ..Default::default() };
    // Aurora 1800 ms.
    assert_eq!(o(1.0).temel_ayar(Efekt::Aurora).sure_ms, 1800);
    assert_eq!(o(2.0).temel_ayar(Efekt::Aurora).sure_ms, 900);
    assert_eq!(o(1.5).temel_ayar(Efekt::Aurora).sure_ms, 1200);
    // Yavaş: 1800 / 0.5 = 3600 → 3000'e kenetlenir.
    assert_eq!(o(0.5).temel_ayar(Efekt::Aurora).sure_ms, 3000);
    // Hızlı: Bozulma 350 / 2 = 175 → 200'e kenetlenir.
    assert_eq!(o(2.0).temel_ayar(Efekt::Bozulma).sure_ms, 200);
    // Yuvarlama: 700 / 1.5 = 466.67 → 467.
    assert_eq!(o(1.5).sure_hesapla(700), 467);
    // Her hızda her efektin süresi geçerli aralıkta.
    for h in [0.5, 0.75, 1.0, 1.25, 2.0] {
        for e in Efekt::HEPSI {
            assert!(o(h).temel_ayar(e).dogrula().is_ok(), "{h} {e:?}");
        }
    }
    // Doğrulanmamış bozuk hız (Stüdyo ara durumu) panik yaratmaz, 1.0 sayılır.
    assert_eq!(o(0.0).sure_hesapla(900), 900);
    assert_eq!(o(f32::NAN).sure_hesapla(900), 900);
}

#[test]
fn hiz_dosyadan_komut_ve_olay_surelerine_uygulanir() {
    let a = coz_ok(&dosya("", "[efekt_ortak]\nhiz = 2.0"));
    assert_eq!(a.efekt_ayari(Efekt::Portal).sure_ms, 550);
    assert_eq!(a.komutlar[0].efekt_coz(&a, 0).1.sure_ms, 350); // tüp kapanışı 700 / 2
    // Olay süresi de hızlanır (tetik gerekli 400 / 2 = 200), renkler olayın kendi renkleri kalır.
    let (e, ayar) = a.olay_efekti(Olay::TetikGerekli).unwrap();
    assert_eq!(e, Efekt::KenarNabzi);
    assert_eq!(ayar.sure_ms, 200);
    assert_eq!(ayar.renk1, Renk::yeni(0xFF, 0xB3, 0x00));
    // Anlaşılmadı 300 / 2 = 150 → 200'e kenetlenir.
    assert_eq!(a.olay_efekti(Olay::Anlasilmadi).unwrap().1.sure_ms, 200);
    let yavas = coz_ok(&dosya("", "[efekt_ortak]\nhiz = 0.5"));
    assert_eq!(yavas.olay_efekti(Olay::Hata).unwrap().1.sure_ms, 800);
    assert_eq!(yavas.olay_efekti(Olay::Hata).unwrap().1.renk1, Renk::yeni(0xFF, 0x52, 0x52));
}

// --- kısmi istisna ---

#[test]
fn kismi_istisna_yazilan_alani_ezer_yazilmayan_ortaktan_gelir() {
    let a = coz_ok(&dosya(
        "",
        "[efekt_ortak]\nrenk1 = \"#FF0000\"\nrenk2 = \"#00FF00\"\nsiddet = 0.4\nekran = \"hepsi\"\nhiz = 2.0\n\
         [efektler.aurora]\nrenk1 = \"#112233\"\n\
         [efektler.sonar]\nsure_ms = 2500\nsese_tepki = false",
    ));
    let aurora = a.efekt_ayari(Efekt::Aurora);
    assert_eq!(aurora.renk1, Renk::yeni(0x11, 0x22, 0x33));
    assert_eq!(aurora.renk2, YESIL);
    assert_eq!(aurora.siddet, 0.4);
    assert_eq!(aurora.ekran, EfektEkrani::Hepsi);
    assert_eq!(aurora.sure_ms, 900); // 1800 / 2: süre yazılmadı, hız uygulanır
    let sonar = a.efekt_ayari(Efekt::Sonar);
    assert_eq!((sonar.renk1, sonar.renk2), (KIRMIZI, YESIL));
    assert_eq!(sonar.sure_ms, 2500); // açık süreye hız uygulanmaz
    assert!(!sonar.sese_tepki);
    // İstisnası olmayan efekt tamamen ortak.
    let portal = a.efekt_ayari(Efekt::Portal);
    assert_eq!((portal.renk1, portal.sure_ms), (KIRMIZI, 550));
    assert_eq!(a.efektler.len(), 2);
    assert_eq!(a.efektler[&Efekt::Aurora].alan_sayisi(), 1);
}

#[test]
fn eski_tam_tablo_okunur_ve_ortak_ayardan_etkilenmez() {
    // Önceki sürümün varsayılan dosyası ve kullanıcı dosyaları tam tablolar içerir.
    let eski = "[efektler.portal]\nrenk1 = \"#4FC3F7\"\nrenk2 = \"#B388FF\"\nsure_ms = 1100\nsiddet = 0.7\nekran = \"ana\"\nsese_tepki = true\n\
                [efektler.tup_kapanisi]\nrenk1 = \"#E0F7FA\"\nrenk2 = \"#00B8D4\"\nsure_ms = 700";
    let a = coz_ok(&dosya("", &format!("[efekt_ortak]\nrenk1 = \"#FF0000\"\nhiz = 2.0\n{eski}")));
    let p = a.efekt_ayari(Efekt::Portal);
    assert_eq!(p, EfektAyari {
        renk1: Renk::yeni(0x4F, 0xC3, 0xF7),
        renk2: Renk::yeni(0xB3, 0x88, 0xFF),
        sure_ms: 1100,
        siddet: 0.7,
        ekran: EfektEkrani::Ana,
        sese_tepki: true,
    });
    assert_eq!(a.efektler[&Efekt::Portal], EfektOzel::tam(p));
    // Eski varsayılanın renk+süre tablosu: renkler ve süre sabit, şiddet/ekran ortaktan.
    let t = a.efekt_ayari(Efekt::TupKapanisi);
    assert_eq!((t.renk1, t.sure_ms), (Renk::yeni(0xE0, 0xF7, 0xFA), 700));
    // Eski dosya: [efekt_ortak] ve yeni anahtarlar yok, hepsi varsayılan.
    let b = coz_ok(&dosya("", eski));
    assert_eq!(b.efekt_ortak, EfektOrtak::default());
    assert!(b.efektler_acik && b.olay_efektleri_acik && b.guncelleme_otomatik);
}

#[test]
fn istisna_tablosu_dogrulanir() {
    assert!(hata(&dosya("", "[efektler.portal]\nsure_ms = 100")).contains("efektler.portal"));
    assert!(hata(&dosya("", "[efektler.portal]\nsure_ms = 3001")).contains("sure_ms"));
    assert!(hata(&dosya("", "[efektler.portal]\nsiddet = -0.1")).contains("siddet"));
    assert!(hata(&dosya("", "[efektler.yok_boyle]\nrenk1 = \"#FF0000\"")).contains("yok_boyle"));
    // Bilinmeyen alan reddi korunur (hiz yalnız ortakta).
    assert!(Ayarlar::coz(&dosya("", "[efektler.portal]\nrenkk = \"#FF0000\"")).is_err());
    assert!(Ayarlar::coz(&dosya("", "[efektler.portal]\nhiz = 1.0")).is_err());
    // Boş tablo geçerli: hiçbir alanı ezmez.
    let a = coz_ok(&dosya("", "[efektler.portal]"));
    assert!(a.efektler[&Efekt::Portal].bos_mu());
    assert_eq!(a.efekt_ayari(Efekt::Portal), EfektAyari::varsayilan(Efekt::Portal));
}

// --- açma/kapatma anahtarları ---

#[test]
fn efektler_kapaliysa_hicbir_efekt_oynamaz() {
    let a = coz_ok(&dosya("efektler_acik = false\n", "[olaylar.duyuldu]\ngoster = true\nefekt = \"nefes\""));
    assert!(!a.efektler_acik);
    assert!(!a.efekt_oynasin_mi());
    for o in [Olay::Duyuldu, Olay::Anlasilmadi, Olay::TetikGerekli, Olay::Hata, Olay::Anlasildi] {
        assert_eq!(a.olay_efekti(o), None, "{o:?}");
    }
    assert!(!a.duyuldu_ipucu);
    // Komutlar yine çözülür (imza değişmedi; oynatıp oynatmamaya çağıran karar verir).
    let (e, ayar) = a.komutlar[0].efekt_coz(&a, 0);
    assert_eq!(e, Efekt::TupKapanisi);
    assert!(ayar.dogrula().is_ok());
    assert_eq!(a.komutlar.len(), 2);
}

#[test]
fn olay_efektleri_kapaliysa_yalniz_olaylar_susar() {
    let a = coz_ok(&dosya("olay_efektleri_acik = false\n", "[olaylar.duyuldu]\ngoster = true\nefekt = \"nefes\""));
    assert!(a.efekt_oynasin_mi());
    assert!(!a.olay_efektleri_acik);
    for o in [Olay::Duyuldu, Olay::Anlasilmadi, Olay::TetikGerekli, Olay::Hata] {
        assert_eq!(a.olay_efekti(o), None, "{o:?}");
    }
    assert!(a.olay_efektleri.duyuldu.goster);
    assert!(!a.duyuldu_ipucu);
    let (e, _) = a.komutlar[1].efekt_coz(&a, 0);
    assert_eq!(e, Efekt::Nefes);
    // İkisi de açıkken olaylar eskisi gibi.
    let b = coz_ok(&dosya("", "[olaylar.duyuldu]\ngoster = true\nefekt = \"nefes\""));
    assert!(b.duyuldu_ipucu);
    assert_eq!(b.olay_efekti(Olay::Duyuldu).unwrap().0, Efekt::Nefes);
    assert!(b.olay_efekti(Olay::Hata).is_some());
}

#[test]
fn guncelleme_anahtari_okunur() {
    assert!(!coz_ok(&dosya("guncelleme_otomatik = false\n", "")).guncelleme_otomatik);
    assert!(coz_ok(&dosya("guncelleme_otomatik = true\n", "")).guncelleme_otomatik);
    assert!(Ayarlar::coz(&dosya("guncelleme_otomatik = 1\n", "")).is_err());
    assert!(Ayarlar::coz(&dosya("efektler_acikk = false\n", "")).is_err());
}

// --- varsayılan dosya ve gidiş-dönüş ---

#[test]
fn varsayilan_dosyada_istisna_yok_ve_ortak_ayar_hepsine_ayni_renkleri_verir() {
    let a = coz_ok(Ayarlar::varsayilan_toml());
    assert!(a.efektler.is_empty());
    assert_eq!(a.efekt_ortak, EfektOrtak::default());
    assert!(a.efektler_acik && a.olay_efektleri_acik && a.guncelleme_otomatik);
    let ilk = a.efekt_ayari(Efekt::Nefes);
    for e in Efekt::HEPSI {
        let ayar = a.efekt_ayari(e);
        assert_eq!((ayar.renk1, ayar.renk2), (ilk.renk1, ilk.renk2), "{e:?}");
        assert_eq!((ayar.siddet, ayar.ekran, ayar.sese_tepki), (ilk.siddet, ilk.ekran, ilk.sese_tepki), "{e:?}");
    }
    assert_eq!(Efekt::HEPSI.len(), 16);
    // Ham dosyada da efekt tablosu yok; ortak tablo ve anahtarlar açıkça yazılı.
    let metin = Ayarlar::varsayilan_toml();
    assert!(!metin.lines().any(|s| s.starts_with("[efektler.")), "efekt başına ön tanım kalmış");
    assert!(metin.lines().any(|s| s == "[efekt_ortak]"));
    for anahtar in ["efektler_acik = true", "olay_efektleri_acik = true", "guncelleme_otomatik = true"] {
        assert!(metin.lines().any(|s| s == anahtar), "{anahtar}");
    }
    assert!(metin.starts_with("# Echora ayar dosyası"));
}

#[test]
fn ham_gidis_donus_yeni_alanlarla() {
    let mut ham = HamAyarlar::coz(&dosya("", "")).unwrap();
    ham.efektler_acik = false;
    ham.olay_efektleri_acik = false;
    ham.guncelleme_otomatik = false;
    ham.efekt_ortak = EfektOrtak { renk1: KIRMIZI, hiz: 0.75, siddet: 0.5, ekran: EfektEkrani::Imlec, ..Default::default() };
    ham.efektler.insert("aurora".into(), EfektOzel { renk2: Some(YESIL), sure_ms: Some(2000), ..Default::default() });
    ham.efektler.insert("portal".into(), EfektOzel::default());
    let yazilan = ham.toml_yaz_dene().unwrap();
    let geri = HamAyarlar::coz(&yazilan).unwrap_or_else(|h| panic!("{h}\n{yazilan}"));
    assert_eq!(geri, ham, "{yazilan}");
    assert_eq!(geri.toml_yaz(), yazilan);
    // Yalnız yazılan alanlar dosyaya çıkar.
    assert!(yazilan.contains("[efektler.aurora]"), "{yazilan}");
    let aurora_bolumu = yazilan.split("[efektler.aurora]").nth(1).unwrap().split("\n[").next().unwrap();
    assert!(aurora_bolumu.contains("renk2") && aurora_bolumu.contains("sure_ms"), "{aurora_bolumu}");
    assert!(!aurora_bolumu.contains("renk1") && !aurora_bolumu.contains("siddet"), "{aurora_bolumu}");
    let a = Ayarlar::hamdan(&geri).unwrap();
    assert_eq!(a, coz_ok(&yazilan));
    assert!(!a.efekt_oynasin_mi());
    assert_eq!(a.efekt_ayari(Efekt::Aurora).renk1, KIRMIZI);
    assert_eq!(a.efekt_ayari(Efekt::Aurora).sure_ms, 2000);
    assert_eq!(a.efekt_ayari(Efekt::Nefes).sure_ms, 800); // 600 / 0.75
    // Varsayılan değerler de yazılır (anahtarlar dosyada görünsün).
    let d = HamAyarlar::default().toml_yaz();
    assert!(d.contains("efektler_acik = true") && d.contains("[efekt_ortak]"), "{d}");
}
