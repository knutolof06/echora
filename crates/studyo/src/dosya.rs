//! Ayar ve "anlaşılmayanlar" dosyalarının okunması/yazılması. Yazımlar atomiktir: aynı klasörde
//! geçici dosyaya yazılır, sonra `rename` ile yerine konur (yarım dosya kalmaz; arka plan süreci
//! dosya izleyicisi hiçbir zaman yarım metin görmez).

use std::path::{Path, PathBuf};

use komut::{Ayarlar, HamAyarlar};

/// Anlaşılmayanlar listesinde gösterilen en çok satır.
pub const ANLASILMAYAN_SINIRI: usize = 50;

/// Açılıştaki ayar dosyası durumu.
#[derive(Debug, Clone, PartialEq)]
pub enum Yukleme {
    /// Dosya okundu ve ham şemaya uydu (doğrulama ayrıca yapılır; geçersiz olabilir).
    Tamam(HamAyarlar),
    /// Dosya yok: gömülü varsayılan kullanılır, kaydedince oluşturulur.
    Yok(HamAyarlar),
    /// Dosya okunamadı ya da şemaya uymadı; düzenlenemez.
    Bozuk(String),
}

/// Gömülü varsayılan ayarın ham hali.
pub fn varsayilan_ham() -> HamAyarlar {
    Ayarlar::ham_coz(Ayarlar::varsayilan_toml()).expect("gömülü varsayılan ayar ham şemaya uymalı")
}

pub fn ayar_yukle(yol: &Path) -> Yukleme {
    match std::fs::read_to_string(yol) {
        Ok(metin) => match Ayarlar::ham_coz(&metin) {
            Ok(ham) => Yukleme::Tamam(ham),
            Err(h) => Yukleme::Bozuk(format!("{}: {h}", yol.display())),
        },
        Err(h) if h.kind() == std::io::ErrorKind::NotFound => Yukleme::Yok(varsayilan_ham()),
        Err(h) => Yukleme::Bozuk(format!("{} okunamadı: {h}", yol.display())),
    }
}

/// Aynı klasördeki geçici dosya yolu: `<ad>.<pid>.tmp`. Süreç numarası, Stüdyo ile tepsi süreci
/// (efekt anahtarı) aynı anda yazarsa birbirinin geçici dosyasını ezmesin diye.
fn gecici_yol(yol: &Path) -> PathBuf {
    let mut ad = yol.file_name().map(|a| a.to_os_string()).unwrap_or_else(|| "ayarlar.toml".into());
    ad.push(format!(".{}.tmp", std::process::id()));
    yol.with_file_name(ad)
}

/// Metni atomik olarak yazar (geçici dosya + `rename`). Klasör yoksa oluşturur.
pub fn atomik_yaz(yol: &Path, metin: &str) -> Result<(), String> {
    if let Some(k) = yol.parent() {
        if !k.as_os_str().is_empty() {
            std::fs::create_dir_all(k).map_err(|h| format!("{} oluşturulamadı: {h}", k.display()))?;
        }
    }
    let gecici = gecici_yol(yol);
    let yaz = || -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(&gecici)?;
        f.write_all(metin.as_bytes())?;
        // Yeniden adlandırmadan önce diske: güç kesilirse yarım/boş dosya yerine geçmesin.
        f.sync_all()?;
        drop(f);
        std::fs::rename(&gecici, yol)
    };
    let sonuc = yaz();
    if let Err(h) = sonuc {
        let _ = std::fs::remove_file(&gecici);
        return Err(format!("{} yazılamadı: {h}", yol.display()));
    }
    Ok(())
}

/// Dosyanın içerik parmak izi (dosya yoksa `None`). Yüklemede/kaydetmede saklanır; kaydetmeden önce
/// karşılaştırılarak dosyanın Stüdyo dışında değişip değişmediği anlaşılır.
pub fn parmak_izi(yol: &Path) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    let veri = std::fs::read(yol).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    veri.hash(&mut h);
    Some(h.finish())
}

/// `ayarlar.toml.bak`: Stüdyo'nun ilk kaydından önce özgün dosyanın (yorumlarıyla) kopyası.
pub fn yedek_yolu(yol: &Path) -> PathBuf {
    let mut ad = yol.file_name().map(|a| a.to_os_string()).unwrap_or_else(|| "ayarlar.toml".into());
    ad.push(".bak");
    yol.with_file_name(ad)
}

/// Yedek yoksa ve özgün dosya varsa kopyalar; oluşturulduysa yolunu döner. Var olan yedeğin
/// üzerine yazılmaz.
pub fn yedek_al(yol: &Path) -> Result<Option<PathBuf>, String> {
    let yedek = yedek_yolu(yol);
    if yedek.exists() || !yol.exists() {
        return Ok(None);
    }
    std::fs::copy(yol, &yedek).map_err(|h| format!("{} yedeklenemedi: {h}", yol.display()))?;
    Ok(Some(yedek))
}

/// Arka plan sürecinin yazdığı son geçerli ayar kopyası (aynı klasörde).
pub fn son_gecerli_yolu(yol: &Path) -> PathBuf {
    yol.with_file_name("ayarlar.son-gecerli.toml")
}

/// Yerel saat (yıl, ay, gün, saat, dakika, saniye).
pub fn yerel_zaman() -> (u16, u16, u16, u16, u16, u16) {
    // SAFETY: salt-okuma.
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    (t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/// `ayarlar.toml.bozuk-YYYYMMDD-HHMMSS`.
pub fn bozuk_yedek_yolu(yol: &Path, z: (u16, u16, u16, u16, u16, u16)) -> PathBuf {
    let mut ad = yol.file_name().map(|a| a.to_os_string()).unwrap_or_else(|| "ayarlar.toml".into());
    ad.push(format!(".bozuk-{:04}{:02}{:02}-{:02}{:02}{:02}", z.0, z.1, z.2, z.3, z.4, z.5));
    yol.with_file_name(ad)
}

/// Bozuk dosyayı zaman damgalı adla kopyalar (üzerine yazılmadan önce). Dosya yoksa `Ok(None)`.
pub fn bozugu_sakla(yol: &Path, z: (u16, u16, u16, u16, u16, u16)) -> Result<Option<PathBuf>, String> {
    if !yol.exists() {
        return Ok(None);
    }
    let hedef = bozuk_yedek_yolu(yol, z);
    std::fs::copy(yol, &hedef).map_err(|h| format!("bozuk dosya saklanamadı: {h}"))?;
    Ok(Some(hedef))
}

/// Ayarları doğrular ve atomik olarak kaydeder. Geçersiz ayarda dosyaya dokunulmaz. Yazılacak metin
/// yazmadan önce yeniden okunur ve aynı `Ayarlar`ı verdiği denetlenir (serileştirme kaybı olursa
/// dosya bozulmasın).
pub fn ayar_kaydet(yol: &Path, ham: &HamAyarlar) -> Result<Ayarlar, String> {
    let ayarlar = Ayarlar::hamdan(ham)?;
    let metin = ham.toml_yaz_dene()?;
    let geri = Ayarlar::coz(&metin).map_err(|h| format!("yazılacak metin yeniden okunamadı: {h}"))?;
    if geri != ayarlar {
        return Err("yazılacak metin aynı ayarları vermiyor; kaydedilmedi".into());
    }
    atomik_yaz(yol, &metin)?;
    Ok(ayarlar)
}

/// Anlaşılmayanlar dosyası: satır başına bir metin, en yenisi sonda.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Anlasilmayanlar {
    /// Dosyadaki tüm (boş olmayan) satırlar, dosya sırasıyla.
    pub satirlar: Vec<String>,
}

impl Anlasilmayanlar {
    pub fn coz(metin: &str) -> Self {
        Anlasilmayanlar {
            satirlar: metin.lines().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect(),
        }
    }

    /// Dosya yoksa boş liste.
    pub fn oku(yol: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(yol) {
            Ok(m) => Ok(Self::coz(&m)),
            Err(h) if h.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(h) => Err(format!("{} okunamadı: {h}", yol.display())),
        }
    }

    /// Gösterilecek satırlar: son `ANLASILMAYAN_SINIRI` satır, EN YENİSİ ÖNCE; (dosyadaki sıra, metin).
    pub fn gosterilecekler(&self) -> Vec<(usize, &str)> {
        let bas = self.satirlar.len().saturating_sub(ANLASILMAYAN_SINIRI);
        (bas..self.satirlar.len()).rev().map(|i| (i, self.satirlar[i].as_str())).collect()
    }

    pub fn metin(&self) -> String {
        let mut m = self.satirlar.join("\n");
        if !m.is_empty() {
            m.push('\n');
        }
        m
    }

    /// Dosya sırasındaki `i`. satırı siler.
    pub fn sil(&mut self, i: usize) {
        if i < self.satirlar.len() {
            self.satirlar.remove(i);
        }
    }

    /// Ekranda `sira`'da gösterilen `metin`i siler. Dosya bu arada değiştiyse (arka plan yeni satır
    /// eklediyse) sıra kaymış olabilir: o durumda metnin son geçtiği satır silinir.
    pub fn metni_sil(&mut self, sira: usize, metin: &str) {
        if self.satirlar.get(sira).is_some_and(|s| s == metin) {
            self.satirlar.remove(sira);
        } else if let Some(p) = self.satirlar.iter().rposition(|s| s == metin) {
            self.satirlar.remove(p);
        }
    }

    pub fn temizle(&mut self) {
        self.satirlar.clear();
    }

    pub fn yaz(&self, yol: &Path) -> Result<(), String> {
        atomik_yaz(yol, &self.metin())
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    /// Testin kendi geçici klasörü; düşerken silinir.
    struct GeciciKlasor(PathBuf);
    impl GeciciKlasor {
        fn yeni(ad: &str) -> Self {
            let k = std::env::temp_dir().join(format!("studyo-test-{ad}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&k);
            std::fs::create_dir_all(&k).unwrap();
            GeciciKlasor(k)
        }
    }
    impl Drop for GeciciKlasor {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn kaydet_gidis_donus_ayni_ayarlar() {
        let k = GeciciKlasor::yeni("gidis");
        let yol = k.0.join("alt").join("ayarlar.toml");
        let mut ham = varsayilan_ham();
        ham.tetik = "pusula".into();
        ham.sessiz_saatler.push("23:00-07:30".into());
        let kaydedilen = ayar_kaydet(&yol, &ham).unwrap();
        let geri = Ayarlar::yukle(&yol).unwrap();
        assert_eq!(geri, kaydedilen);
        assert_eq!(geri, Ayarlar::hamdan(&ham).unwrap());
        match ayar_yukle(&yol) {
            Yukleme::Tamam(h) => assert_eq!(Ayarlar::hamdan(&h).unwrap(), kaydedilen),
            d => panic!("beklenmeyen: {d:?}"),
        }
        // Geçici dosya kalmadı.
        let dosyalar: Vec<_> = std::fs::read_dir(yol.parent().unwrap()).unwrap().collect();
        assert_eq!(dosyalar.len(), 1);
    }

    #[test]
    fn gecici_dosya_adi_surec_numarali() {
        let g = gecici_yol(Path::new(r"C:\x\ayarlar.toml"));
        assert_eq!(g, PathBuf::from(format!(r"C:\x\ayarlar.toml.{}.tmp", std::process::id())));
    }

    #[test]
    fn gecersiz_ayar_yazilmaz() {
        let k = GeciciKlasor::yeni("gecersiz");
        let yol = k.0.join("ayarlar.toml");
        ayar_kaydet(&yol, &varsayilan_ham()).unwrap();
        let once = std::fs::read_to_string(&yol).unwrap();
        let mut ham = varsayilan_ham();
        ham.tetik = "iki kelime".into();
        assert!(ayar_kaydet(&yol, &ham).is_err());
        assert_eq!(std::fs::read_to_string(&yol).unwrap(), once);
        // Hiç yoksa da oluşturulmaz.
        let yeni = k.0.join("yok.toml");
        assert!(ayar_kaydet(&yeni, &ham).is_err());
        assert!(!yeni.exists());
    }

    #[test]
    fn eksik_ve_bozuk_dosya() {
        let k = GeciciKlasor::yeni("bozuk");
        assert!(matches!(ayar_yukle(&k.0.join("yok.toml")), Yukleme::Yok(_)));
        let yol = k.0.join("bozuk.toml");
        std::fs::write(&yol, "tetik = \"atlas\"\nbilinmeyen_alan = 3\n").unwrap();
        assert!(matches!(ayar_yukle(&yol), Yukleme::Bozuk(_)));
        std::fs::write(&yol, "bu toml değil [").unwrap();
        assert!(matches!(ayar_yukle(&yol), Yukleme::Bozuk(_)));
    }

    #[test]
    fn anlasilmayanlar_son_elli_en_yeni_once() {
        let metin: String = (0..60).map(|i| format!("metin {i}\n")).collect();
        let a = Anlasilmayanlar::coz(&metin);
        let g = a.gosterilecekler();
        assert_eq!(g.len(), ANLASILMAYAN_SINIRI);
        assert_eq!(g[0], (59, "metin 59"));
        assert_eq!(g[49], (10, "metin 10"));
        assert!(Anlasilmayanlar::coz("\n  \n").gosterilecekler().is_empty());
    }

    #[test]
    fn anlasilmayanlar_sil_temizle_yaz() {
        let k = GeciciKlasor::yeni("anlasilmayan");
        let yol = k.0.join("anlasilmayanlar.txt");
        assert_eq!(Anlasilmayanlar::oku(&yol).unwrap(), Anlasilmayanlar::default());
        std::fs::write(&yol, "bir\niki\nüç\n").unwrap();
        let mut a = Anlasilmayanlar::oku(&yol).unwrap();
        a.sil(1);
        a.yaz(&yol).unwrap();
        assert_eq!(std::fs::read_to_string(&yol).unwrap(), "bir\nüç\n");
        a.temizle();
        a.yaz(&yol).unwrap();
        assert_eq!(std::fs::read_to_string(&yol).unwrap(), "");
    }

    #[test]
    fn yedek_bir_kez_alinir_ve_ezilmez() {
        let k = GeciciKlasor::yeni("yedek");
        let yol = k.0.join("ayarlar.toml");
        assert_eq!(yedek_al(&yol), Ok(None), "özgün yokken yedek yok");
        std::fs::write(&yol, "# özgün yorum\n").unwrap();
        assert_eq!(yedek_al(&yol), Ok(Some(k.0.join("ayarlar.toml.bak"))));
        std::fs::write(&yol, "yeni").unwrap();
        assert_eq!(yedek_al(&yol), Ok(None));
        assert_eq!(std::fs::read_to_string(yedek_yolu(&yol)).unwrap(), "# özgün yorum\n");
    }

    #[test]
    fn bozuk_dosya_zaman_damgasiyla_saklanir() {
        let k = GeciciKlasor::yeni("bozukyedek");
        let yol = k.0.join("ayarlar.toml");
        let z = (2026, 11, 5, 9, 3, 7);
        assert_eq!(bozuk_yedek_yolu(&yol, z), k.0.join("ayarlar.toml.bozuk-20261105-090307"));
        assert_eq!(bozugu_sakla(&yol, z), Ok(None));
        std::fs::write(&yol, "bu = [bozuk").unwrap();
        let s = bozugu_sakla(&yol, z).unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(s).unwrap(), "bu = [bozuk");
        assert_eq!(son_gecerli_yolu(&yol), k.0.join("ayarlar.son-gecerli.toml"));
    }

    #[test]
    fn parmak_izi_dis_degisikligi_yakalar() {
        let k = GeciciKlasor::yeni("parmak");
        let yol = k.0.join("ayarlar.toml");
        assert_eq!(parmak_izi(&yol), None);
        ayar_kaydet(&yol, &varsayilan_ham()).unwrap();
        let once = parmak_izi(&yol);
        assert!(once.is_some());
        assert_eq!(parmak_izi(&yol), once);
        std::fs::write(&yol, std::fs::read_to_string(&yol).unwrap() + "\n# elle\n").unwrap();
        assert_ne!(parmak_izi(&yol), once);
    }

    #[test]
    fn kaymis_sirada_metinle_silme() {
        let mut a = Anlasilmayanlar::coz("x\ny\nx\nz\n");
        a.metni_sil(3, "z");
        assert_eq!(a.satirlar, vec!["x", "y", "x"]);
        // Sıra tutmuyor (dosyaya satır eklenmiş): metnin son geçtiği yer silinir.
        a.metni_sil(0, "y");
        assert_eq!(a.satirlar, vec!["x", "x"]);
        a.metni_sil(9, "yok");
        assert_eq!(a.satirlar.len(), 2);
    }
}
