//! Gömülü sürüm zip'i: özet doğrulama, güvenli açma ve eski geçici klasör temizliği.
//!
//! Zip açıcısı `uygulama/yanki/src/guncelleme.rs`'teki güncelleyiciyle AYNI reddetme kurallarına
//! uyar (mutlak yol, `..`, sürücü/akış adı, ayrılmış aygıt adı, nokta/boşlukla biten bileşen,
//! yinelenen girdi, şifreli girdi, sembolik bağ, desteklenmeyen sıkıştırma, boyut sınırları).
//! Kod kopyadır (ortak crate Ç ajanının alanını bozardı); testler de aynı vakaları taşır.

use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};

pub const EN_COK_ACILMIS: u64 = 1024 * 1024 * 1024;
pub const EN_COK_GIRDI: usize = 10_000;
/// Geçici klasör ön eki; temizlik yalnız bu önekli klasörlere dokunur.
pub const KLASOR_ON_EKI: &str = "Echora-Kurulum-";
/// Bu süreden eski kurulum klasörleri silinir.
pub const ESKI_SINIRI: Duration = Duration::from_secs(24 * 60 * 60);

/// Baytların SHA-256 özeti, küçük harf onaltılık.
pub fn ozet_hex(bayt: &[u8]) -> String {
    Sha256::digest(bayt).iter().map(|b| format!("{b:02x}")).collect()
}

/// Gömülü baytların özeti derleme anında gömülen özetle (büyük/küçük harf duyarsız) aynı mı.
pub fn ozet_dogru(bayt: &[u8], beklenen: &str) -> bool {
    beklenen.len() == 64 && ozet_hex(bayt).eq_ignore_ascii_case(beklenen)
}

fn ayrilmis_ad(p: &str) -> bool {
    let kok = p.split('.').next().unwrap_or("").to_uppercase();
    if matches!(kok.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") {
        return true;
    }
    // COM1-9, LPT1-9 ve Windows'un aygıt saydığı üst simge biçimleri (COM¹, LPT³ ...).
    let mut k = kok.chars();
    let on: String = k.by_ref().take(3).collect();
    let son: Vec<char> = k.collect();
    (on == "COM" || on == "LPT") && son.len() == 1 && matches!(son[0], '1'..='9' | '¹' | '²' | '³')
}

/// Zip girdisinin hedefe göre güvenli göreli yolu ve klasör olup olmadığı. `\` da ayırıcı sayılır.
pub fn zip_girdi_yolu(ad: &str) -> Result<(PathBuf, bool), String> {
    if ad.is_empty() || ad.contains('\0') {
        return Err("boş ya da NUL içeren girdi adı".into());
    }
    let n = ad.replace('\\', "/");
    if n.starts_with('/') {
        return Err(format!("mutlak yol reddedildi: {ad}"));
    }
    if n.contains(':') {
        return Err(format!("sürücü/akış adı reddedildi: {ad}"));
    }
    let klasor = n.ends_with('/');
    let mut yol = PathBuf::new();
    for p in n.split('/') {
        match p {
            "" | "." => continue,
            ".." => return Err(format!("yol geçişi reddedildi: {ad}")),
            _ if ayrilmis_ad(p) || p.ends_with('.') || p.ends_with(' ') => {
                return Err(format!("geçersiz dosya adı reddedildi: {ad}"));
            }
            _ => yol.push(p),
        }
    }
    if yol.as_os_str().is_empty() {
        return Err(format!("boş yol: {ad}"));
    }
    Ok((yol, klasor))
}

fn girdi_anahtari(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// Açma sonucu: girdi yolları (özgün büyük/küçük harfle; klasörler dahil).
fn zip_isle(bayt: &[u8], hedef: Option<&Path>) -> Result<Vec<PathBuf>, String> {
    use zip::CompressionMethod;
    let mut a = zip::ZipArchive::new(Cursor::new(bayt)).map_err(|h| format!("zip okunamadı: {h}"))?;
    if a.len() > EN_COK_GIRDI {
        return Err(format!("zip'te çok fazla girdi ({})", a.len()));
    }
    if let Some(h) = hedef {
        // Var olan klasör SİLİNMEZ (hedefi çağıran dışlamalı olarak yeni oluşturur).
        std::fs::create_dir_all(h).map_err(|x| format!("{} oluşturulamadı: {x}", h.display()))?;
    }
    let mut gorulen = HashSet::new();
    let mut girdiler = Vec::new();
    let mut toplam: u64 = 0;
    for i in 0..a.len() {
        let mut g = a.by_index(i).map_err(|h| format!("zip girdisi okunamadı: {h}"))?;
        let ad = g.name().map_err(|h| format!("zip girdi adı okunamadı: {h}"))?.into_owned();
        let (goreli, klasor) = zip_girdi_yolu(&ad)?;
        if g.encrypted() {
            return Err(format!("şifreli girdi reddedildi: {ad}"));
        }
        if g.is_symlink() {
            return Err(format!("sembolik bağ reddedildi: {ad}"));
        }
        if !matches!(g.compression(), CompressionMethod::Stored | CompressionMethod::Deflated) {
            return Err(format!("desteklenmeyen sıkıştırma ({:?}): {ad}", g.compression()));
        }
        if !gorulen.insert(girdi_anahtari(&goreli)) {
            return Err(format!("yinelenen girdi reddedildi: {ad}"));
        }
        let klasor = klasor || g.is_dir();
        girdiler.push(goreli.clone());
        let Some(h) = hedef else {
            toplam = toplam.saturating_add(g.size());
            if toplam > EN_COK_ACILMIS {
                return Err("açılmış boyut sınırı aşıldı".into());
            }
            continue;
        };
        let yol = h.join(&goreli);
        if klasor {
            std::fs::create_dir_all(&yol).map_err(|x| format!("{} oluşturulamadı: {x}", yol.display()))?;
            continue;
        }
        if let Some(u) = yol.parent() {
            std::fs::create_dir_all(u).map_err(|x| format!("{} oluşturulamadı: {x}", u.display()))?;
        }
        let mut cikti = std::fs::File::create(&yol).map_err(|x| format!("{} yazılamadı: {x}", yol.display()))?;
        let kalan = EN_COK_ACILMIS - toplam;
        // CRC denetimi okuma sonunda zip kitaplığınca yapılır (bozuk girdi hata verir).
        let n = std::io::copy(&mut (&mut g).take(kalan + 1), &mut cikti).map_err(|x| format!("{ad} açılamadı: {x}"))?;
        toplam += n;
        if toplam > EN_COK_ACILMIS {
            return Err("açılmış boyut sınırı aşıldı".into());
        }
    }
    Ok(girdiler)
}

/// Tüm girdiler tek bir kök klasörün altında olmalı ve `<kök>/echora.exe` bulunmalı. Dönen: kök adı.
pub fn kok_klasoru(girdiler: &[PathBuf]) -> Result<String, String> {
    let mut kok: Option<String> = None;
    let mut exe_var = false;
    for g in girdiler {
        let bilesenler: Vec<String> = g.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        let ilk = bilesenler.first().cloned().unwrap_or_default();
        match &kok {
            None => kok = Some(ilk.clone()),
            Some(k) if !k.eq_ignore_ascii_case(&ilk) => {
                return Err(format!("zip tek kök klasörlü değil ({k} ve {ilk})"));
            }
            _ => {}
        }
        if bilesenler.len() == 2 && bilesenler[1].eq_ignore_ascii_case("echora.exe") {
            exe_var = true;
        }
    }
    let kok = kok.ok_or("zip boş")?;
    if !exe_var {
        return Err(format!("{kok}/echora.exe zip'te yok"));
    }
    Ok(kok)
}

/// Zip'i açmadan denetler (girdi güvenliği, kök klasör, echora.exe). Dönen: girdi sayısı.
pub fn zip_denetle(bayt: &[u8]) -> Result<usize, String> {
    let g = zip_isle(bayt, None)?;
    kok_klasoru(&g)?;
    Ok(g.len())
}

/// Zip'i `hedef`e açar (hedef boş/yeni olmalı; var olan hiçbir şey silinmez). Dönen: `echora.exe`nin tam yolu.
pub fn zip_ac(bayt: &[u8], hedef: &Path) -> Result<PathBuf, String> {
    let g = zip_isle(bayt, Some(hedef))?;
    let kok = kok_klasoru(&g)?;
    let exe = hedef.join(kok).join("echora.exe");
    if !exe.is_file() {
        return Err(format!("{} açılamadı", exe.display()));
    }
    Ok(exe)
}

/// Saf karar: bu klasör eski bir kurulum artığı olarak silinsin mi? Yalnız ön ekli ve yeterince eski
/// klasörler; içindeki echora.exe çalışıyorsa (kullanımda) dokunulmaz. Yarım kalmış (echora.exe'siz)
/// eski klasörler de silinir.
pub fn eski_klasor_silinsin_mi(ad: &str, yas: Duration, kullanimda: bool) -> bool {
    let on_ek_uyuyor = ad.get(..KLASOR_ON_EKI.len()).is_some_and(|p| p.eq_ignore_ascii_case(KLASOR_ON_EKI));
    on_ek_uyuyor && ad.len() > KLASOR_ON_EKI.len() && yas > ESKI_SINIRI && !kullanimda
}

/// `exe` çalışıyor ya da başka bir işlemce açık mı? Yazma erişimi + paylaşımsız açmayı dener (dosyaya
/// yazılmaz, kısaltılmaz): çalışan bir imajın yazmaya açılması paylaşım ihlaliyle reddedilir.
pub fn exe_kullanimda_mi(exe: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().write(true).share_mode(0).open(exe).is_err()
}

/// Klasörde (kökte ya da bir alt klasörde) kullanımda olan bir echora.exe var mı?
fn echora_kullanimda_mi(klasor: &Path) -> bool {
    let mut adaylar = vec![klasor.join("echora.exe")];
    if let Ok(d) = std::fs::read_dir(klasor) {
        adaylar.extend(d.flatten().filter(|e| e.path().is_dir()).map(|e| e.path().join("echora.exe")));
    }
    adaylar.iter().any(|a| a.is_file() && exe_kullanimda_mi(a))
}

/// Yeni, benzersiz hedef klasörü DIŞLAMALI oluşturur (`create_dir`: var olan klasör hata verir, asla
/// silinmez/sahiplenilmez); ad çakışırsa yeni ek üretip yeniden dener. `ek` her çağrıda yeni rastgele
/// bir sonek döndürür.
pub fn yeni_hedef_klasoru(temp: &Path, pid: u32, zaman: u64, mut ek: impl FnMut() -> String) -> Result<PathBuf, String> {
    let mut son_hata = String::new();
    for _ in 0..16 {
        let yol = temp.join(format!("{KLASOR_ON_EKI}{pid}-{zaman}-{}", ek()));
        match std::fs::create_dir(&yol) {
            Ok(()) => return Ok(yol),
            Err(h) if h.kind() == std::io::ErrorKind::AlreadyExists => son_hata = format!("{} zaten var", yol.display()),
            Err(h) => return Err(format!("{} oluşturulamadı: {h}", yol.display())),
        }
    }
    Err(format!("benzersiz geçici klasör oluşturulamadı ({son_hata})"))
}

/// 128 bitlik rastgele onaltılık sonek. Rastgelelik std'nin `RandomState` anahtarlarından gelir
/// (işletim sistemi CSPRNG'si, std'nin zaten içe aktardığı bcryptprimitives.dll); ayrı bir
/// bcrypt.dll bağımlılığı eklememek için `BCryptGenRandom` doğrudan çağrılmaz.
pub fn rastgele_ek() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut s = String::new();
    for _ in 0..2 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(0);
        s.push_str(&format!("{:016x}", h.finish()));
    }
    s
}

/// `temp` altındaki eski kurulum klasörlerini siler (hatalar yok sayılır: exe çalışıyor olabilir).
/// Dönen: silinen klasör sayısı.
pub fn eskileri_temizle(temp: &Path, simdi: SystemTime) -> usize {
    let Ok(d) = std::fs::read_dir(temp) else { return 0 };
    let mut silinen = 0;
    for e in d.flatten() {
        let yol = e.path();
        let Some(ad) = e.file_name().to_str().map(str::to_owned) else { continue };
        let Ok(m) = e.metadata() else { continue };
        if !m.is_dir() {
            continue;
        }
        let yas = m.modified().ok().and_then(|t| simdi.duration_since(t).ok()).unwrap_or(Duration::ZERO);
        if eski_klasor_silinsin_mi(&ad, yas, echora_kullanimda_mi(&yol)) && std::fs::remove_dir_all(&yol).is_ok() {
            silinen += 1;
        }
    }
    silinen
}

#[cfg(test)]
mod testler {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    fn s(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn zip_yol_guvenligi() {
        assert_eq!(
            zip_girdi_yolu("Echora-0.1.0-win64/echora.exe").unwrap(),
            (PathBuf::from("Echora-0.1.0-win64").join("echora.exe"), false)
        );
        assert_eq!(zip_girdi_yolu("a\\b\\c.txt").unwrap().0, PathBuf::from("a").join("b").join("c.txt"));
        assert_eq!(zip_girdi_yolu("modeller/").unwrap(), (PathBuf::from("modeller"), true));
        assert_eq!(zip_girdi_yolu("./a/./b").unwrap().0, PathBuf::from("a").join("b"));
        for kotu in [
            "../echora.exe",
            "a/../../b",
            "a/..",
            "..\\x",
            "/etc/passwd",
            "\\Windows\\x",
            "C:/Windows/x",
            "C:x",
            "a/b:akis",
            "a/CON",
            "nul.txt",
            "a/com1.log",
            "a/LPT9",
            "a/CONIN$",
            "conout$.txt",
            "a/COM¹",
            "a/lpt³.log",
            "COM²",
            "a/b.",
            "a/b ",
            "",
            "./",
            "a\0b",
        ] {
            assert!(zip_girdi_yolu(kotu).is_err(), "{kotu:?}");
        }
        assert!(zip_girdi_yolu("a/COM10").is_ok());
        assert!(zip_girdi_yolu("console.txt").is_ok());
    }

    fn crc32(v: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in v {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
            }
        }
        !c
    }

    /// Elle yazılan en küçük zip: "saklanmış" girdiler (zip crate yalnız okuma özelliğiyle derli).
    fn zip_yaz(girdiler: &[(&str, &[u8])]) -> Vec<u8> {
        let mut b = Vec::new();
        let mut merkez = Vec::new();
        for (ad, veri) in girdiler {
            let konum = b.len() as u32;
            let crc = crc32(veri);
            let n = ad.len() as u16;
            let boy = veri.len() as u32;
            b.extend_from_slice(&0x04034b50u32.to_le_bytes());
            b.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            b.extend_from_slice(&crc.to_le_bytes());
            b.extend_from_slice(&boy.to_le_bytes());
            b.extend_from_slice(&boy.to_le_bytes());
            b.extend_from_slice(&n.to_le_bytes());
            b.extend_from_slice(&0u16.to_le_bytes());
            b.extend_from_slice(ad.as_bytes());
            b.extend_from_slice(veri);
            merkez.extend_from_slice(&0x02014b50u32.to_le_bytes());
            merkez.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            merkez.extend_from_slice(&crc.to_le_bytes());
            merkez.extend_from_slice(&boy.to_le_bytes());
            merkez.extend_from_slice(&boy.to_le_bytes());
            merkez.extend_from_slice(&n.to_le_bytes());
            merkez.extend_from_slice(&[0; 12]);
            merkez.extend_from_slice(&konum.to_le_bytes());
            merkez.extend_from_slice(ad.as_bytes());
        }
        let merkez_konum = b.len() as u32;
        b.extend_from_slice(&merkez);
        let adet = girdiler.len() as u16;
        b.extend_from_slice(&0x06054b50u32.to_le_bytes());
        b.extend_from_slice(&[0, 0, 0, 0]);
        b.extend_from_slice(&adet.to_le_bytes());
        b.extend_from_slice(&adet.to_le_bytes());
        b.extend_from_slice(&(merkez.len() as u32).to_le_bytes());
        b.extend_from_slice(&merkez_konum.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b
    }

    /// Süreç kimlikli, kendi kendini temizleyen geçici klasör.
    struct Gecici(PathBuf);
    impl Gecici {
        fn yeni(etiket: &str) -> Self {
            let p = std::env::temp_dir().join(format!("kurulum-test-{}-{etiket}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Gecici(p)
        }
    }
    impl Drop for Gecici {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn zip_acma_ve_kotu_girdi() {
        let t = Gecici::yeni("acma");
        let iyi = zip_yaz(&[("Echora-0.2.0-win64/echora.exe", b"exe"), ("Echora-0.2.0-win64/README.md", b"merhaba")]);
        let hedef = t.0.join("acik");
        let exe = zip_ac(&iyi, &hedef).unwrap();
        assert_eq!(exe, hedef.join("Echora-0.2.0-win64").join("echora.exe"));
        assert_eq!(std::fs::read_to_string(hedef.join("Echora-0.2.0-win64").join("README.md")).unwrap(), "merhaba");
        assert_eq!(zip_denetle(&iyi).unwrap(), 2);

        let kotu = zip_yaz(&[("../disari.txt", b"x")]);
        let hedef2 = t.0.join("ic").join("acik");
        assert!(zip_ac(&kotu, &hedef2).unwrap_err().contains("yol geçişi"));
        assert!(!t.0.join("ic").join("disari.txt").exists());
        assert!(!t.0.join("disari.txt").exists());

        let ters = zip_yaz(&[("a\\..\\..\\disari2.txt", b"x")]);
        assert!(zip_ac(&ters, &hedef2).is_err());
        assert!(!t.0.join("disari2.txt").exists());
    }

    #[test]
    fn zip_yinelenen_girdi_reddedilir() {
        let t = Gecici::yeni("yinelenen");
        let z = zip_yaz(&[("E/libvosk.dll", b"iyi"), ("e\\LIBVOSK.DLL", b"kotu")]);
        assert!(zip_ac(&z, &t.0.join("a")).unwrap_err().contains("yinelenen"));
        assert!(zip_denetle(&z).is_err());
    }

    #[test]
    fn zip_kok_klasoru_ve_exe_ister() {
        // echora.exe yok.
        assert!(zip_denetle(&zip_yaz(&[("E/a.dll", b"1")])).unwrap_err().contains("echora.exe"));
        // Kök klasörsüz (tek bileşenli) girdi.
        assert!(zip_denetle(&zip_yaz(&[("echora.exe", b"1")])).is_err());
        // İki farklı kök.
        assert!(zip_denetle(&zip_yaz(&[("E/echora.exe", b"1"), ("F/x.dll", b"2")])).unwrap_err().contains("tek kök"));
        // echora.exe iç içe klasörde: sayılmaz.
        assert!(zip_denetle(&zip_yaz(&[("E/alt/echora.exe", b"1")])).is_err());
        // Büyük/küçük harf duyarsız.
        assert!(zip_denetle(&zip_yaz(&[("E/ECHORA.EXE", b"1")])).is_ok());
        // Boş zip ve zip olmayan bayt.
        assert!(zip_denetle(&zip_yaz(&[])).is_err());
        assert!(zip_denetle(b"bu zip degil").is_err());
    }

    #[test]
    fn gomulu_ozet_dogrulamasi() {
        let bayt = b"echora paketi";
        let ozet = ozet_hex(bayt);
        assert_eq!(ozet.len(), 64);
        // Bilinen SHA-256("abc").
        assert_eq!(ozet_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert!(ozet_dogru(bayt, &ozet));
        assert!(ozet_dogru(bayt, &ozet.to_uppercase()));
        assert!(!ozet_dogru(b"echora paketi!", &ozet));
        assert!(!ozet_dogru(bayt, ""));
        assert!(!ozet_dogru(bayt, &ozet[..63]));
    }

    #[test]
    fn eski_klasor_karari() {
        let gun = 24 * 3600;
        // (ad, yaş, kullanımda)
        assert!(eski_klasor_silinsin_mi("Echora-Kurulum-123-456", s(gun + 1), false));
        assert!(eski_klasor_silinsin_mi("echora-kurulum-1-2", s(5 * gun), false));
        // Tam 1 gün henüz eski sayılmaz; taze klasör de değil.
        assert!(!eski_klasor_silinsin_mi("Echora-Kurulum-1-2", s(gun), false));
        assert!(!eski_klasor_silinsin_mi("Echora-Kurulum-1-2", s(60), false));
        // Çalışan echora.exe içeren klasöre dokunulmaz.
        assert!(!eski_klasor_silinsin_mi("Echora-Kurulum-1-2", s(5 * gun), true));
        // Ön ek uymuyor.
        assert!(!eski_klasor_silinsin_mi("Baska-Klasor", s(5 * gun), false));
        assert!(!eski_klasor_silinsin_mi("Echora-Kurulum-", s(5 * gun), false));
        assert!(!eski_klasor_silinsin_mi("Echora", s(5 * gun), false));
        // Çok baytlı karakter ön ek sınırına denk gelse de panik yok.
        assert!(!eski_klasor_silinsin_mi("Echora-Kurulıım-1", s(5 * gun), false));
    }

    #[test]
    fn calisan_exe_kullanimda_sayilir() {
        // Bu test ikilisi şu an çalışıyor: yazmaya paylaşımsız açılamaz.
        assert!(exe_kullanimda_mi(&std::env::current_exe().unwrap()));
        let t = Gecici::yeni("kullanim");
        let f = t.0.join("a.exe");
        std::fs::write(&f, b"x").unwrap();
        assert!(!exe_kullanimda_mi(&f));
        let acik = std::fs::File::options().write(true).share_mode(0).open(&f).unwrap();
        assert!(exe_kullanimda_mi(&f));
        drop(acik);
        assert!(!exe_kullanimda_mi(&f));
        assert_eq!(std::fs::read(&f).unwrap(), b"x");
    }

    #[test]
    fn hedef_klasoru_dislamali_olusur_ve_varsa_silinmez() {
        let t = Gecici::yeni("hedef");
        // Önceden aynı adla var olan klasörün içeriği korunmalı, yeni ad denenmeli.
        let onceden = t.0.join(format!("{KLASOR_ON_EKI}7-9-ayni"));
        std::fs::create_dir_all(&onceden).unwrap();
        std::fs::write(onceden.join("dokunma.txt"), b"x").unwrap();
        let mut n = 0;
        let yol = yeni_hedef_klasoru(&t.0, 7, 9, || {
            n += 1;
            if n == 1 { "ayni".into() } else { "yeni".into() }
        })
        .unwrap();
        assert_eq!(yol, t.0.join(format!("{KLASOR_ON_EKI}7-9-yeni")));
        assert!(yol.is_dir());
        assert!(onceden.join("dokunma.txt").is_file());
        // Sürekli çakışma: sonsuz döngü yok, hata döner.
        assert!(yeni_hedef_klasoru(&t.0, 7, 9, || "ayni".into()).is_err());
        // Rastgele ekler benzersiz ve 32 onaltılık karakter.
        let (a, b) = (rastgele_ek(), rastgele_ek());
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn eskileri_temizle_yalniz_uygun_klasorleri_siler() {
        let t = Gecici::yeni("temizlik");
        let ac = |ad: &str, exe: bool| {
            let k = t.0.join(ad).join("Echora-0.1.0-win64");
            std::fs::create_dir_all(&k).unwrap();
            if exe {
                std::fs::write(k.join("echora.exe"), b"x").unwrap();
            }
        };
        ac("Echora-Kurulum-1-1", true);
        ac("Echora-Kurulum-2-2", false); // yarım kalmış: exe'siz
        ac("Echora-Kurulum-4-4", true); // exe kullanımda (açık tutulur)
        ac("Baska", true);
        let kullanimda = std::fs::File::options()
            .write(true)
            .share_mode(0)
            .open(t.0.join("Echora-Kurulum-4-4").join("Echora-0.1.0-win64").join("echora.exe"))
            .unwrap();
        // "Şimdi" 3 gün sonrası: hepsi eski sayılır; ön ekli ve kullanımda olmayanlar silinmeli.
        let simdi = SystemTime::now() + Duration::from_secs(3 * 24 * 3600);
        assert_eq!(eskileri_temizle(&t.0, simdi), 2);
        assert!(!t.0.join("Echora-Kurulum-1-1").exists());
        assert!(!t.0.join("Echora-Kurulum-2-2").exists());
        assert!(t.0.join("Echora-Kurulum-4-4").exists());
        assert!(t.0.join("Baska").exists());
        drop(kullanimda);
        // Gerçek "şimdi": taze klasörler silinmez.
        ac("Echora-Kurulum-3-3", true);
        assert_eq!(eskileri_temizle(&t.0, SystemTime::now()), 0);
        assert!(t.0.join("Echora-Kurulum-3-3").exists());
    }
}
