//! v0.2 şema genişlemesinin testleri: kısayollar, dinleme kipi, çift alkış, zamanlı komutlar.
//! (Kısayol ayrıştırıcısının kendi testleri `kisayol.rs` içindedir.)

use crate::{
    Ayarlar, DinlemeKipi, Eslesme, Eylem, HamAyarlar, HamKisayol, MOD_ALT, MOD_CONTROL,
};

fn coz_ok(metin: &str) -> Ayarlar {
    Ayarlar::coz(metin).unwrap_or_else(|h| panic!("beklenmedik hata: {h}"))
}

fn hata(metin: &str) -> String {
    Ayarlar::coz(metin).expect_err("hata bekleniyordu")
}

const KOMUTLAR: &str = "[[uygulama]]\nsoylenis = \"krom\"\nad = \"Chrome\"\nhedef = \"chrome\"\n\
    [[sahne]]\nad = \"İş\"\nsoylenis = \"iş\"\nadimlar = [ { bekle_ms = 1 } ]\n\
    [[komut]]\nifadeler = [\"krom aç\", \"sesi aç\"]\neylem = \"sesi_ac\"\n\
    [[komut]]\nifadeler = [\"ekranları kapat\"]\neylem = \"ekranlari_kapat\"\n\
    [[komut]]\nifadeler = [\"sesi yüzde {sayi} yap\"]\neylem = \"ses_ayarla\"\n";

/// `üst` genel anahtarlar, `tablo` tablolar ([alkis], [[kisayol]] ...) — komutlardan önce gelir.
fn dosya(ust: &str, tablo: &str) -> String {
    format!("tetik = \"atlas\"\n{ust}\n{tablo}\n{KOMUTLAR}")
}

// --- dinleme kipi ve tuşlar ---

#[test]
fn dinleme_varsayilanlari() {
    let a = coz_ok(&dosya("", ""));
    assert_eq!(a.dinleme_kipi, DinlemeKipi::Surekli);
    assert_eq!(a.dinleme_tusu, None);
    assert_eq!(a.palet_tusu, None);
    assert_eq!(a.dinleme_suresi_sn, 6);
    assert!(a.kisayollar.is_empty());
    assert!(!a.alkis.acik);
    assert_eq!(a.alkis.hassasiyet, 0.6);
    assert_eq!(a.alkis.ifade, "");
}

#[test]
fn dinleme_tusu_palet_tusu_ve_sure() {
    let a = coz_ok(&dosya(
        "dinleme_kipi = \"tusla\"\ndinleme_tusu = \"Kontrol+ALT+Boşluk\"\ndinleme_suresi_sn = 12\npalet_tusu = \"ctrl+alt+p\"",
        "",
    ));
    assert_eq!(a.dinleme_kipi, DinlemeKipi::Tusla);
    let t = a.dinleme_tusu.as_ref().unwrap();
    assert_eq!((t.degistirici, t.vk, t.metin.as_str()), (MOD_CONTROL | MOD_ALT, 0x20, "ctrl+alt+space"));
    assert_eq!(a.palet_tusu.as_ref().unwrap().vk, 0x50);
    assert_eq!(a.dinleme_suresi_sn, 12);
    // Sürekli kipte de dinleme tuşu yazılabilir.
    let b = coz_ok(&dosya("dinleme_tusu = \"ctrl+alt+d\"", ""));
    assert_eq!(b.dinleme_kipi, DinlemeKipi::Surekli);
    assert!(b.dinleme_tusu.is_some());
}

#[test]
fn dinleme_suresi_sinirlari() {
    for iyi in [2, 6, 30] {
        assert_eq!(coz_ok(&dosya(&format!("dinleme_suresi_sn = {iyi}"), "")).dinleme_suresi_sn, iyi);
    }
    for kotu in [0, 1, 31, 600] {
        let h = hata(&dosya(&format!("dinleme_suresi_sn = {kotu}"), ""));
        assert!(h.contains("dinleme_suresi_sn"), "{kotu}: {h}");
    }
}

#[test]
fn tusla_kipi_dinleme_tusu_ister() {
    let h = hata(&dosya("dinleme_kipi = \"tusla\"", ""));
    assert!(h.contains("dinleme_tusu") && h.contains("gerekli"), "{h}");
    assert!(Ayarlar::coz(&dosya("dinleme_kipi = \"tusla\"\ndinleme_tusu = \"ctrl+alt+d\"", "")).is_ok());
    // Geçersiz kip adı reddedilir.
    assert!(Ayarlar::coz(&dosya("dinleme_kipi = \"surekli_dinle\"", "")).is_err());
}

#[test]
fn gecersiz_tuslar_alan_adini_soyler() {
    let h = hata(&dosya("dinleme_tusu = \"a\"", ""));
    assert!(h.contains("dinleme_tusu"), "{h}");
    let h = hata(&dosya("palet_tusu = \"ctrl+alt\"", ""));
    assert!(h.contains("palet_tusu"), "{h}");
    let h = hata(&dosya("", "[[kisayol]]\ntuslar = \"win+l\"\nifade = \"krom aç\""));
    assert!(h.contains("kisayol #1") && h.contains("ayrılmış"), "{h}");
}

// --- [[kisayol]] ---

#[test]
fn kisayol_ifadeyi_komutla_eslestirir() {
    let a = coz_ok(&dosya(
        "",
        "[[kisayol]]\ntuslar = \"ctrl+alt+1\"\nifade = \"Krom  Aç\"\n\
         [[kisayol]]\ntuslar = \"win+shift+f5\"\nifade = \"sesi yüzde elli yap\"\n\
         [[kisayol]]\ntuslar = \"ctrl+alt+3\"\nifade = \"ekranları kapat\"",
    ));
    assert_eq!(a.kisayollar.len(), 3);
    assert_eq!(a.kisayollar[0].ifade, "krom aç");
    assert_eq!(a.kisayollar[0].kisayol.metin, "ctrl+alt+1");
    assert_eq!(a.kisayollar[1].ifade, "sesi yüzde elli yap");
    assert_eq!(a.kisayollar[1].kisayol.metin, "shift+win+f5");
    // Şablonlu komutun açılmış ifadesi ve riskli komut da olur.
    let k = a.komut_bul(&a.kisayollar[1].ifade).unwrap();
    assert_eq!(k.eylem, Eylem::SesAyarla(50));
    assert!(a.komut_bul(&a.kisayollar[2].ifade).unwrap().riskli);
}

#[test]
fn kisayol_bilinmeyen_ifade_ve_tetikli_ifade_reddedilir() {
    let h = hata(&dosya("", "[[kisayol]]\ntuslar = \"ctrl+alt+1\"\nifade = \"var olmayan komut\""));
    assert!(h.contains("kisayol #1") && h.contains("var olmayan komut"), "{h}");
    // Tetikli yazım birebir eşleşmez (tetiksiz çalışır).
    let h = hata(&dosya("", "[[kisayol]]\ntuslar = \"ctrl+alt+1\"\nifade = \"atlas ekranları kapat\""));
    assert!(h.contains("eşleşmiyor"), "{h}");
    // Şablon yer tutucusu açılmış biçim değildir.
    assert!(Ayarlar::coz(&dosya("", "[[kisayol]]\ntuslar = \"ctrl+alt+1\"\nifade = \"sesi yüzde {sayi} yap\"")).is_err());
    let h = hata(&dosya("", "[[kisayol]]\ntuslar = \"ctrl+alt+1\"\nifade = \"   \""));
    assert!(h.contains("boş"), "{h}");
}

#[test]
fn kisayol_cakismasi_kanonik_metne_gore() {
    // Aynı kısayol iki farklı yazımla.
    let h = hata(&dosya(
        "",
        "[[kisayol]]\ntuslar = \"Ctrl+Alt+1\"\nifade = \"krom aç\"\n\
         [[kisayol]]\ntuslar = \"alt+kontrol+1\"\nifade = \"sesi aç\"",
    ));
    assert!(h.contains("çakışma") && h.contains("kisayol #1") && h.contains("kisayol #2"), "{h}");
    // dinleme_tusu ile kısayol.
    let h = hata(&dosya(
        "dinleme_tusu = \"ctrl+alt+space\"",
        "[[kisayol]]\ntuslar = \"kontrol+alt+boşluk\"\nifade = \"krom aç\"",
    ));
    assert!(h.contains("dinleme_tusu") && h.contains("kisayol #1"), "{h}");
    // palet_tusu ile dinleme_tusu.
    let h = hata(&dosya("dinleme_tusu = \"ctrl+alt+p\"\npalet_tusu = \"ALT+CTRL+P\"", ""));
    assert!(h.contains("çakışma"), "{h}");
    // Farklı kısayollar çakışmaz.
    assert!(Ayarlar::coz(&dosya(
        "dinleme_tusu = \"ctrl+alt+d\"\npalet_tusu = \"ctrl+alt+p\"",
        "[[kisayol]]\ntuslar = \"ctrl+alt+1\"\nifade = \"krom aç\"\n[[kisayol]]\ntuslar = \"ctrl+alt+2\"\nifade = \"krom aç\"",
    ))
    .is_ok());
}

#[test]
fn kisayol_bilinmeyen_alan_reddi() {
    assert!(Ayarlar::coz(&dosya("", "[[kisayol]]\ntuslar = \"ctrl+alt+1\"\nifade = \"krom aç\"\nrisk = true")).is_err());
    assert!(Ayarlar::coz(&dosya("", "[[kisayol]]\ntuslar = \"ctrl+alt+1\"")).is_err());
    assert!(Ayarlar::coz(&dosya("", "[alkis]\nacik = true\nifade = \"sesi aç\"\nhassasyet = 0.5")).is_err());
}

// --- çift alkış ---

#[test]
fn alkis_dogrulamasi() {
    let a = coz_ok(&dosya("", "[alkis]\nacik = true\nifade = \"Ekranları  KAPAT\"\nhassasiyet = 0.8"));
    assert!(a.alkis.acik);
    assert_eq!(a.alkis.ifade, "ekranları kapat");
    assert!((a.alkis.hassasiyet - 0.8).abs() < 1e-6);
    // Alkış riskli komutu tetiksiz çalıştırabilir: komut riskli kalır, ama ifade bulunur.
    assert!(a.komut_bul(&a.alkis.ifade).unwrap().riskli);

    // Açıkken ifade zorunlu ve bir komutla eşleşmeli.
    assert!(hata(&dosya("", "[alkis]\nacik = true")).contains("alkis.ifade"));
    assert!(hata(&dosya("", "[alkis]\nacik = true\nifade = \"yok boyle komut\"")).contains("eşleşmiyor"));
    // Hassasiyet sınırları (kapalıyken de).
    for kotu in ["-0.1", "1.1", "7"] {
        let h = hata(&dosya("", &format!("[alkis]\nacik = false\nhassasiyet = {kotu}")));
        assert!(h.contains("hassasiyet"), "{kotu}: {h}");
    }
    for iyi in ["0.0", "1.0", "0.5"] {
        assert!(Ayarlar::coz(&dosya("", &format!("[alkis]\nhassasiyet = {iyi}"))).is_ok());
    }
}

#[test]
fn alkis_kapaliyken_ifade_eslesmesi_aranmaz_ama_bicim_denetlenir() {
    let a = coz_ok(&dosya("", "[alkis]\nacik = false\nifade = \"hiçbir komut bu değil\""));
    assert!(!a.alkis.acik);
    assert_eq!(a.alkis.ifade, "hiçbir komut bu değil");
    // Yazılmışsa biçimi denetlenir (yalnız harf ve boşluk).
    let h = hata(&dosya("", "[alkis]\nacik = false\nifade = \"ekran 1\""));
    assert!(h.contains("alkis.ifade"), "{h}");
}

// --- komut_bul ---

#[test]
fn komut_bul_duzler_ve_tetikliyi_bulmaz() {
    let a = coz_ok(&dosya("", ""));
    assert_eq!(a.komut_bul("  KROM   Aç ").unwrap().ifade, "krom aç");
    assert_eq!(a.komut_bul("sesi yüzde yirmi beş yap").unwrap().eylem, Eylem::SesAyarla(25));
    assert!(a.komut_bul("atlas krom aç").is_none());
    assert!(a.komut_bul("yok").is_none());
    assert!(a.komut_bul("").is_none());
    // Riskli komut tetiksiz bulunur; `esle` ise tetik ister.
    assert!(a.komut_bul("ekranları kapat").is_some());
    assert!(matches!(a.esle("ekranları kapat"), Eslesme::TetikGerekli(_)));
}

// --- zamanlı komutlar ---

fn zamanli_dosya(satir: &str) -> String {
    format!(
        "tetik = \"atlas\"\n[[uygulama]]\nsoylenis = \"krom\"\nad = \"Chrome\"\nhedef = \"chrome\"\n\
         [[sahne]]\nad = \"İş\"\nsoylenis = \"iş\"\nadimlar = [ {{ bekle_ms = 1 }} ]\n{satir}"
    )
}

#[test]
fn zamanli_acilim_sayisi_gosterim_ve_eylem() {
    let a = coz_ok(&zamanli_dosya(
        "[[komut]]\nifadeler = [\"bilgisayarı uyut\", \"uykuya yatır\"]\neylem = \"uyut\"\nzamanlanabilir = true\n",
    ));
    // 2 ifade × (1 + 9 zamanlı).
    assert_eq!(a.komutlar.len(), 20);
    let k = a.komut_bul("beş dakika sonra bilgisayarı uyut").expect("zamanlı ifade");
    assert_eq!(k.gosterim, "beş dakika sonra: bilgisayarı uyut");
    assert_eq!(
        k.eylem,
        Eylem::Zamanla { dakika: 5, eylem: Box::new(Eylem::Uyut), gosterim: "bilgisayarı uyut".into() }
    );
    assert!(k.riskli, "asıl komut riskli (uyut)");
    assert_eq!(k.efekt, None);
    // Süre listesinin tamamı.
    for (soz, dk) in [
        ("beş dakika", 5), ("on dakika", 10), ("on beş dakika", 15), ("yirmi dakika", 20),
        ("otuz dakika", 30), ("kırk beş dakika", 45), ("yarım saat", 30), ("bir saat", 60),
        ("iki saat", 120),
    ] {
        let k = a.komut_bul(&format!("{soz} sonra uykuya yatır")).unwrap_or_else(|| panic!("{soz}"));
        match &k.eylem {
            Eylem::Zamanla { dakika, eylem, .. } => {
                assert_eq!(*dakika, dk, "{soz}");
                assert_eq!(**eylem, Eylem::Uyut);
            }
            e => panic!("{e:?}"),
        }
    }
    // Zamanlı ifade tetiksiz riskli: tetik ister.
    assert!(matches!(a.esle("beş dakika sonra bilgisayarı uyut"), Eslesme::TetikGerekli(_)));
    assert!(matches!(a.esle("atlas beş dakika sonra bilgisayarı uyut"), Eslesme::Komut(_)));
    // Efekt eyleme göre: Sonar.
    let (e, _) = k.efekt_coz(&a, 0);
    assert_eq!(e, crate::efekt::Efekt::Sonar);
}

#[test]
fn zamanli_acilim_riski_asil_komuttan_gelir() {
    let a = coz_ok(&zamanli_dosya(
        "[[komut]]\nifadeler = [\"sessize al\"]\neylem = \"sessize_al\"\nzamanlanabilir = true\n\
         [[komut]]\nifadeler = [\"ekranı aç\"]\neylem = \"ekranlari_ac\"\nriskli = true\nzamanlanabilir = true\n",
    ));
    let rahat = a.komut_bul("on dakika sonra sessize al").unwrap();
    assert!(!rahat.riskli);
    assert!(matches!(a.esle("on dakika sonra sessize al"), Eslesme::Komut(_)));
    // Açıkça riskli yazılan eylem zamanlıda da riskli.
    assert!(a.komut_bul("on dakika sonra ekranı aç").unwrap().riskli);
}

#[test]
fn zamanli_ekran_sablonuyla() {
    let a = coz_ok(&zamanli_dosya(
        "[[komut]]\nifadeler = [\"{ekran} ekranı kapat\"]\neylem = \"ekrani_kapat\"\nzamanlanabilir = true\n",
    ));
    // 4 ekran × (1 + 9).
    assert_eq!(a.komutlar.len(), 40);
    let k = a.komut_bul("yarım saat sonra sağ ekranı kapat").unwrap();
    assert_eq!(k.gosterim, "yarım saat sonra: sağ ekranı kapat");
    assert!(k.riskli);
    assert_eq!(
        k.eylem,
        Eylem::Zamanla {
            dakika: 30,
            eylem: Box::new(Eylem::EkraniKapat(crate::EkranSecimi::Sag)),
            gosterim: "sağ ekranı kapat".into(),
        }
    );
    // Sabit ekranlı ekrani_kapat da zamanlanabilir.
    let b = coz_ok(&zamanli_dosya(
        "[[komut]]\nifadeler = [\"sol perdeyi indir\"]\neylem = \"ekrani_kapat\"\nekran = \"sol\"\nzamanlanabilir = true\n",
    ));
    assert_eq!(b.komutlar.len(), 10);
}

#[test]
fn zamanlanabilir_yasak_sablonlar_ve_eylemler() {
    for (eylem, ifade) in [
        ("ses_ayarla", "sesi yüzde {sayi} yap"),
        ("uygulama_ac", "{uygulama} aç"),
        ("uygulama_kapat", "{uygulama} kapat"),
        ("sahne", "{sahne} modu"),
    ] {
        let h = hata(&zamanli_dosya(&format!(
            "[[komut]]\nifadeler = [\"{ifade}\"]\neylem = \"{eylem}\"\nzamanlanabilir = true\n"
        )));
        assert!(h.contains("zamanlanabilir") && h.contains("komut #1"), "{eylem}: {h}");
    }
    let h = hata(&zamanli_dosya(
        "[[komut]]\nifadeler = [\"zamanlayıcı iptal\"]\neylem = \"zamanlayicilari_iptal\"\nzamanlanabilir = true\n",
    ));
    assert!(h.contains("zamanlanamaz"), "{h}");
    // false yazmak her şablonda serbest.
    assert!(Ayarlar::coz(&zamanli_dosya(
        "[[komut]]\nifadeler = [\"sesi yüzde {sayi} yap\"]\neylem = \"ses_ayarla\"\nzamanlanabilir = false\n"
    ))
    .is_ok());
}

#[test]
fn zamanli_ifade_yinelenirse_hata() {
    // İkinci komutun yalın ifadesi birincinin zamanlı açılımıyla aynı.
    let h = hata(&zamanli_dosya(
        "[[komut]]\nifadeler = [\"sessize al\"]\neylem = \"sessize_al\"\nzamanlanabilir = true\n\
         [[komut]]\nifadeler = [\"beş dakika sonra sessize al\"]\neylem = \"sesi_ac\"\n",
    ));
    assert!(h.contains("yinelenen"), "{h}");
}

#[test]
fn zamanla_eylemi_sahne_adiminda_yasak() {
    let h = hata(&zamanli_dosya(
        "[[sahne]]\nad = \"Z\"\nsoylenis = \"zaman\"\nadimlar = [ { eylem = \"zamanla\" } ]\n\
         [[komut]]\nifadeler = [\"ekranı aç\"]\neylem = \"ekranlari_ac\"\n",
    ));
    assert!(h.contains("zamanla") && h.contains("bilinmeyen eylem"), "{h}");
    // Komut eylemi olarak da yazılamaz (yalnız zamanlanabilir ile üretilir).
    let h = hata(&zamanli_dosya("[[komut]]\nifadeler = [\"ekranı aç\"]\neylem = \"zamanla\"\n"));
    assert!(h.contains("bilinmeyen eylem"), "{h}");
}

#[test]
fn zamanlayicilari_iptal_eylemi() {
    let a = coz_ok(&zamanli_dosya(
        "[[komut]]\nifadeler = [\"zamanlayıcı iptal\", \"geri sayım iptal\"]\neylem = \"zamanlayicilari_iptal\"\n",
    ));
    assert_eq!(a.komutlar.len(), 2);
    for k in &a.komutlar {
        assert_eq!(k.eylem, Eylem::ZamanlayicilariIptal);
        assert!(!k.riskli);
    }
    assert!(crate::eylem_adlari().iter().any(|(a, g)| *a == "zamanlayicilari_iptal" && !g.is_empty()));
    assert!(crate::eylem_alanlari("zamanlayicilari_iptal").is_empty());
    assert!(matches!(a.esle("geri sayım iptal"), Eslesme::Komut(_)));
}

// --- varsayılan dosya ---

#[test]
fn varsayilan_dosya_v02_alanlari() {
    let a = coz_ok(Ayarlar::varsayilan_toml());
    assert_eq!(a.dinleme_kipi, DinlemeKipi::Surekli);
    assert_eq!(a.dinleme_tusu.as_ref().unwrap().metin, "alt+win+space");
    assert_eq!(a.palet_tusu.as_ref().unwrap().metin, "ctrl+alt+shift+p");
    assert_eq!(a.dinleme_suresi_sn, 6);
    assert!(a.kisayollar.is_empty());
    assert!(!a.alkis.acik);
    assert_eq!(a.alkis.hassasiyet, 0.6);

    for (ifade, dakika) in [
        ("beş dakika sonra bilgisayarı uyut", 5),
        ("yarım saat sonra bilgisayarı kitle", 30),
        ("iki saat sonra ekranları kapat", 120),
        ("on dakika sonra müziği durdur", 10),
        ("kırk beş dakika sonra sessize al", 45),
        ("bir saat sonra hepsini kapat", 60),
    ] {
        let k = a.komut_bul(ifade).unwrap_or_else(|| panic!("{ifade}"));
        assert!(matches!(&k.eylem, Eylem::Zamanla { dakika: d, .. } if *d == dakika), "{ifade}");
    }
    assert!(a.komut_bul("beş dakika sonra ekranları kapat").unwrap().riskli);
    assert!(!a.komut_bul("beş dakika sonra sessize al").unwrap().riskli);
    assert!(!a.komut_bul("beş dakika sonra müziği durdur").unwrap().riskli);
    assert_eq!(a.komut_bul("zamanlayıcı iptal").unwrap().eylem, Eylem::ZamanlayicilariIptal);
    assert_eq!(a.komut_bul("geri sayım iptal").unwrap().eylem, Eylem::ZamanlayicilariIptal);
    // Şablonlu komutlara zamanlama eklenmemiş.
    assert!(a.komut_bul("beş dakika sonra krom aç").is_none());
    assert!(a.komut_bul("beş dakika sonra sesi yüzde elli yap").is_none());
    // Beş zamanlanabilir satır: 3+1+1+1+2+2 ... = ifade sayısı × 9 zamanlı.
    let zamanli = a.komutlar.iter().filter(|k| matches!(k.eylem, Eylem::Zamanla { .. })).count();
    assert_eq!(zamanli, 9 * (3 + 2 + 1 + 1 + 2));
}

// --- ham şema ---

#[test]
fn ham_v02_gidis_donus_ve_varsayilanlar() {
    let d = HamAyarlar::default();
    assert_eq!(d.dinleme_kipi, DinlemeKipi::Surekli);
    assert_eq!(d.dinleme_tusu, None);
    assert_eq!(d.dinleme_suresi_sn, 6);
    assert_eq!(d.palet_tusu, None);
    assert!(d.kisayol.is_empty());
    assert!(!d.alkis.acik);
    assert_eq!(d.alkis.hassasiyet, 0.6);
    assert_eq!(HamAyarlar::coz(&d.toml_yaz()).unwrap(), d);

    let mut ham = HamAyarlar::coz(&dosya(
        "dinleme_kipi = \"tusla\"\ndinleme_tusu = \"ctrl+alt+d\"\ndinleme_suresi_sn = 9\npalet_tusu = \"ctrl+alt+p\"",
        "[alkis]\nacik = true\nifade = \"sesi aç\"\nhassasiyet = 0.3\n\
         [[kisayol]]\ntuslar = \"ctrl+alt+1\"\nifade = \"krom aç\"",
    ))
    .unwrap();
    ham.kisayol.push(HamKisayol { tuslar: "ctrl+alt+2".into(), ifade: "ekranları kapat".into() });
    let yazilan = ham.toml_yaz_dene().unwrap();
    let geri = HamAyarlar::coz(&yazilan).unwrap_or_else(|h| panic!("{h}\n{yazilan}"));
    assert_eq!(geri, ham, "{yazilan}");
    assert_eq!(Ayarlar::hamdan(&geri).unwrap(), Ayarlar::hamdan(&ham).unwrap());
    assert_eq!(geri.toml_yaz(), ham.toml_yaz());
    let a = Ayarlar::hamdan(&ham).unwrap();
    assert_eq!(a.dinleme_kipi, DinlemeKipi::Tusla);
    assert_eq!(a.kisayollar.len(), 2);
    assert!(a.alkis.acik);
}

#[test]
fn ham_zamanlanabilir_yalniz_true_iken_yazilir() {
    let mut ham = HamAyarlar::coz(&zamanli_dosya(
        "[[komut]]\nifadeler = [\"sessize al\"]\neylem = \"sessize_al\"\nzamanlanabilir = true\n\
         [[komut]]\nifadeler = [\"sesi aç\"]\neylem = \"sesi_ac\"\n",
    ))
    .unwrap();
    assert!(ham.komut[0].zamanlanabilir && !ham.komut[1].zamanlanabilir);
    let yazilan = ham.toml_yaz();
    assert_eq!(yazilan.matches("zamanlanabilir").count(), 1, "{yazilan}");
    assert_eq!(HamAyarlar::coz(&yazilan).unwrap(), ham);
    ham.komut[1].zamanlanabilir = true;
    assert_eq!(ham.toml_yaz().matches("zamanlanabilir").count(), 2);
}
