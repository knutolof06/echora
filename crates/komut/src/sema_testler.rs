//! Faz 2-5 şema genişlemesinin testleri: yeni eylemler, şablonlar, sahneler, efektler, olaylar,
//! sessiz saatler, ham şema ve varsayılan dosya.

use crate::efekt::{Efekt, EfektAyari, EfektEkrani, Olay, Renk};
use crate::{
    adim_alanlari, efekt_varsayilani, eylem_adlari, eylem_alanlari, Adim, Ayarlar, EfektKipi,
    EkranSecimi, Eslesme, Eylem, HamAdim, HamAyarlar, HamEkran, HamKomut, HamSahne, HamUygulama,
    MedyaTusu, OlayAyari, YontemTercihi,
};

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

const UYGULAMALAR: &str = "[[uygulama]]\nsoylenis = \"krom\"\nad = \"Chrome\"\nhedef = \"chrome\"\n\
    [[uygulama]]\nsoylenis = \"youtube\"\nad = \"YouTube\"\nhedef = \"https://www.youtube.com\"\n";

const SAHNE_IS: &str = "[[sahne]]\nad = \"İş\"\nsoylenis = \"iş\"\n\
    adimlar = [ { eylem = \"uygulama_ac\", uygulama = \"krom\" }, { bekle_ms = 500 } ]\n";

/// Bir komutlu, uygulama ve sahne içeren ayar metni.
fn ornek_ayar(eylem: &str, ifade: &str, ekstra: &str) -> String {
    kok(&format!(
        "{UYGULAMALAR}{SAHNE_IS}[[komut]]\nifadeler = [\"{ifade}\"]\neylem = \"{eylem}\"\n{ekstra}\n"
    ))
}

/// Verilen adımlarla tek sahneli ayar metni.
fn sahneli(adimlar: &str) -> String {
    kok(&format!(
        "{UYGULAMALAR}[[sahne]]\nad = \"Deneme\"\nsoylenis = \"deneme\"\nadimlar = [ {adimlar} ]\n\
         [[komut]]\nifadeler = [\"ekranı aç\"]\neylem = \"ekranlari_ac\"\n"
    ))
}

/// Genel ayarı (ya da `[tablo]`yu) tek komutlu geçerli bir dosyaya ekler.
fn onde(ek: &str) -> String {
    let komut = "[[komut]]\nifadeler = [\"ekranı aç\"]\neylem = \"ekranlari_ac\"\n";
    if ek.trim_start().starts_with('[') {
        // Tablo ise tetik satırı en üste, komut tablonun ardına.
        kok(&format!("{ek}\n{komut}"))
    } else {
        format!("{ek}\n{}", kok(komut))
    }
}

fn ilk_komut(a: &Ayarlar) -> &crate::Komut {
    &a.komutlar[0]
}

// --- yeni eylemler: tek tablo, 29 eylemin hepsi ---

#[test]
fn butun_eylemler_okunur_ve_varsayilan_riskleri_dogru() {
    let krom = Eylem::UygulamaAc { ad: "Chrome".into(), hedef: "chrome".into(), argumanlar: vec![] };
    // (eylem adı, ifade, ek alanlar, ilk komutun eylemi, varsayılan riskli)
    let satirlar: Vec<(&str, &str, &str, Eylem, bool)> = vec![
        ("ekranlari_kapat", "şunu bunu yap", "", Eylem::EkranlariKapat, true),
        ("ana_ekrani_kapat", "şunu bunu yap", "", Eylem::AnaEkraniKapat, true),
        ("ekrani_kapat", "{ekran} ekranı kapat", "", Eylem::EkraniKapat(EkranSecimi::Ana), true),
        ("ekranlari_ac", "şunu bunu yap", "", Eylem::EkranlariAc, false),
        ("uygulama_ac", "{uygulama} aç", "", krom, false),
        (
            "uygulama_kapat",
            "{uygulama} kapat",
            "",
            Eylem::UygulamaKapat { ad: "Chrome".into(), surec: "chrome".into() },
            true,
        ),
        (
            "uygulama_one_getir",
            "{uygulama} öne getir",
            "",
            Eylem::UygulamaOneGetir { ad: "Chrome".into(), surec: "chrome".into() },
            false,
        ),
        ("ses_ayarla", "sesi yüzde {sayi} yap", "", Eylem::SesAyarla(0), false),
        ("ses_artir", "şunu bunu yap", "", Eylem::SesArtir(10), false),
        ("ses_azalt", "şunu bunu yap", "", Eylem::SesAzalt(10), false),
        ("sessize_al", "şunu bunu yap", "", Eylem::SessizeAl, false),
        ("sesi_ac", "şunu bunu yap", "", Eylem::SesiAc, false),
        ("pencere_kucult", "şunu bunu yap", "", Eylem::PencereKucult, false),
        ("pencere_buyut", "şunu bunu yap", "", Eylem::PencereBuyut, false),
        ("pencere_geri_yukle", "şunu bunu yap", "", Eylem::PencereGeriYukle, false),
        ("pencereyi_tasi", "şunu bunu yap", "", Eylem::PencereyiTasi, false),
        ("masaustunu_goster", "şunu bunu yap", "", Eylem::MasaustunuGoster, false),
        ("oturumu_kilitle", "şunu bunu yap", "", Eylem::OturumuKilitle, true),
        ("uyut", "şunu bunu yap", "", Eylem::Uyut, true),
        ("medya", "şunu bunu yap", "tus = \"durdur\"", Eylem::Medya(MedyaTusu::Durdur), false),
        ("dinlemeyi_duraklat", "şunu bunu yap", "", Eylem::DinlemeyiDuraklat, true),
        ("geri_al", "şunu bunu yap", "", Eylem::GeriAl, false),
        ("zamanlayicilari_iptal", "şunu bunu yap", "", Eylem::ZamanlayicilariIptal, false),
        ("studyoyu_ac", "şunu bunu yap", "", Eylem::StudyoyuAc, false),
        ("ayarlari_yeniden_yukle", "şunu bunu yap", "", Eylem::AyarlariYenidenYukle, false),
        ("cik", "şunu bunu yap", "", Eylem::Cik, true),
        ("sahne", "{sahne} modu", "", Eylem::Sahne { ad: "İş".into() }, true),
        (
            "komut_calistir",
            "şunu bunu yap",
            "komut = \"notepad\"",
            Eylem::KomutCalistir("notepad".into()),
            true,
        ),
        (
            "tus_gonder",
            "şunu bunu yap",
            "tuslar = \"win+d\"",
            Eylem::TusGonder("win+d".into()),
            true,
        ),
    ];
    assert_eq!(satirlar.len(), 29);
    // Tablo, eylem listesinin tamamını kapsıyor.
    let kapsanan: std::collections::HashSet<&str> = satirlar.iter().map(|s| s.0).collect();
    let tum: std::collections::HashSet<&str> = eylem_adlari().iter().map(|(a, _)| *a).collect();
    assert_eq!(kapsanan, tum);

    for (eylem, ifade, ekstra, beklenen, riskli) in satirlar {
        let a = coz_ok(&ornek_ayar(eylem, ifade, ekstra));
        let k = ilk_komut(&a);
        assert_eq!(k.eylem, beklenen, "{eylem}");
        assert_eq!(k.riskli, riskli, "{eylem} riskli");
        assert_eq!(k.efekt, None, "{eylem}");
    }
}

#[test]
fn eylem_listesi_tutarli() {
    let adlar = eylem_adlari();
    assert_eq!(adlar.len(), 29);
    let benzersiz: std::collections::HashSet<_> = adlar.iter().map(|(a, _)| a).collect();
    assert_eq!(benzersiz.len(), 29);
    assert!(adlar.iter().all(|(a, g)| !a.is_empty() && !g.is_empty()));
    assert_eq!(adlar[0].0, "ekranlari_kapat");
}

#[test]
fn eylem_alanlari_ve_adim_alanlari() {
    assert_eq!(eylem_alanlari("medya"), &["tus"]);
    assert_eq!(eylem_alanlari("ekrani_kapat"), &["ekran"]);
    assert_eq!(eylem_alanlari("ses_artir"), &["adim"]);
    assert_eq!(eylem_alanlari("komut_calistir"), &["komut"]);
    assert_eq!(eylem_alanlari("tus_gonder"), &["tuslar"]);
    assert!(eylem_alanlari("ekranlari_kapat").is_empty());
    assert!(eylem_alanlari("yok_boyle").is_empty());
    assert_eq!(adim_alanlari("uygulama_ac"), &["uygulama"]);
    assert_eq!(adim_alanlari("ses_ayarla"), &["seviye"]);
    assert!(adim_alanlari("geri_al").is_empty());
}

#[test]
fn riskli_acikca_yazilirsa_yeni_eylemlerde_de_gecerli() {
    let a = coz_ok(&ornek_ayar("uyut", "şunu bunu yap", "riskli = false"));
    assert!(!ilk_komut(&a).riskli);
    let b = coz_ok(&ornek_ayar("sesi_ac", "şunu bunu yap", "riskli = true"));
    assert!(ilk_komut(&b).riskli);
}

// --- eylem başına alan doğrulaması ---

#[test]
fn yanlis_eyleme_verilen_alan_reddedilir() {
    for (eylem, ekstra, alan) in [
        ("ekranlari_kapat", "tus = \"sonraki\"", "tus"),
        ("ekranlari_kapat", "ekran = \"sol\"", "ekran"),
        ("medya", "tus = \"durdur\"\nadim = 5", "adim"),
        ("ses_artir", "komut = \"x\"", "komut"),
        ("ses_azalt", "tuslar = \"a\"", "tuslar"),
        ("komut_calistir", "komut = \"x\"\ntuslar = \"y\"", "tuslar"),
        ("uyut", "adim = 5", "adim"),
    ] {
        let h = hata(&ornek_ayar(eylem, "şunu bunu yap", ekstra));
        assert!(h.contains(alan) && h.contains(eylem), "{eylem}/{alan}: {h}");
    }
}

#[test]
fn zorunlu_alan_eksikse_hata() {
    for (eylem, alan) in [("medya", "tus"), ("komut_calistir", "komut"), ("tus_gonder", "tuslar")] {
        let h = hata(&ornek_ayar(eylem, "şunu bunu yap", ""));
        assert!(h.contains(alan) && h.contains("gerekli"), "{eylem}: {h}");
    }
}

#[test]
fn medya_tuslari_ve_gecersiz_tus() {
    for (t, beklenen) in [
        ("oynat_duraklat", MedyaTusu::OynatDuraklat),
        ("sonraki", MedyaTusu::Sonraki),
        ("onceki", MedyaTusu::Onceki),
        ("durdur", MedyaTusu::Durdur),
    ] {
        let a = coz_ok(&ornek_ayar("medya", "şunu bunu yap", &format!("tus = \"{t}\"")));
        assert_eq!(ilk_komut(&a).eylem, Eylem::Medya(beklenen));
    }
    assert!(Ayarlar::coz(&ornek_ayar("medya", "şunu bunu yap", "tus = \"patlat\"")).is_err());
}

#[test]
fn ses_adimi_varsayilan_ve_sinirlar() {
    let a = coz_ok(&ornek_ayar("ses_artir", "şunu bunu yap", "adim = 25"));
    assert_eq!(ilk_komut(&a).eylem, Eylem::SesArtir(25));
    for ok in ["1", "100"] {
        assert!(Ayarlar::coz(&ornek_ayar("ses_azalt", "şunu bunu yap", &format!("adim = {ok}"))).is_ok(), "{ok}");
    }
    for kotu in ["0", "101", "-3"] {
        assert!(Ayarlar::coz(&ornek_ayar("ses_artir", "şunu bunu yap", &format!("adim = {kotu}"))).is_err(), "{kotu}");
    }
}

#[test]
fn komut_ve_tus_bos_olamaz_ve_kirpilir() {
    assert!(Ayarlar::coz(&ornek_ayar("komut_calistir", "şunu bunu yap", "komut = \"  \"")).is_err());
    assert!(Ayarlar::coz(&ornek_ayar("tus_gonder", "şunu bunu yap", "tuslar = \"\"")).is_err());
    let a = coz_ok(&ornek_ayar("tus_gonder", "şunu bunu yap", "tuslar = \" ctrl+shift+esc \""));
    assert_eq!(ilk_komut(&a).eylem, Eylem::TusGonder("ctrl+shift+esc".into()));
}

#[test]
fn ekrani_kapat_sabit_ekran_alaniyla() {
    let a = coz_ok(&ornek_ayar("ekrani_kapat", "soldakini kapat", "ekran = \"sol\""));
    assert_eq!(a.komutlar.len(), 1);
    assert_eq!(ilk_komut(&a).eylem, Eylem::EkraniKapat(EkranSecimi::Sol));
    assert!(ilk_komut(&a).riskli);
    for (s, e) in [("ana", EkranSecimi::Ana), ("ikinci", EkranSecimi::Ikinci), ("sag", EkranSecimi::Sag)] {
        let a = coz_ok(&ornek_ayar("ekrani_kapat", "şu ekranı kapat", &format!("ekran = \"{s}\"")));
        assert_eq!(ilk_komut(&a).eylem, Eylem::EkraniKapat(e));
    }
    assert!(Ayarlar::coz(&ornek_ayar("ekrani_kapat", "şu ekranı kapat", "ekran = \"üst\"")).is_err());
    // Alan da şablon da yoksa hangi ekran belli değil.
    let h = hata(&ornek_ayar("ekrani_kapat", "şu ekranı kapat", ""));
    assert!(h.contains("{ekran}"), "{h}");
    // Hem alan hem şablon: şablon kullanılamaz.
    let h = hata(&ornek_ayar("ekrani_kapat", "{ekran} ekranı kapat", "ekran = \"sol\""));
    assert!(h.contains("{ekran}"), "{h}");
}

// --- şablonlar ---

#[test]
fn ekran_sablonu_dort_ifadeye_acilir() {
    let a = coz_ok(&kok("[[komut]]\nifadeler = [\"{ekran} ekranı kapat\"]\neylem = \"ekrani_kapat\""));
    let ifadeler: Vec<_> = a.komutlar.iter().map(|k| k.ifade.as_str()).collect();
    assert_eq!(
        ifadeler,
        ["ana ekranı kapat", "ikinci ekranı kapat", "sol ekranı kapat", "sağ ekranı kapat"]
    );
    assert_eq!(a.komutlar[3].eylem, Eylem::EkraniKapat(EkranSecimi::Sag));
    assert_eq!(a.komutlar[1].eylem, Eylem::EkraniKapat(EkranSecimi::Ikinci));
    assert!(a.komutlar.iter().all(|k| k.riskli));
}

#[test]
fn sayi_sablonu_yuz_bir_ifade_uretir() {
    let a = coz_ok(&kok("[[komut]]\nifadeler = [\"sesi yüzde {sayi} yap\"]\neylem = \"ses_ayarla\""));
    assert_eq!(a.komutlar.len(), 101);
    let k = |i: &str| a.komutlar.iter().find(|k| k.ifade == i).unwrap_or_else(|| panic!("{i}"));
    assert_eq!(k("sesi yüzde sıfır yap").eylem, Eylem::SesAyarla(0));
    assert_eq!(k("sesi yüzde yirmi beş yap").eylem, Eylem::SesAyarla(25));
    assert_eq!(k("sesi yüzde yirmi beş yap").gosterim, "sesi yüzde 25 yap");
    assert_eq!(k("sesi yüzde yüz yap").eylem, Eylem::SesAyarla(100));
    assert!(a.komutlar.iter().all(|k| !k.riskli));
    // Gramer: yalın + tetikli.
    assert_eq!(a.gramer_ifadeleri().len(), 202);
}

#[test]
fn acilmis_tek_kelimelik_sayi_riskli() {
    // "{sayi}" tek başına: "sıfır" ve "yüz" tek kelime, gerisi iki kelime.
    let a = coz_ok(&kok("[[komut]]\nifadeler = [\"{sayi}\"]\neylem = \"ses_ayarla\""));
    let k = |i: &str| a.komutlar.iter().find(|k| k.ifade == i).unwrap();
    assert!(k("sıfır").riskli);
    assert!(k("yüz").riskli);
    assert!(!k("yirmi beş").riskli);
}

#[test]
fn uygulama_kapat_ve_one_getir_sablonu_surece_gore() {
    let m = kok(&format!(
        "{UYGULAMALAR}[[uygulama]]\nsoylenis = \"hesap makinesi\"\nad = \"Hesap\"\nhedef = \"calc\"\nsurec = \"CalculatorApp\"\n\
         [[uygulama]]\nsoylenis = \"yazı\"\nad = \"Yazı\"\nhedef = \"C:\\\\Program Files\\\\X\\\\yazi.EXE\"\n\
         [[uygulama]]\nsoylenis = \"video\"\nad = \"Video\"\nhedef = \"https://v.example\"\nsurec = \"videoapp\"\n\
         [[komut]]\nifadeler = [\"{{uygulama}} kapat\"]\neylem = \"uygulama_kapat\"\n\
         [[komut]]\nifadeler = [\"{{uygulama}} öne getir\"]\neylem = \"uygulama_one_getir\"\n"
    ));
    let a = coz_ok(&m);
    let kapat = a.komutlar.iter().filter(|k| k.ifade.ends_with(" kapat")).count();
    // youtube (web adresi, surec yok) elenir: krom, hesap makinesi, yazı, video.
    assert_eq!(kapat, 4);
    assert_eq!(a.komutlar.iter().filter(|k| k.ifade.ends_with(" getir")).count(), 4);
    assert!(a.komutlar.iter().all(|k| !k.ifade.starts_with("youtube")));
    let eylem = |i: &str| a.komutlar.iter().find(|k| k.ifade == i).unwrap().eylem.clone();
    assert_eq!(eylem("krom kapat"), Eylem::UygulamaKapat { ad: "Chrome".into(), surec: "chrome".into() });
    assert_eq!(
        eylem("hesap makinesi kapat"),
        Eylem::UygulamaKapat { ad: "Hesap".into(), surec: "CalculatorApp".into() }
    );
    // Yoldan türetilen süreç adı: son parça, ".exe" (büyük/küçük harf fark etmez) atılır.
    assert_eq!(eylem("yazı kapat"), Eylem::UygulamaKapat { ad: "Yazı".into(), surec: "yazi".into() });
    assert_eq!(
        eylem("video öne getir"),
        Eylem::UygulamaOneGetir { ad: "Video".into(), surec: "videoapp".into() }
    );
}

#[test]
fn surec_yazilmayan_web_hedefi_yalniz_acilir() {
    let m = kok(&format!(
        "{UYGULAMALAR}[[komut]]\nifadeler = [\"{{uygulama}} aç\"]\neylem = \"uygulama_ac\"\n\
         [[komut]]\nifadeler = [\"{{uygulama}} kapat\"]\neylem = \"uygulama_kapat\"\n"
    ));
    let a = coz_ok(&m);
    assert!(a.komutlar.iter().any(|k| k.ifade == "youtube aç"));
    assert!(!a.komutlar.iter().any(|k| k.ifade == "youtube kapat"));
}

#[test]
fn sahne_sablonu_sahne_soylenisiyle_acilir() {
    let m = kok(&format!(
        "{UYGULAMALAR}{SAHNE_IS}[[sahne]]\nad = \"Gece Modu\"\nsoylenis = \"gece\"\nadimlar = [ {{ eylem = \"sessize_al\" }} ]\n\
         [[komut]]\nifadeler = [\"{{sahne}} modu\"]\neylem = \"sahne\"\n"
    ));
    let a = coz_ok(&m);
    assert_eq!(a.komutlar.len(), 2);
    assert_eq!(a.komutlar[0].ifade, "iş modu");
    assert_eq!(a.komutlar[0].gosterim, "İş modu");
    assert_eq!(a.komutlar[0].eylem, Eylem::Sahne { ad: "İş".into() });
    assert_eq!(a.komutlar[1].ifade, "gece modu");
    assert_eq!(a.komutlar[1].eylem, Eylem::Sahne { ad: "Gece Modu".into() });
    assert!(a.komutlar.iter().all(|k| k.riskli));
    assert_eq!(a.esle("iş modu"), Eslesme::TetikGerekli(&a.komutlar[0]));
    assert_eq!(a.esle("atlas iş modu"), Eslesme::Komut(&a.komutlar[0]));
}

#[test]
fn uyumsuz_sablon_reddedilir() {
    for (eylem, ifade, beklenen) in [
        ("ekranlari_kapat", "{sayi} ekranı kapat", "{sayi}"),
        ("ekranlari_kapat", "{ekran} ekranı kapat", "{ekran}"),
        ("ekranlari_kapat", "{sahne} ekranı kapat", "{sahne}"),
        ("uygulama_ac", "{sayi} aç", "{sayi}"),
        ("uygulama_kapat", "{ekran} kapat", "{ekran}"),
        ("ses_ayarla", "sesi {uygulama} yap", "{uygulama}"),
        ("ses_ayarla", "sesi {ekran} yap", "{ekran}"),
        ("ekrani_kapat", "{sayi} ekranı kapat", "{sayi}"),
        ("sahne", "{uygulama} modu", "{uygulama}"),
        ("medya", "{sayi} şarkı", "{sayi}"),
    ] {
        let ekstra = if eylem == "medya" { "tus = \"sonraki\"" } else { "" };
        let h = hata(&ornek_ayar(eylem, ifade, ekstra));
        assert!(h.contains(beklenen), "{eylem}/{ifade}: {h}");
    }
}

#[test]
fn bilinmeyen_yer_tutucu_ve_yer_tutucu_sayisi() {
    let h = hata(&ornek_ayar("ekranlari_kapat", "{foo} kapat", ""));
    assert!(h.contains("{foo}") && h.contains("bilinmeyen"), "{h}");
    let h = hata(&ornek_ayar("uygulama_ac", "{uygulama} {foo} aç", ""));
    assert!(h.contains("{foo}"), "{h}");
    // İfade başına en çok bir yer tutucu: iki farklı ya da aynı yer tutucu reddedilir.
    assert!(Ayarlar::coz(&ornek_ayar("uygulama_ac", "{uygulama} {sayi} aç", "")).is_err());
    assert!(Ayarlar::coz(&ornek_ayar("ses_ayarla", "{sayi} {sayi}", "")).is_err());
    // Şablonlu eylemde yer tutucu yoksa hata.
    for (e, i) in [("ses_ayarla", "sesi yap"), ("ekrani_kapat", "ekranı kapat"), ("sahne", "iş modu")] {
        assert!(Ayarlar::coz(&ornek_ayar(e, i, "")).is_err(), "{e}");
    }
    // Eşleşmeyen süslü parantez geçersiz karakterdir.
    assert!(Ayarlar::coz(&ornek_ayar("ekranlari_kapat", "şunu { bunu", "")).is_err());
}

#[test]
fn sablon_acilimi_sonrasi_yineleme_tetik_ve_tek_kelime() {
    let m = kok(
        "[[komut]]\nifadeler = [\"{ekran} ekranı kapat\"]\neylem = \"ekrani_kapat\"\n\
         [[komut]]\nifadeler = [\"ana ekranı kapat\"]\neylem = \"ana_ekrani_kapat\"",
    );
    let h = hata(&m);
    assert!(h.contains("yinelenen") && h.contains("komut #1") && h.contains("komut #2"), "{h}");
    // Açılımın kendisi tetikle başlayamaz: söyleniş "atlas" olan sahne.
    let t = kok(
        "[[sahne]]\nad = \"A\"\nsoylenis = \"atlas\"\nadimlar = [ { eylem = \"sessize_al\" } ]\n\
         [[komut]]\nifadeler = [\"{sahne} modu\"]\neylem = \"sahne\"",
    );
    assert!(hata(&t).contains("tetik"));
    // Açılmış tek kelime riskli.
    let s = kok(
        "[[sahne]]\nad = \"Film\"\nsoylenis = \"film\"\nadimlar = [ { eylem = \"sessize_al\" } ]\n\
         [[komut]]\nifadeler = [\"{sahne}\"]\neylem = \"sahne\"\nriskli = false",
    );
    assert!(coz_ok(&s).komutlar[0].riskli);
}

#[test]
fn sahne_yoksa_sahne_sablonu_komut_uretmez() {
    let m = kok(
        "[[komut]]\nifadeler = [\"{sahne} modu\"]\neylem = \"sahne\"\n\
         [[komut]]\nifadeler = [\"ekranı aç\"]\neylem = \"ekranlari_ac\"",
    );
    assert_eq!(coz_ok(&m).komutlar.len(), 1);
}

// --- sahneler ---

#[test]
fn sahne_adimlari_kurulur() {
    let m = sahneli(
        "{ eylem = \"uygulama_ac\", uygulama = \"krom\" }, { bekle_ms = 500 }, \
         { eylem = \"ses_ayarla\", seviye = 20 }, { eylem = \"ekrani_kapat\", ekran = \"sag\" }, \
         { eylem = \"ses_artir\" }, { eylem = \"ses_azalt\", adim = 5 }, { eylem = \"medya\", tus = \"sonraki\" }, \
         { eylem = \"komut_calistir\", komut = \"calc\" }, { eylem = \"tus_gonder\", tuslar = \"win+d\" }, \
         { eylem = \"uygulama_kapat\", uygulama = \"KROM\" }, { eylem = \"uygulama_one_getir\", uygulama = \"krom\" }, \
         { eylem = \"ekranlari_kapat\" }, { eylem = \"pencere_kucult\" }",
    );
    let a = coz_ok(&m);
    assert_eq!(a.sahneler.len(), 1);
    let s = &a.sahneler[0];
    assert_eq!(s.ad, "Deneme");
    assert_eq!(s.soylenis, "deneme");
    assert_eq!(s.adimlar.len(), 13);
    assert_eq!(
        s.adimlar[..6],
        [
            Adim::Eylem(Eylem::UygulamaAc { ad: "Chrome".into(), hedef: "chrome".into(), argumanlar: vec![] }),
            Adim::Bekle(500),
            Adim::Eylem(Eylem::SesAyarla(20)),
            Adim::Eylem(Eylem::EkraniKapat(EkranSecimi::Sag)),
            Adim::Eylem(Eylem::SesArtir(10)),
            Adim::Eylem(Eylem::SesAzalt(5)),
        ]
    );
    assert_eq!(s.adimlar[6], Adim::Eylem(Eylem::Medya(MedyaTusu::Sonraki)));
    assert_eq!(s.adimlar[7], Adim::Eylem(Eylem::KomutCalistir("calc".into())));
    assert_eq!(s.adimlar[8], Adim::Eylem(Eylem::TusGonder("win+d".into())));
    // Uygulama söylenişinde büyük/küçük harf fark etmez.
    assert_eq!(
        s.adimlar[9],
        Adim::Eylem(Eylem::UygulamaKapat { ad: "Chrome".into(), surec: "chrome".into() })
    );
    assert_eq!(
        s.adimlar[10],
        Adim::Eylem(Eylem::UygulamaOneGetir { ad: "Chrome".into(), surec: "chrome".into() })
    );
}

#[test]
fn sahnede_yasak_adimlar() {
    for e in ["sahne", "geri_al", "cik", "dinlemeyi_duraklat"] {
        let h = hata(&sahneli(&format!("{{ eylem = \"{e}\" }}")));
        assert!(h.contains(e) && h.contains("sahne içinde"), "{e}: {h}");
    }
    // İzinli kardeşleri etkilenmez.
    assert!(Ayarlar::coz(&sahneli("{ eylem = \"studyoyu_ac\" }, { eylem = \"uyut\" }")).is_ok());
}

#[test]
fn sahnede_bilinmeyen_uygulama_ve_eylem() {
    let h = hata(&sahneli("{ eylem = \"uygulama_ac\", uygulama = \"yok\" }"));
    assert!(h.contains("bilinmeyen uygulama") && h.contains("yok"), "{h}");
    let h = hata(&sahneli("{ eylem = \"patlat\" }"));
    assert!(h.contains("patlat"), "{h}");
    // Web adresi hedefli ve süreci olmayan uygulama kapatılamaz.
    let h = hata(&sahneli("{ eylem = \"uygulama_kapat\", uygulama = \"youtube\" }"));
    assert!(h.contains("surec"), "{h}");
    assert!(Ayarlar::coz(&sahneli("{ eylem = \"uygulama_ac\", uygulama = \"youtube\" }")).is_ok());
}

#[test]
fn sahnede_bekle_siniri() {
    assert_eq!(coz_ok(&sahneli("{ bekle_ms = 0 }")).sahneler[0].adimlar[0], Adim::Bekle(0));
    assert_eq!(coz_ok(&sahneli("{ bekle_ms = 10000 }")).sahneler[0].adimlar[0], Adim::Bekle(10000));
    for kotu in ["10001", "60000"] {
        let h = hata(&sahneli(&format!("{{ bekle_ms = {kotu} }}")));
        assert!(h.contains("bekle_ms"), "{kotu}: {h}");
    }
    assert!(Ayarlar::coz(&sahneli("{ bekle_ms = -1 }")).is_err());
}

#[test]
fn sahne_adimi_kurallari() {
    // eylem + bekle_ms birlikte, ikisi de yok, bekle_ms ile alan.
    assert!(hata(&sahneli("{ eylem = \"sessize_al\", bekle_ms = 5 }")).contains("birlikte"));
    assert!(hata(&sahneli("{ }")).contains("gerekli"));
    assert!(hata(&sahneli("{ bekle_ms = 5, seviye = 3 }")).contains("seviye"));
    // Yanlış eyleme alan, eksik zorunlu alan, sınır dışı değerler.
    assert!(hata(&sahneli("{ eylem = \"sessize_al\", seviye = 3 }")).contains("seviye"));
    assert!(hata(&sahneli("{ eylem = \"ses_ayarla\" }")).contains("seviye"));
    assert!(hata(&sahneli("{ eylem = \"ses_ayarla\", seviye = 101 }")).contains("0-100"));
    assert!(Ayarlar::coz(&sahneli("{ eylem = \"ses_ayarla\", seviye = 100 }")).is_ok());
    assert!(Ayarlar::coz(&sahneli("{ eylem = \"ses_ayarla\", seviye = 0 }")).is_ok());
    assert!(hata(&sahneli("{ eylem = \"ekrani_kapat\" }")).contains("ekran"));
    assert!(hata(&sahneli("{ eylem = \"medya\" }")).contains("tus"));
    assert!(hata(&sahneli("{ eylem = \"ses_artir\", adim = 0 }")).contains("adim"));
    assert!(Ayarlar::coz(&sahneli("{ eylem = \"uygulama_ac\" }")).is_err());
    // Bilinmeyen adım alanı.
    assert!(Ayarlar::coz(&sahneli("{ eylem = \"sessize_al\", xx = 1 }")).is_err());
    // Hata iletisi sahneyi ve adımı söyler.
    let h = hata(&sahneli("{ bekle_ms = 1 }, { eylem = \"patlat\" }"));
    assert!(h.contains("sahne #1") && h.contains("adım #2"), "{h}");
}

#[test]
fn sahne_ad_ve_soylenis_kurallari() {
    let iki = |a1: &str, s1: &str, a2: &str, s2: &str| {
        kok(&format!(
            "[[sahne]]\nad = \"{a1}\"\nsoylenis = \"{s1}\"\nadimlar = [ {{ eylem = \"sessize_al\" }} ]\n\
             [[sahne]]\nad = \"{a2}\"\nsoylenis = \"{s2}\"\nadimlar = [ {{ eylem = \"sessize_al\" }} ]\n\
             [[komut]]\nifadeler = [\"ekranı aç\"]\neylem = \"ekranlari_ac\""
        ))
    };
    assert!(Ayarlar::coz(&iki("A", "iş", "B", "film")).is_ok());
    assert!(hata(&iki("A", "iş", "a", "film")).contains("adı"));
    assert!(hata(&iki("A", "iş", "B", "İŞ")).contains("söyleniş"));
    assert!(Ayarlar::coz(&iki("", "iş", "B", "film")).is_err());
    assert!(Ayarlar::coz(&iki("A", "", "B", "film")).is_err());
    assert!(Ayarlar::coz(&iki("A", "iş1", "B", "film")).is_err());
    assert!(Ayarlar::coz(&iki("A", "iş!", "B", "film")).is_err());
    // Adımsız sahne ve çok uzun sahne.
    let adimsiz = kok(
        "[[sahne]]\nad = \"A\"\nsoylenis = \"iş\"\nadimlar = []\n\
         [[komut]]\nifadeler = [\"ekranı aç\"]\neylem = \"ekranlari_ac\"",
    );
    assert!(hata(&adimsiz).contains("adım"));
    let otuz_uc = vec!["{ bekle_ms = 1 }"; 33].join(", ");
    assert!(hata(&sahneli(&otuz_uc)).contains("32"));
    let otuz_iki = vec!["{ bekle_ms = 1 }"; 32].join(", ");
    assert!(Ayarlar::coz(&sahneli(&otuz_iki)).is_ok());
}

// --- efektler ---

#[test]
fn efekt_varsayilanlari_katalogla_uyumlu() {
    let (ad, sr) = ("x".to_string(), "x".to_string());
    for (eylem, beklenen) in [
        (Eylem::EkranlariKapat, Efekt::TupKapanisi),
        (Eylem::AnaEkraniKapat, Efekt::Perde),
        (Eylem::EkraniKapat(EkranSecimi::Sol), Efekt::Perde),
        (Eylem::EkranlariAc, Efekt::Nefes),
        (Eylem::UygulamaAc { ad: "x".into(), hedef: "x".into(), argumanlar: vec![] }, Efekt::Portal),
        (Eylem::UygulamaKapat { ad: ad.clone(), surec: sr.clone() }, Efekt::Nefes),
        (Eylem::UygulamaOneGetir { ad, surec: sr }, Efekt::YildizKaymasi),
        (Eylem::SesAyarla(5), Efekt::KenarNabzi),
        (Eylem::SesArtir(5), Efekt::KenarNabzi),
        (Eylem::SesAzalt(5), Efekt::KenarNabzi),
        (Eylem::SessizeAl, Efekt::KenarNabzi),
        (Eylem::SesiAc, Efekt::KenarNabzi),
        (Eylem::Medya(MedyaTusu::Sonraki), Efekt::KenarNabzi),
        (Eylem::PencereKucult, Efekt::Sonar),
        (Eylem::PencereBuyut, Efekt::Sonar),
        (Eylem::PencereGeriYukle, Efekt::Sonar),
        (Eylem::PencereyiTasi, Efekt::Sonar),
        (Eylem::MasaustunuGoster, Efekt::Sonar),
        (Eylem::OturumuKilitle, Efekt::SokDalgasi),
        (Eylem::Uyut, Efekt::Nefes),
        (Eylem::DinlemeyiDuraklat, Efekt::Murekkep),
        (Eylem::GeriAl, Efekt::Tarama),
        (Eylem::ZamanlayicilariIptal, Efekt::Tarama),
        (
            Eylem::Zamanla { dakika: 5, eylem: Box::new(Eylem::Uyut), gosterim: "x".into() },
            Efekt::Sonar,
        ),
        (Eylem::StudyoyuAc, Efekt::Nefes),
        (Eylem::AyarlariYenidenYukle, Efekt::Nefes),
        (Eylem::Cik, Efekt::Nefes),
        (Eylem::Sahne { ad: "x".into() }, Efekt::Aurora),
        (Eylem::KomutCalistir("x".into()), Efekt::Kivilcim),
        (Eylem::TusGonder("x".into()), Efekt::Kivilcim),
    ] {
        assert_eq!(efekt_varsayilani(&eylem), beklenen, "{eylem:?}");
    }
}

#[test]
fn komut_efekti_ve_komuta_gore_kip() {
    let m = kok(
        "[efektler.tup_kapanisi]\nrenk1 = \"#112233\"\nsure_ms = 777\n\
         [[komut]]\nifadeler = [\"ekranları kapat\"]\neylem = \"ekranlari_kapat\"\n\
         [[komut]]\nifadeler = [\"ekranları aç\"]\neylem = \"ekranlari_ac\"\nefekt = \"aurora\"\n",
    );
    let a = coz_ok(&m);
    assert_eq!(a.efekt_kipi, EfektKipi::KomutaGore);
    let (e, ayar) = a.komutlar[0].efekt_coz(&a, 1);
    assert_eq!(e, Efekt::TupKapanisi);
    assert_eq!(ayar.renk1, Renk::yeni(0x11, 0x22, 0x33));
    assert_eq!(ayar.sure_ms, 777);
    // Yazılmayan alan varsayılandan gelir.
    assert_eq!(ayar.ekran, EfektEkrani::Ana);
    // Komutun kendi efekti eylem varsayılanını ezer; ayarı yazılmayan efekt katalog ayarını alır.
    let (e2, ayar2) = a.komutlar[1].efekt_coz(&a, 1);
    assert_eq!(e2, Efekt::Aurora);
    assert_eq!(ayar2, EfektAyari::varsayilan(Efekt::Aurora));
    assert_eq!(a.komutlar[1].efekt, Some(Efekt::Aurora));
}

#[test]
fn tek_tema_kipi() {
    let a = coz_ok(&onde("efekt_tema = \"nefes\""));
    assert_eq!(a.efekt_kipi, EfektKipi::TekTema(Efekt::Nefes));
    for tohum in [0, 1, 99] {
        assert_eq!(ilk_komut(&a).efekt_coz(&a, tohum).0, Efekt::Nefes);
    }
    // Komutun kendi efekti tek temayı değiştirmez.
    let m = kok(
        "efekt_tema = \"sonar\"\n\
         [[komut]]\nifadeler = [\"ekranları kapat\"]\neylem = \"ekranlari_kapat\"\nefekt = \"perde\"",
    );
    let a = coz_ok(&m);
    assert_eq!(a.komutlar[0].efekt, Some(Efekt::Perde));
    assert_eq!(a.komutlar[0].efekt_coz(&a, 0).0, Efekt::Sonar);
}

#[test]
fn rastgele_kip_nefes_disi_ve_belirleyici() {
    let a = coz_ok(&onde("efekt_kipi = \"rastgele\""));
    assert_eq!(a.efekt_kipi, EfektKipi::Rastgele);
    let k = ilk_komut(&a);
    let mut goruldu = std::collections::HashSet::new();
    for tohum in 0..400u64 {
        let (e, ayar) = k.efekt_coz(&a, tohum);
        assert_ne!(e, Efekt::Nefes, "tohum {tohum}");
        assert_eq!(ayar, a.efekt_ayari(e));
        // Aynı tohum aynı sonucu verir.
        assert_eq!(k.efekt_coz(&a, tohum).0, e);
        goruldu.insert(e);
    }
    // Nefes dışındaki 15 efektin hepsi görülür.
    assert_eq!(goruldu.len(), 15);
}

#[test]
fn efekt_kipi_gecersiz_ve_celiski() {
    assert!(hata(&onde("efekt_kipi = \"karisik\"")).contains("efekt_kipi"));
    assert!(hata(&onde("efekt_kipi = \"rastgele\"\nefekt_tema = \"nefes\"")).contains("birlikte"));
    assert!(hata(&onde("efekt_tema = \"yok_boyle\"")).contains("efekt_tema"));
    assert_eq!(coz_ok(&onde("efekt_kipi = \"komuta_gore\"")).efekt_kipi, EfektKipi::KomutaGore);
    // Komuttaki bilinmeyen efekt adı serde ile reddedilir.
    let m = kok("[[komut]]\nifadeler = [\"ekranı aç\"]\neylem = \"ekranlari_ac\"\nefekt = \"yok\"");
    assert!(Ayarlar::coz(&m).is_err());
}

#[test]
fn efektler_tablosu_dogrulanir() {
    // Tablolar komuttan önce yazılır: `komut_ornegi` `[[komut]]` ile devam eder.
    let h = hata(&onde("[efektler.yok_boyle]\nsure_ms = 500"));
    assert!(h.contains("yok_boyle"), "{h}");
    assert!(hata(&onde("[efektler.portal]\nsure_ms = 100")).contains("efektler.portal"));
    assert!(hata(&onde("[efektler.portal]\nsiddet = 1.5")).contains("siddet"));
    assert!(Ayarlar::coz(&onde("[efektler.portal]\nrenk1 = \"kırmızı\"")).is_err());
    assert!(Ayarlar::coz(&onde("[efektler.portal]\nrenkk = \"#FF0000\"")).is_err());
    let a = coz_ok(&onde(
        "[efektler.portal]\nrenk1 = \"#FF0000\"\nsure_ms = 3000\nekran = \"hepsi\"\nsese_tepki = false",
    ));
    let p = a.efekt_ayari(Efekt::Portal);
    assert_eq!(p.renk1, Renk::yeni(255, 0, 0));
    assert_eq!(p.sure_ms, 3000);
    assert_eq!(p.ekran, EfektEkrani::Hepsi);
    assert!(!p.sese_tepki);
    assert_eq!(a.efekt_ayari(Efekt::Sonar), EfektAyari::varsayilan(Efekt::Sonar));
}

// --- olaylar ---

#[test]
fn olay_on_tanimlari_dosyada_yoksa() {
    let a = coz_ok(&komut_ornegi("ekranı aç"));
    assert!(!a.duyuldu_ipucu);
    assert_eq!(a.olay_efekti(Olay::Duyuldu), None);
    assert_eq!(a.olay_efekti(Olay::Anlasildi), None);
    let (e, ayar) = a.olay_efekti(Olay::Anlasilmadi).unwrap();
    assert_eq!(e, Efekt::Bozulma);
    assert_eq!(ayar.renk1, Renk::yeni(0x9E, 0x9E, 0x9E));
    assert_eq!(ayar.sure_ms, 300);
    let (e, ayar) = a.olay_efekti(Olay::TetikGerekli).unwrap();
    assert_eq!(e, Efekt::KenarNabzi);
    assert_eq!(ayar.renk1, Renk::yeni(0xFF, 0xB3, 0x00));
    assert_eq!(ayar.sure_ms, 400);
    let (e, ayar) = a.olay_efekti(Olay::Hata).unwrap();
    assert_eq!(e, Efekt::Bozulma);
    assert_eq!(ayar.renk1, Renk::yeni(0xFF, 0x52, 0x52));
    assert_eq!(ayar.sure_ms, 400);
    // Duyuldu ön tanımı: ses dalgası, kısık şiddet (kapalı olsa da kayıtlı).
    assert_eq!(a.olay_efektleri.duyuldu.efekt, Efekt::SesDalgasi);
    assert!((a.olay_efektleri.duyuldu.siddet - 0.3).abs() < 1e-6);
    assert!(a.olay_efektleri.duyuldu.efekt_ayari().dogrula().is_ok());
}

#[test]
fn olay_acilip_kapatilabilir_ve_dogrulanir() {
    let a = coz_ok(&onde("[olaylar.duyuldu]\ngoster = true\nefekt = \"nefes\""));
    assert!(a.duyuldu_ipucu);
    assert_eq!(a.olay_efekti(Olay::Duyuldu).unwrap().0, Efekt::Nefes);
    let a = coz_ok(&onde("[olaylar.hata]\ngoster = false\nefekt = \"bozulma\""));
    assert_eq!(a.olay_efekti(Olay::Hata), None);
    // Diğer olaylar ön tanımda kalır.
    assert!(a.olay_efekti(Olay::TetikGerekli).is_some());
    assert!(hata(&onde("[olaylar.hata]\nsure_ms = 50")).contains("olaylar.hata"));
    assert!(hata(&onde("[olaylar.anlasilmadi]\nsiddet = 3.0")).contains("olaylar.anlasilmadi"));
    assert!(Ayarlar::coz(&onde("[olaylar.bilinmeyen]\ngoster = true")).is_err());
    assert!(Ayarlar::coz(&onde("[olaylar.hata]\ngosterr = true")).is_err());
    assert!(Ayarlar::coz(&onde("[olaylar.hata]\nefekt = \"yok\"")).is_err());
}

// --- sessiz saatler ve diğer genel ayarlar ---

#[test]
fn sessiz_saatler_ayarlardan_okunur() {
    let a = coz_ok(&onde("sessiz_saatler = [\"23:00-07:30\", \"13:00-14:00\"]"));
    assert_eq!(a.sessiz_saatler, vec![(23 * 60, 7 * 60 + 30), (13 * 60, 14 * 60)]);
    assert!(a.sessiz_mi(23 * 60)); // başlangıç dahil
    assert!(a.sessiz_mi(0));
    assert!(a.sessiz_mi(7 * 60 + 29));
    assert!(!a.sessiz_mi(7 * 60 + 30)); // bitiş hariç
    assert!(!a.sessiz_mi(10 * 60));
    assert!(a.sessiz_mi(13 * 60 + 30));
    assert!(!a.sessiz_mi(14 * 60));
    assert!(!a.sessiz_mi(22 * 60 + 59));
    let bos = coz_ok(&komut_ornegi("ekranı aç"));
    assert!(bos.sessiz_saatler.is_empty());
    assert!(!bos.sessiz_mi(3 * 60));
    for kotu in ["\"23:00\"", "\"25:00-07:00\"", "\"07:00-07:00\"", "\"a-b\"", "\"\""] {
        let h = hata(&onde(&format!("sessiz_saatler = [{kotu}]")));
        assert!(h.contains("sessiz"), "{kotu}: {h}");
    }
}

#[test]
fn genel_ayar_varsayilanlari_ve_sinirlari() {
    let a = coz_ok(&komut_ornegi("ekranı aç"));
    assert_eq!(a.model_birakma_sn, 30);
    assert!(!a.windows_ile_baslat);
    assert!(!a.anlasilmayanlari_kaydet);
    assert!(a.ekran_yontemleri.is_empty());
    assert_eq!(a.mikrofon, None);
    assert!(a.sahneler.is_empty());
    assert!(a.efektler.is_empty());

    for ok in ["5", "3600", "120"] {
        let a = coz_ok(&onde(&format!("model_birakma_sn = {ok}")));
        assert_eq!(a.model_birakma_sn, ok.parse::<u32>().unwrap());
    }
    for kotu in ["4", "3601", "0"] {
        assert!(hata(&onde(&format!("model_birakma_sn = {kotu}"))).contains("model_birakma_sn"), "{kotu}");
    }
    assert!(Ayarlar::coz(&onde("model_birakma_sn = -5")).is_err());
    let a = coz_ok(&onde(
        "windows_ile_baslat = true\nanlasilmayanlari_kaydet = false\nmikrofon = \"{0.0.1.00000000}.{abc}\"",
    ));
    assert!(a.windows_ile_baslat);
    assert!(!a.anlasilmayanlari_kaydet);
    assert_eq!(a.mikrofon.as_deref(), Some("{0.0.1.00000000}.{abc}"));
    assert!(hata(&onde("mikrofon = \"  \"")).contains("mikrofon"));
}

#[test]
fn ekran_yontemleri_okunur() {
    let g = |ek: &str| format!("{}\n{ek}", komut_ornegi("ekranı aç"));
    let a = coz_ok(&g(
        "[[ekran]]\naygit = \"\\\\\\\\.\\\\DISPLAY1\"\nyontem = \"ddc\"\n\
         [[ekran]]\naygit = \"\\\\\\\\.\\\\DISPLAY2\"\nyontem = \"perde\"\n\
         [[ekran]]\naygit = \"\\\\\\\\.\\\\DISPLAY3\"",
    ));
    assert_eq!(
        a.ekran_yontemleri,
        vec![
            ("\\\\.\\DISPLAY1".to_string(), YontemTercihi::Ddc),
            ("\\\\.\\DISPLAY2".to_string(), YontemTercihi::Perde),
            ("\\\\.\\DISPLAY3".to_string(), YontemTercihi::Otomatik),
        ]
    );
    assert!(Ayarlar::coz(&g("[[ekran]]\naygit = \"\"")).is_err());
    assert!(Ayarlar::coz(&g("[[ekran]]\naygit = \"a\"\nyontem = \"sihir\"")).is_err());
    assert!(Ayarlar::coz(&g("[[ekran]]\nyontem = \"ddc\"")).is_err());
    assert!(hata(&g("[[ekran]]\naygit = \"a\"\n[[ekran]]\naygit = \"A\"")).contains("birden fazla"));
}

#[test]
fn uygulama_surec_ve_yineleme_kurallari() {
    let u = |ek: &str| {
        kok(&format!(
            "[[uygulama]]\nsoylenis = \"krom\"\nad = \"C\"\nhedef = \"c\"\n{ek}\n\
             [[komut]]\nifadeler = [\"ekranı aç\"]\neylem = \"ekranlari_ac\""
        ))
    };
    assert!(Ayarlar::coz(&u("surec = \"chrome\"")).is_ok());
    assert!(hata(&u("surec = \"  \"")).contains("surec"));
    // Aynı söyleniş iki uygulamada.
    let iki = kok(
        "[[uygulama]]\nsoylenis = \"krom\"\nad = \"A\"\nhedef = \"a\"\n\
         [[uygulama]]\nsoylenis = \"KROM\"\nad = \"B\"\nhedef = \"b\"\n\
         [[komut]]\nifadeler = [\"ekranı aç\"]\neylem = \"ekranlari_ac\"",
    );
    assert!(hata(&iki).contains("söyleniş"));
}

// --- Ham şema ---

fn el_yapimi_ham() -> HamAyarlar {
    HamAyarlar {
        tetik: "atlas".into(),
        guven_esigi: 0.55,
        model_yolu: Some("modeller/tr".into()),
        mikrofon: Some("{mik}".into()),
        efekt_kipi: Some("rastgele".into()),
        efekt_tema: None,
        model_birakma_sn: 120,
        windows_ile_baslat: true,
        anlasilmayanlari_kaydet: false,
        dinleme_kipi: crate::DinlemeKipi::Tusla,
        dinleme_tusu: Some("Kontrol+Alt+Boşluk".into()),
        dinleme_suresi_sn: 10,
        palet_tusu: Some("ctrl+alt+p".into()),
        alkis: crate::HamAlkis { acik: true, ifade: "sesi aç".into(), hassasiyet: 0.4 },
        efektler_acik: true,
        olay_efektleri_acik: false,
        guncelleme_otomatik: false,
        sessiz_saatler: vec!["23:00-07:30".into(), "13:00-14:00".into()],
        efekt_ortak: crate::EfektOrtak {
            renk1: Renk::yeni(9, 8, 7),
            hiz: 1.5,
            ekran: EfektEkrani::Hepsi,
            ..Default::default()
        },
        olaylar: crate::HamOlaylar {
            duyuldu: OlayAyari::yeni(true, Efekt::Nefes, EfektAyari::varsayilan(Efekt::Nefes)),
            ..Default::default()
        },
        efektler: [
            (
                "portal".to_string(),
                crate::EfektOzel::tam(EfektAyari {
                    renk1: Renk::yeni(1, 2, 3),
                    renk2: Renk::yeni(4, 5, 6),
                    sure_ms: 1500,
                    siddet: 0.4,
                    ekran: EfektEkrani::Imlec,
                    sese_tepki: false,
                }),
            ),
            ("aurora".to_string(), crate::EfektOzel { renk2: Some(Renk::yeni(0x7C, 0x4D, 0xFF)), ..Default::default() }),
        ]
        .into_iter()
        .collect(),
        ekran: vec![HamEkran { aygit: "\\\\.\\DISPLAY1".into(), yontem: YontemTercihi::Ddc }],
        uygulama: vec![
            HamUygulama {
                soylenis: "krom".into(),
                ad: "Chrome".into(),
                hedef: "chrome".into(),
                argumanlar: vec!["--yeni".into()],
                surec: Some("chrome".into()),
            },
            HamUygulama {
                soylenis: "youtube".into(),
                ad: "YouTube".into(),
                hedef: "https://www.youtube.com".into(),
                ..Default::default()
            },
        ],
        sahne: vec![HamSahne {
            ad: "İş".into(),
            soylenis: "iş".into(),
            adimlar: vec![
                HamAdim { eylem: Some("uygulama_ac".into()), uygulama: Some("krom".into()), ..Default::default() },
                HamAdim { bekle_ms: Some(500), ..Default::default() },
                HamAdim { eylem: Some("ekrani_kapat".into()), ekran: Some(EkranSecimi::Sag), ..Default::default() },
                HamAdim { eylem: Some("medya".into()), tus: Some(MedyaTusu::Sonraki), ..Default::default() },
            ],
        }],
        kisayol: vec![
            crate::HamKisayol { tuslar: "ctrl+alt+1".into(), ifade: "krom aç".into() },
            crate::HamKisayol { tuslar: "win+shift+f5".into(), ifade: "sesi yüzde elli yap".into() },
        ],
        komut: vec![
            HamKomut {
                ifadeler: vec!["{uygulama} aç".into()],
                eylem: "uygulama_ac".into(),
                efekt: Some(Efekt::Kivilcim),
                ..Default::default()
            },
            HamKomut {
                ifadeler: vec!["müziği durdur".into(), "müziği oynat".into()],
                eylem: "medya".into(),
                tus: Some(MedyaTusu::OynatDuraklat),
                ..Default::default()
            },
            HamKomut {
                ifadeler: vec!["sesi aç".into()],
                eylem: "sesi_ac".into(),
                zamanlanabilir: true,
                ..Default::default()
            },
            HamKomut {
                ifadeler: vec!["sesi yüzde {sayi} yap".into()],
                eylem: "ses_ayarla".into(),
                ..Default::default()
            },
            HamKomut {
                ifadeler: vec!["sesi artır".into()],
                eylem: "ses_artir".into(),
                adim: Some(15),
                riskli: Some(true),
                ..Default::default()
            },
            HamKomut {
                ifadeler: vec!["{sahne} modu".into()],
                eylem: "sahne".into(),
                ..Default::default()
            },
            HamKomut {
                ifadeler: vec!["görev yöneticisi".into()],
                eylem: "tus_gonder".into(),
                tuslar: Some("ctrl+shift+esc".into()),
                ..Default::default()
            },
        ],
    }
}

#[test]
fn ham_gidis_donus_varsayilan_dosya() {
    let ham = HamAyarlar::coz(Ayarlar::varsayilan_toml()).expect("ham okuma");
    let yazilan = ham.toml_yaz();
    assert!(yazilan.starts_with("# Echora ayar dosyası"), "{yazilan}");
    let geri = HamAyarlar::coz(&yazilan).unwrap_or_else(|h| panic!("{h}\n{yazilan}"));
    assert_eq!(geri, ham);
    let a1 = coz_ok(Ayarlar::varsayilan_toml());
    let a2 = coz_ok(&yazilan);
    assert_eq!(a1, a2);
    assert_eq!(Ayarlar::ham_coz(Ayarlar::varsayilan_toml()).unwrap(), ham);
}

#[test]
fn ham_gidis_donus_el_yapimi() {
    let ham = el_yapimi_ham();
    let beklenen = Ayarlar::hamdan(&ham).unwrap_or_else(|h| panic!("{h}"));
    let yazilan = ham.toml_yaz_dene().unwrap();
    let geri = HamAyarlar::coz(&yazilan).unwrap_or_else(|h| panic!("{h}\n{yazilan}"));
    assert_eq!(geri, ham, "{yazilan}");
    assert_eq!(coz_ok(&yazilan), beklenen);
    // Beklenen içerik.
    assert_eq!(beklenen.efekt_kipi, EfektKipi::Rastgele);
    // Duyuldu açık ama olay efektleri genel olarak kapalı: ipucu gösterilmez.
    assert!(beklenen.olay_efektleri.duyuldu.goster);
    assert!(!beklenen.duyuldu_ipucu);
    assert_eq!(beklenen.sahneler.len(), 1);
    assert_eq!(beklenen.sessiz_saatler.len(), 2);
    assert_eq!(beklenen.efekt_ayari(Efekt::Portal).sure_ms, 1500);
    // Kısmi istisna: yazılan alan ezer, gerisi ortak ayardan (hız 1.5: 1800 / 1.5 = 1200 ms).
    let aurora = beklenen.efekt_ayari(Efekt::Aurora);
    assert_eq!(aurora.renk2, Renk::yeni(0x7C, 0x4D, 0xFF));
    assert_eq!(aurora.renk1, Renk::yeni(9, 8, 7));
    assert_eq!(aurora.ekran, EfektEkrani::Hepsi);
    assert_eq!(aurora.sure_ms, 1200);
    assert!(!beklenen.olay_efektleri_acik && !beklenen.guncelleme_otomatik);
    // Yazım kararlı: ikinci gidiş-dönüş aynı metni verir.
    assert_eq!(geri.toml_yaz(), ham.toml_yaz());
}

#[test]
fn ham_varsayilan_ve_zorunlu_alanlar() {
    let d = HamAyarlar::default();
    assert_eq!(d.tetik, "atlas");
    assert_eq!(d.guven_esigi, 0.7);
    assert_eq!(d.model_birakma_sn, 30);
    assert!(!d.anlasilmayanlari_kaydet);
    assert!(!d.olaylar.duyuldu.goster);
    // Komutsuz varsayılan ham ayar yüklenemez (hiç komut yok) ama yazılıp okunabilir.
    assert!(Ayarlar::hamdan(&d).is_err());
    assert_eq!(HamAyarlar::coz(&d.toml_yaz()).unwrap(), d);
    // tetik zorunlu, bilinmeyen alan reddi.
    assert!(HamAyarlar::coz("guven_esigi = 0.5").is_err());
    assert!(HamAyarlar::coz("tetik = \"a\"\nxx = 1").is_err());
    assert!(HamAyarlar::coz("tetik = \"a\"\n[[komut]]\nifadeler = []\neylem = \"cik\"\nriskly = true").is_err());
}

#[test]
fn hamdan_dogrulamayi_tek_yerde_yapar() {
    // Stüdyo'nun bozuk bir ham ayarı yazmasına karşı: Ayarlar::hamdan aynı kuralları uygular.
    let mut ham = el_yapimi_ham();
    assert!(Ayarlar::hamdan(&ham).is_ok());
    ham.guven_esigi = 2.0;
    assert!(Ayarlar::hamdan(&ham).unwrap_err().contains("guven_esigi"));
    ham.guven_esigi = 0.5;
    ham.komut[1].ifadeler = vec!["müziği".into()];
    assert!(Ayarlar::hamdan(&ham).is_ok()); // tek kelime riskli olur, hata değil
    ham.komut[1].tus = None;
    assert!(Ayarlar::hamdan(&ham).unwrap_err().contains("tus"));
    ham.komut[1].tus = Some(MedyaTusu::Durdur);
    ham.efekt_tema = Some("nefes".into());
    assert!(Ayarlar::hamdan(&ham).unwrap_err().contains("birlikte"));
    ham.efekt_tema = None;
    ham.sessiz_saatler.push("bozuk".into());
    assert!(Ayarlar::hamdan(&ham).unwrap_err().contains("sessiz"));
}

// --- varsayılan dosya (yeni) ---

#[test]
fn varsayilan_dosya_sahne_efekt_ve_olaylari_icerir() {
    let a = coz_ok(Ayarlar::varsayilan_toml());
    assert_eq!(a.sahneler.len(), 3);
    let ad: Vec<_> = a.sahneler.iter().map(|s| s.soylenis.as_str()).collect();
    assert_eq!(ad, ["iş", "film", "gece"]);
    assert_eq!(
        a.sahneler[0].adimlar,
        vec![
            Adim::Eylem(Eylem::UygulamaAc { ad: "Chrome".into(), hedef: "chrome".into(), argumanlar: vec![] }),
            Adim::Bekle(500),
            Adim::Eylem(Eylem::SesAyarla(30)),
        ]
    );
    assert_eq!(
        a.sahneler[1].adimlar,
        vec![Adim::Eylem(Eylem::SesAyarla(70)), Adim::Eylem(Eylem::EkraniKapat(EkranSecimi::Sag))]
    );
    assert_eq!(
        a.sahneler[2].adimlar,
        vec![Adim::Eylem(Eylem::SesAyarla(15)), Adim::Eylem(Eylem::EkranlariKapat)]
    );
    assert!(!a.duyuldu_ipucu);
    assert!(a.olay_efekti(Olay::Anlasilmadi).is_some());
    // Efekt başına ön tanım yok: ortak ayar hepsine işler, süre katalog süresi (hız 1.0).
    assert!(a.efektler.is_empty());
    for e in Efekt::HEPSI {
        assert_eq!(a.efekt_ayari(e).sure_ms, e.varsayilan_sure_ms(), "{e:?}");
    }
    assert_eq!(a.efekt_kipi, EfektKipi::KomutaGore);
    assert_eq!(a.model_birakma_sn, 30);
    assert!(!a.anlasilmayanlari_kaydet);
}

#[test]
fn varsayilan_dosyada_yeni_komutlarin_riski_ve_eslesmesi() {
    let a = coz_ok(Ayarlar::varsayilan_toml());
    let k = |i: &str| a.komutlar.iter().find(|k| k.ifade == i).unwrap_or_else(|| panic!("{i}"));
    for riskli in [
        "sağ ekranı kapat", "krom kapat", "bilgisayarı kitle", "bilgisayarı uyut", "iş modu",
        "film modu", "gece modu", "dinlemeyi durdur",
    ] {
        assert!(k(riskli).riskli, "{riskli}");
    }
    for rahat in [
        "krom aç", "krom öne getir", "sesi yüzde yirmi beş yap", "sesi artır", "sesi azalt", "sesi kıs",
        "sessize al", "sesi kapat", "sesi aç", "pencereyi indir", "pencereyi tam ekran yap",
        "pencereyi daralt", "pencereyi diğer ekrana taşı", "masaüstü göster", "müziği durdur",
        "müziği oynat", "sonraki şarkı", "önceki şarkı", "geri al", "stüdyo aç", "ayarları aç",
        "ayarları yeniden yükle",
    ] {
        assert!(!k(rahat).riskli, "{rahat}");
    }
    assert_eq!(k("sesi yüzde yüz yap").eylem, Eylem::SesAyarla(100));
    assert_eq!(k("müziği durdur").eylem, Eylem::Medya(MedyaTusu::OynatDuraklat));
    assert_eq!(k("müziği oynat").eylem, k("müziği durdur").eylem);
    assert_eq!(k("sonraki şarkı").eylem, Eylem::Medya(MedyaTusu::Sonraki));
    assert_eq!(k("önceki şarkı").eylem, Eylem::Medya(MedyaTusu::Onceki));
    assert_eq!(
        k("hesap makinesi kapat").eylem,
        Eylem::UygulamaKapat { ad: "Hesap Makinesi".into(), surec: "CalculatorApp".into() }
    );
    assert_eq!(k("ana ekranı kapat").eylem, Eylem::EkraniKapat(EkranSecimi::Ana));
    // Web adresi hedefli youtube yalnız açılır.
    assert!(a.komutlar.iter().any(|k| k.ifade == "youtube aç"));
    assert!(!a.komutlar.iter().any(|k| k.ifade == "youtube kapat" || k.ifade == "youtube öne getir"));
    assert!(matches!(a.esle("film modu"), Eslesme::TetikGerekli(_)));
    assert!(matches!(a.esle("atlas film modu"), Eslesme::Komut(_)));
    assert!(matches!(a.esle("sesi aç"), Eslesme::Komut(_)));
    // Varsayılan efektler eyleme göre.
    assert_eq!(k("film modu").efekt_coz(&a, 0).0, Efekt::Aurora);
    assert_eq!(k("ekranları kapat").efekt_coz(&a, 0).0, Efekt::TupKapanisi);
    assert_eq!(k("sesi artır").efekt_coz(&a, 0).0, Efekt::KenarNabzi);
}

#[test]
fn varsayilan_dosyadaki_tum_kelimeler_modelin_sozlugunde() {
    // Modelde VAR olduğu ölçülmüş kelimeler (plan: Sözlük kısıtı) + önceki fazlardaki bilinenler.
    const VAR: &str = "pencereyi indir kaldır daralt tam ekran ekranı ekrana ekranları diğer öteki taşı \
        masaüstü göster bilgisayarı kitle uyut uyku moduna geç sesi ses yüzde yap artır azalt kıs \
        sessize al aç kapat müziği durdur oynat sonraki önceki şarkı geri stüdyo ayarları sahne \
        modu iş oyun film gece sol sağ ikinci öne getir dinlemeyi başlat sürdür yeniden yükle \
        sıfır bir iki üç dört beş altı yedi sekiz dokuz on yirmi otuz kırk elli altmış yetmiş \
        seksen doksan yüz dakika sonra yarım saat zamanlayıcı iptal geri sayım \
        hepsini ana krom not defteri hesap makinesi youtube atlas";
    let izinli: std::collections::HashSet<&str> = VAR.split_whitespace().collect();
    let a = coz_ok(Ayarlar::varsayilan_toml());
    let mut yabanci = std::collections::BTreeSet::new();
    for ifade in a.gramer_ifadeleri() {
        for kelime in ifade.split(' ') {
            if !izinli.contains(kelime) {
                yabanci.insert(kelime.to_string());
            }
        }
    }
    assert!(yabanci.is_empty(), "sözlükte olmayan kelimeler: {yabanci:?}");
    // Sınama gerçekten bir şey denetliyor: sözlükte olmayan kelimeler listede yok.
    assert!(!izinli.contains("chrome") && !izinli.contains("kilitle") && !izinli.contains("büyüt"));
}

#[test]
fn varsayilan_dosyada_komut_ve_gramer_sayilari() {
    let a = coz_ok(Ayarlar::varsayilan_toml());
    assert_eq!(a.komutlar.len(), VARSAYILAN_KOMUT_SAYISI);
    assert_eq!(a.gramer_ifadeleri().len(), VARSAYILAN_KOMUT_SAYISI * 2);
    // Her ifade benzersiz.
    let benzersiz: std::collections::HashSet<_> = a.komutlar.iter().map(|k| &k.ifade).collect();
    assert_eq!(benzersiz.len(), a.komutlar.len());
}
