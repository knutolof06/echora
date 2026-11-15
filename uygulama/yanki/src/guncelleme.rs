//! Güncelleyici: GitHub Releases'ten son sürümü denetler, daha yeniyse indirir, SHA-256 ile
//! doğrular ve açar; durum `%LOCALAPPDATA%\Echora\guncelleme\durum.json`'a yazılır (Stüdyo okur).
//! Kurulum ASLA onaysız başlamaz: onaydan sonra yeni exe `--guncelle-uygula` ile ayrı süreç olarak
//! başlatılır, bu süreç kapanır; yeni süreç eskisinin bitmesini bekleyip dosyaları değiştirir
//! (yedekli, hata olursa geri alır) ve Echora'yı yeniden başlatır.
//!
//! Güvenlik kararları:
//! - Ağ: WinHTTP, yalnız HTTPS (TLS 1.2+), yalnız `IZINLI_ANA_MAKINELER`. Yönlendirmeler ELLE
//!   izlenir: her hedef bağlanmadan önce izin listesinden geçer; yanıttan sonra `WINHTTP_OPTION_URL`
//!   ile son adres yeniden denetlenir. İndirme adresleri `https://github.com/<DEPO>/releases/download/`
//!   ile başlamalıdır. Boyut sınırları: API 4 MB, özet 64 KB, zip 300 MB, açılmış 1 GB.
//! - Bütünlük: zip, aynı sürümdeki `SHA256SUMS.txt`'deki özetle karşılaştırılır. Bu yalnız
//!   bütünlüğü (bozuk/yarım indirme) korur; kaynağın kimliğini GitHub hesabı belirler.
//! - Zip: yalnız saklanmış/deflate girdiler; yol geçişi (`..`), mutlak yol, sürücü/akış adı,
//!   ayrılmış aygıt adı, sembolik bağ, şifreli girdi ve büyük/küçük harf duyarsız yinelenen girdi
//!   REDDEDİLİR.
//! - TOCTOU: beklenen özet tepsi sürecinin BELLEĞİNDE tutulur; onaydan hemen sonra zip'in özeti
//!   yeniden hesaplanır ve zip yeni bir klasöre açılır. Süreç yeniden başladıysa diskteki "hazır"a
//!   güvenilmez: zip GitHub'daki SHA256SUMS ile yeniden doğrulanır.
//! - `--guncelle-uygula` yalnız `<guncelleme>\<kendi sürümü>\` altından çalışan ve kurulu exe'den
//!   (sürüm kaynağı) YENİ olan exe için kabul edilir.
//! - `--kuru` kipinde bu modülün ağ kodu hiç çağrılmaz (bkz. `tepsi`).
//!
//! Saf işlevler (`Surum`, `adres_coz`, `ozet_bul`, `yayin_karari`, `zip_girdi_yolu`,
//! `kurulum_karari`) ağsız birim testlidir.

use std::ffi::c_void;
use std::fmt;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use studyo::guncelleme::{self as durum, DurumTuru, GuncellemeDurumu, DEPO};
use studyo::ipc::ayni_yol;
use studyo::kurulum::{self, Konum};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpQueryHeaders, WinHttpQueryOption, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetOption, WinHttpSetTimeouts, INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_DISABLE_REDIRECTS, WINHTTP_FLAG_SECURE, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2,
    WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3, WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_OPTION_SECURE_PROTOCOLS,
    WINHTTP_OPTION_URL, WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_LOCATION,
    WINHTTP_QUERY_STATUS_CODE,
};
use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};

use crate::gunluk;

/// Bu derlemenin sürümü.
pub const SURUM: &str = env!("CARGO_PKG_VERSION");
/// Bağlanılmasına izin verilen ana makineler (birebir eşleşme).
pub const IZINLI_ANA_MAKINELER: [&str; 4] =
    ["api.github.com", "github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"];
pub const OZET_DOSYASI: &str = "SHA256SUMS.txt";
const EN_COK_API: u64 = 4 * 1024 * 1024;
const EN_COK_OZET: u64 = 64 * 1024;
pub const EN_COK_INDIRME: u64 = 300 * 1024 * 1024;
const EN_COK_ACILMIS: u64 = 1024 * 1024 * 1024;
const EN_COK_GIRDI: usize = 10_000;
const EN_COK_YONLENDIRME: u32 = 5;
/// Eski sürecin kapanması için beklenen en uzun süre (`--bekle`).
const BEKLEME: Duration = Duration::from_secs(15);

// --- Sürüm ---

/// Önsürümsüz anlamsal sürüm (X.Y.Z). Sıralama alan sırasıyla (ana, ara, yama).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Surum {
    pub ana: u64,
    pub ara: u64,
    pub yama: u64,
}

impl Surum {
    /// "1.2.3" ya da "v1.2.3". Önsürüm/derleme eki (`-beta.1`, `+x`), eksik/fazla parça, baştaki
    /// sıfır ya da sayı dışı karakter → `None` (önsürümler güncelleme sayılmaz).
    pub fn coz(s: &str) -> Option<Surum> {
        let s = s.trim();
        let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
        let p: Vec<&str> = s.split('.').collect();
        if p.len() != 3 {
            return None;
        }
        let sayi = |x: &str| -> Option<u64> {
            if x.is_empty() || x.len() > 9 || !x.bytes().all(|b| b.is_ascii_digit()) || (x.len() > 1 && x.starts_with('0')) {
                return None;
            }
            x.parse().ok()
        };
        Some(Surum { ana: sayi(p[0])?, ara: sayi(p[1])?, yama: sayi(p[2])? })
    }

    /// Bu derlemenin sürümü (önsürüm eki varsa çekirdeği).
    pub fn simdiki() -> Surum {
        Surum::coz(SURUM.split(['-', '+']).next().unwrap_or("0.0.0")).unwrap_or(Surum { ana: 0, ara: 0, yama: 0 })
    }
}

impl fmt::Display for Surum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.ana, self.ara, self.yama)
    }
}

/// Sürüm zip'inin adı: `Echora-X.Y.Z-win64.zip` (paketleme betiğiyle aynı).
pub fn varlik_adi(s: Surum) -> String {
    format!("Echora-{s}-win64.zip")
}

// --- Adresler ---

/// Ayrıştırılmış https adresi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adres {
    /// Küçük harf ana makine.
    pub ana: String,
    /// `/` ile başlayan yol + sorgu (parça atılır).
    pub yol: String,
}

impl Adres {
    #[cfg(test)]
    pub fn metin(&self) -> String {
        format!("https://{}{}", self.ana, self.yol)
    }
}

/// Yalnız `https://ana[:443]/yol` biçimi. Kullanıcı bilgisi (`@`), 443 dışı bağlantı noktası,
/// geçersiz ana makine karakteri, boşluk/denetim karakteri reddedilir.
pub fn adres_coz(url: &str) -> Result<Adres, String> {
    let u = url.trim();
    if u.len() > 8192 {
        return Err("adres çok uzun".into());
    }
    let kalan = match u.get(..8) {
        Some(b) if b.eq_ignore_ascii_case("https://") => &u[8..],
        _ => return Err("yalnız https adresleri kabul edilir".into()),
    };
    let son = kalan.find(['/', '?', '#']).unwrap_or(kalan.len());
    let (yetki, yol) = kalan.split_at(son);
    if yetki.contains('@') {
        return Err("kullanıcı bilgisi içeren adres reddedildi".into());
    }
    let ana = match yetki.rsplit_once(':') {
        Some((a, "443")) => a,
        Some(_) => return Err("443 dışı bağlantı noktası reddedildi".into()),
        None => yetki,
    };
    if ana.is_empty() || !ana.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.') {
        return Err("geçersiz ana makine".into());
    }
    let yol = yol.split('#').next().unwrap_or("");
    if yol.chars().any(|c| c.is_control() || c == ' ') {
        return Err("adreste boşluk ya da denetim karakteri var".into());
    }
    let yol = if yol.is_empty() {
        "/".to_string()
    } else if yol.starts_with('?') {
        format!("/{yol}")
    } else {
        yol.to_string()
    };
    Ok(Adres { ana: ana.to_ascii_lowercase(), yol })
}

pub fn ana_makine_izinli(ana: &str) -> bool {
    IZINLI_ANA_MAKINELER.contains(&ana)
}

/// Ayrıştırır ve izin listesinden geçirir.
pub fn izinli_adres(url: &str) -> Result<Adres, String> {
    let a = adres_coz(url)?;
    if !ana_makine_izinli(&a.ana) {
        return Err(format!("izin verilmeyen ana makine: {}", a.ana));
    }
    Ok(a)
}

/// `Location` başlığının hedefi: mutlak https adresi ya da aynı ana makinede `/` ile başlayan yol.
/// Şemasız (`//x`) ya da göreli yollar reddedilir.
pub fn yonlendirme_hedefi(mevcut: &Adres, konum: &str) -> Result<Adres, String> {
    let k = konum.trim();
    if k.starts_with("//") {
        return Err("şemasız yönlendirme reddedildi".into());
    }
    if k.starts_with('/') {
        return izinli_adres(&format!("https://{}{}", mevcut.ana, k));
    }
    izinli_adres(k)
}

// --- SHA256SUMS ---

fn onaltilik_coz(h: &str) -> Option<[u8; 32]> {
    if h.len() != 64 || !h.is_ascii() {
        return None;
    }
    let mut o = [0u8; 32];
    for (i, b) in o.iter_mut().enumerate() {
        *b = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(o)
}

pub fn onaltilik(o: &[u8]) -> String {
    o.iter().map(|b| format!("{b:02x}")).collect()
}

/// `sha256sum` biçimi (`<64 onaltılık>  <ad>` ya da `<özet> *<ad>`) içinde `ad`ın özeti. Aynı ad
/// farklı özetlerle birden çok kez geçiyorsa `None`.
pub fn ozet_bul(metin: &str, ad: &str) -> Option<[u8; 32]> {
    let mut bulunan: Option<[u8; 32]> = None;
    for satir in metin.lines() {
        let s = satir.trim().trim_start_matches('\u{feff}');
        let Some((h, dosya)) = s.split_once(char::is_whitespace) else { continue };
        let dosya = dosya.trim_start().trim_start_matches('*').trim();
        if dosya != ad {
            continue;
        }
        let o = onaltilik_coz(h)?;
        match bulunan {
            Some(b) if b != o => return None,
            _ => bulunan = Some(o),
        }
    }
    bulunan
}

// --- GitHub sürüm yanıtı ---

#[derive(Deserialize)]
struct ApiSurumu {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<ApiVarligi>,
}

#[derive(Deserialize)]
struct ApiVarligi {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Yayin {
    pub surum: Surum,
    pub zip_url: String,
    pub zip_boyut: u64,
    pub ozet_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YayinKarari {
    Guncel,
    Yeni(Yayin),
}

/// `releases/latest` yanıtından karar. Taslak, önsürüm ya da önsürüm etiketi (`v1.0.0-beta`)
/// güncelleme sayılmaz. Yeni sürümde zip ve `SHA256SUMS.txt` varlıkları bulunmalı ve indirme
/// adresleri bu deponun sürüm indirme yolunda olmalıdır.
pub fn yayin_karari(json: &str, simdiki: Surum) -> Result<YayinKarari, String> {
    let r: ApiSurumu = serde_json::from_str(json).map_err(|h| format!("sürüm bilgisi okunamadı: {h}"))?;
    if r.draft || r.prerelease {
        return Ok(YayinKarari::Guncel);
    }
    let Some(s) = Surum::coz(&r.tag_name) else { return Ok(YayinKarari::Guncel) };
    if s <= simdiki {
        return Ok(YayinKarari::Guncel);
    }
    let onek = format!("https://github.com/{DEPO}/releases/download/").to_ascii_lowercase();
    let zip_adi = varlik_adi(s);
    let bul = |ad: &str| r.assets.iter().find(|v| v.name == ad);
    let zip = bul(&zip_adi).ok_or_else(|| format!("{} sürümünde {zip_adi} yok", r.tag_name))?;
    let ozet = bul(OZET_DOSYASI).ok_or_else(|| format!("{} sürümünde {OZET_DOSYASI} yok", r.tag_name))?;
    for v in [zip, ozet] {
        if !v.browser_download_url.to_ascii_lowercase().starts_with(&onek) {
            return Err(format!("beklenmeyen indirme adresi: {}", v.name));
        }
        izinli_adres(&v.browser_download_url)?;
    }
    if zip.size > EN_COK_INDIRME {
        return Err(format!("sürüm dosyası çok büyük ({} bayt)", zip.size));
    }
    Ok(YayinKarari::Yeni(Yayin {
        surum: s,
        zip_url: zip.browser_download_url.clone(),
        zip_boyut: zip.size,
        ozet_url: ozet.browser_download_url.clone(),
    }))
}

// --- Zip ---

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
/// Mutlak yol, `..`, sürücü harfi / NTFS akışı (`:`), NUL, ayrılmış aygıt adı ve nokta/boşlukla
/// biten bileşen REDDEDİLİR.
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

/// Yinelenen girdi denetimi için anahtar: küçük harf, `/` ayırıcılı (Windows dosya sistemi büyük/
/// küçük harf duyarsızdır; "A.dll" ile "a.dll" aynı dosyanın üzerine yazardı).
fn girdi_anahtari(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// Zip'i açar ve her girdiyi denetler; `hedef` verilirse oraya yazar (önce boşaltılır). Dönen:
/// (açılan bayt, dosya yolları anahtarları).
fn zip_isle(zip: &Path, hedef: Option<&Path>) -> Result<(u64, std::collections::HashSet<String>), String> {
    use zip::CompressionMethod;
    let f = std::fs::File::open(zip).map_err(|h| format!("{} açılamadı: {h}", zip.display()))?;
    let mut a = zip::ZipArchive::new(BufReader::new(f)).map_err(|h| format!("zip okunamadı: {h}"))?;
    if a.len() > EN_COK_GIRDI {
        return Err(format!("zip'te çok fazla girdi ({})", a.len()));
    }
    if let Some(h) = hedef {
        let _ = std::fs::remove_dir_all(h);
        std::fs::create_dir_all(h).map_err(|x| format!("{} oluşturulamadı: {x}", h.display()))?;
    }
    let mut gorulen = std::collections::HashSet::new();
    let mut dosyalar = std::collections::HashSet::new();
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
        let anahtar = girdi_anahtari(&goreli);
        if !gorulen.insert(anahtar.clone()) {
            return Err(format!("yinelenen girdi reddedildi: {ad}"));
        }
        let klasor = klasor || g.is_dir();
        if !klasor {
            dosyalar.insert(anahtar);
        }
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
    Ok((toplam, dosyalar))
}

/// Zip'i `hedef`e açar (önce boşaltılır). Dönen: açılan bayt.
pub fn zip_ac(zip: &Path, hedef: &Path) -> Result<u64, String> {
    zip_isle(zip, Some(hedef)).map(|(n, _)| n)
}

/// İndirilen sürüm zip'ini açmadan denetler: girdi güvenliği ve kurulum için gereken dosyalar
/// (`Echora-X.Y.Z-win64/` altında).
pub fn zip_denetle(zip: &Path, s: Surum) -> Result<(), String> {
    let (_, dosyalar) = zip_isle(zip, None)?;
    let onek = format!("echora-{s}-win64/");
    let mut gerekli: Vec<String> = vec![kurulum::EXE_ADI.to_string()];
    gerekli.extend(kurulum::DLLLER.iter().map(|d| d.to_string()));
    gerekli.push(format!("{}/final.mdl", kurulum::MODEL_GORELI.replace('\\', "/")));
    for g in gerekli {
        if !dosyalar.contains(&format!("{onek}{}", g.to_lowercase())) {
            return Err(format!("sürüm zip'inde {g} yok"));
        }
    }
    Ok(())
}

// --- Kurulum kararı ---

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KurulumKarari {
    /// Kurulu konumdan çalışıyor: bu klasör güncellenir.
    Kurulu(PathBuf),
    /// `\target\` altından: geliştirme sürümü güncellenmez.
    Gelistirme,
    /// Başka bir yerden (açılmış zip vb.): güncellenmez.
    KuruluDegil,
}

pub fn kurulum_karari(exe: &Path, kurulum_klasoru: Option<&Path>) -> KurulumKarari {
    match kurulum::konum(exe, kurulum_klasoru) {
        Konum::Kurulu => KurulumKarari::Kurulu(exe.parent().map(Path::to_path_buf).unwrap_or_default()),
        Konum::Gelistirme => KurulumKarari::Gelistirme,
        Konum::Kurulmamis => KurulumKarari::KuruluDegil,
    }
}

/// Güncelleyici kaynağı `<guncelleme klasörü>\<kendi sürümü>\` altında mı? (`--guncelle-uygula`
/// yalnız indirilen sürümün kendi klasöründen çalışan exe için kabul edilir.)
pub fn guncelleyici_kaynagi_gecerli(kaynak: &Path, klasor: &Path, s: Surum) -> bool {
    let k = studyo::ipc::yol_anahtari(kaynak);
    let ust = studyo::ipc::yol_anahtari(&surum_klasoru(klasor, s));
    let ust = ust.trim_end_matches('\\');
    k.starts_with(&format!("{ust}\\"))
}

/// Exe'nin sürüm kaynağındaki ürün sürümü (VS_FIXEDFILEINFO). Okunamazsa `None`.
pub fn exe_surumu(exe: &Path) -> Option<Surum> {
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO};
    let yol = genis(&exe.to_string_lossy());
    // SAFETY: yol NUL'lu; tampon boyutu sorgudan; VerQueryValueW tampon içini gösterir.
    unsafe {
        let boyut = GetFileVersionInfoSizeW(PCWSTR(yol.as_ptr()), None);
        if boyut == 0 {
            return None;
        }
        let mut tampon = vec![0u8; boyut as usize];
        GetFileVersionInfoW(PCWSTR(yol.as_ptr()), None, boyut, tampon.as_mut_ptr().cast()).ok()?;
        let mut p: *mut c_void = std::ptr::null_mut();
        let mut n: u32 = 0;
        if !VerQueryValueW(tampon.as_ptr().cast(), windows::core::w!("\\"), &mut p, &mut n).as_bool()
            || p.is_null()
            || (n as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>()
        {
            return None;
        }
        let f = &*(p as *const VS_FIXEDFILEINFO);
        if f.dwSignature != 0xFEEF04BD {
            return None;
        }
        Some(Surum {
            ana: u64::from(f.dwProductVersionMS >> 16),
            ara: u64::from(f.dwProductVersionMS & 0xFFFF),
            yama: u64::from(f.dwProductVersionLS >> 16),
        })
    }
}

// --- Klasörler ve durum ---

fn surum_klasoru(klasor: &Path, s: Surum) -> PathBuf {
    klasor.join(s.to_string())
}

/// İndirilip doğrulanmış sürüm zip'inin yolu (kurulumdan sonra klasörüyle silinir).
pub fn zip_yolu(klasor: &Path, s: Surum) -> PathBuf {
    surum_klasoru(klasor, s).join(varlik_adi(s))
}

/// Sürüm klasörlerini siler: `simdiki`den eski ya da ona eşit olanlar ve (verildiyse) `koru`
/// dışındaki daha yeni olanlar. Silinemeyen (kullanımda) klasör bir sonraki denetimde yeniden denenir.
pub fn surum_klasorlerini_temizle(klasor: &Path, simdiki: Surum, koru: Option<Surum>) {
    let Ok(liste) = std::fs::read_dir(klasor) else { return };
    for g in liste.flatten() {
        let ad = g.file_name().to_string_lossy().into_owned();
        let Some(s) = Surum::coz(&ad) else { continue };
        let sil = s <= simdiki || koru.is_some_and(|k| k != s);
        if sil && g.path().is_dir() {
            let _ = std::fs::remove_dir_all(g.path());
        }
    }
}

fn durum_yaz(klasor: &Path, d: &GuncellemeDurumu) {
    if let Err(h) = d.yaz(&durum::durum_yolu(klasor)) {
        gunluk::yaz(&format!("UYARI: güncelleme durumu yazılamadı: {h}"));
    }
}

/// Önceki kurulum hatası metninin öneki (açılışta bir kez balonla bildirilir).
pub const KURULUM_HATASI: &str = "kurulum başarısız";

/// Açılışta durum dosyasını toparlar. "İndiriliyor" (yarım kalmış) ve "hazır" (bu süreç
/// doğrulamadı; zip bir sonraki denetimde GitHub'daki SHA256SUMS ile yeniden doğrulanır) "güncel"e
/// döner. Bildirilmemiş kurulum hatası varsa metni döner ve bildirildi olarak işaretlenir.
pub fn baslangic_durumu(klasor: &Path) -> Option<String> {
    let yol = durum::durum_yolu(klasor);
    let mut d = GuncellemeDurumu::oku(&yol).ok().flatten()?;
    match d.durum {
        DurumTuru::Indiriliyor | DurumTuru::Hazir => {
            d.durum = DurumTuru::Guncel;
            d.surum = None;
            d.hata = None;
            d.bildirildi = false;
            durum_yaz(klasor, &d);
            None
        }
        DurumTuru::Hata if !d.bildirildi && d.hata.as_deref().is_some_and(|h| h.starts_with(KURULUM_HATASI)) => {
            d.bildirildi = true;
            durum_yaz(klasor, &d);
            d.hata
        }
        _ => None,
    }
}

/// Son denetimin zamanı (Unix sn).
pub fn son_denetim(klasor: &Path) -> Option<u64> {
    GuncellemeDurumu::oku(&durum::durum_yolu(klasor)).ok().flatten()?.son_denetim
}

/// Otomatik denetim zamanı geldi mi: hiç denetlenmediyse, 24 saat geçtiyse ya da saat geri
/// alınmışsa (son denetim bir saatten fazla gelecekte).
pub fn denetim_zamani_mi(son: Option<u64>, simdi: u64) -> bool {
    match son {
        None => true,
        Some(t) if t > simdi.saturating_add(3600) => true,
        Some(t) => simdi.saturating_sub(t) >= 24 * 3600,
    }
}

// --- WinHTTP ---

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgHatasi {
    Ag(String),
    Durum(u32),
    CokBuyuk,
    Izinsiz(String),
}

impl fmt::Display for AgHatasi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgHatasi::Ag(m) => write!(f, "ağ hatası: {m}"),
            AgHatasi::Durum(404) => f.write_str("bulunamadı (HTTP 404): depoda yayınlanmış sürüm yok"),
            AgHatasi::Durum(403) => f.write_str("GitHub isteği reddetti (HTTP 403; istek sınırı aşılmış olabilir)"),
            AgHatasi::Durum(k) => write!(f, "beklenmeyen HTTP yanıtı {k}"),
            AgHatasi::CokBuyuk => f.write_str("yanıt boyut sınırını aşıyor"),
            AgHatasi::Izinsiz(m) => write!(f, "güvenlik: {m}"),
        }
    }
}

fn win_hatasi(adim: &str) -> AgHatasi {
    let e = windows::core::Error::from_thread();
    let kod = (e.code().0 as u32) & 0xFFFF;
    let aciklama = match kod {
        12002 => "zaman aşımı".to_string(),
        12007 => "ad çözülemedi (internet bağlantısı yok olabilir)".to_string(),
        12029 => "sunucuya bağlanılamadı".to_string(),
        12175 | 12045 | 12169 => "güvenli bağlantı kurulamadı (sertifika/TLS)".to_string(),
        _ => e.message(),
    };
    AgHatasi::Ag(format!("{adim}: {aciklama} ({kod})"))
}

/// WinHTTP tutamacı; düşerken kapanır.
struct Tutamac(*mut c_void);

impl Tutamac {
    fn yeni(p: *mut c_void, adim: &str) -> Result<Self, AgHatasi> {
        if p.is_null() {
            Err(win_hatasi(adim))
        } else {
            Ok(Tutamac(p))
        }
    }
}

impl Drop for Tutamac {
    fn drop(&mut self) {
        // SAFETY: WinHTTP'nin döndürdüğü geçerli tutamaç; bir kez kapatılır.
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

fn genis(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn secenek_u32(h: &Tutamac, secenek: u32, deger: u32) -> windows::core::Result<()> {
    // SAFETY: geçerli tutamaç; tampon çağrı boyunca yaşar.
    unsafe { WinHttpSetOption(Some(h.0 as *const c_void), secenek, Some(&deger.to_le_bytes())) }
}

/// Bayt uzunluğu önce sorgulanan metin başlığı / seçeneği okur.
fn metin_oku(mut sorgu: impl FnMut(Option<*mut c_void>, &mut u32) -> windows::core::Result<()>) -> Option<String> {
    let mut bayt: u32 = 0;
    let _ = sorgu(None, &mut bayt);
    if bayt == 0 || bayt > 65_536 {
        return None;
    }
    let mut tampon = vec![0u16; (bayt as usize).div_ceil(2) + 1];
    let mut bayt = (tampon.len() * 2) as u32;
    sorgu(Some(tampon.as_mut_ptr().cast()), &mut bayt).ok()?;
    let n = (bayt as usize / 2).min(tampon.len());
    let son = tampon[..n].iter().position(|&c| c == 0).unwrap_or(n);
    Some(String::from_utf16_lossy(&tampon[..son]))
}

fn baslik(istek: &Tutamac, bilgi: u32) -> Option<String> {
    metin_oku(|tampon, bayt| {
        // SAFETY: geçerli istek tutamacı; tampon (varsa) `bayt` uzunluğunda.
        unsafe { WinHttpQueryHeaders(istek.0, bilgi, PCWSTR::null(), tampon, bayt, std::ptr::null_mut()) }
    })
}

/// WinHTTP oturumu: genel User-Agent, sistem vekil ayarı, TLS 1.2+, zaman aşımları.
pub struct Istemci {
    oturum: Tutamac,
}

/// Yönlendirmesiz tek istek ve hedef türü.
enum Yanit {
    Yonlendir(Adres),
    Govde(u64),
}

impl Istemci {
    pub fn new() -> Result<Self, AgHatasi> {
        let ua = genis(&format!("Echora-Guncelleyici/{SURUM}"));
        // SAFETY: metin NUL'lu ve çağrı boyunca yaşar.
        let p = unsafe { WinHttpOpen(PCWSTR(ua.as_ptr()), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0) };
        let oturum = Tutamac::yeni(p, "WinHTTP oturumu açılamadı")?;
        // Çözümleme 10 sn, bağlanma 15 sn, gönderme 30 sn, alma (her okuma) 60 sn.
        // SAFETY: geçerli oturum tutamacı.
        unsafe { WinHttpSetTimeouts(oturum.0, 10_000, 15_000, 30_000, 60_000) }.map_err(|_| win_hatasi("zaman aşımları"))?;
        // TLS 1.3 desteklemeyen eski Windows'ta yalnız TLS 1.2.
        let tls = WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2 | WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_3;
        if secenek_u32(&oturum, WINHTTP_OPTION_SECURE_PROTOCOLS, tls).is_err() {
            secenek_u32(&oturum, WINHTTP_OPTION_SECURE_PROTOCOLS, WINHTTP_FLAG_SECURE_PROTOCOL_TLS1_2)
                .map_err(|_| win_hatasi("TLS ayarı"))?;
        }
        Ok(Istemci { oturum })
    }

    fn tek_istek(&self, a: &Adres, kabul: &str, sinir: u64, hedef: &mut dyn Write) -> Result<Yanit, AgHatasi> {
        let ana = genis(&a.ana);
        // SAFETY: geçerli oturum; metin çağrı boyunca yaşar.
        let baglanti = Tutamac::yeni(unsafe { WinHttpConnect(self.oturum.0, PCWSTR(ana.as_ptr()), INTERNET_DEFAULT_HTTPS_PORT, 0) }, "bağlantı")?;
        let yol = genis(&a.yol);
        // SAFETY: geçerli bağlantı; metinler çağrı boyunca yaşar; kabul türleri listesi verilmez.
        let istek = Tutamac::yeni(
            unsafe {
                WinHttpOpenRequest(baglanti.0, windows::core::w!("GET"), PCWSTR(yol.as_ptr()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), WINHTTP_FLAG_SECURE)
            },
            "istek",
        )?;
        // Yönlendirmeler ELLE izlenir: izin listesi her hedefe bağlanmadan ÖNCE uygulanır.
        secenek_u32(&istek, WINHTTP_OPTION_DISABLE_FEATURE, WINHTTP_DISABLE_REDIRECTS).map_err(|_| win_hatasi("yönlendirme ayarı"))?;
        let basliklar: Vec<u16> = format!("Accept: {kabul}\r\nX-GitHub-Api-Version: 2022-11-28\r\n").encode_utf16().collect();
        // SAFETY: geçerli istek; başlık dilimi çağrı boyunca yaşar; gövde yok.
        unsafe { WinHttpSendRequest(istek.0, Some(&basliklar), None, 0, 0, 0) }.map_err(|_| win_hatasi("istek gönderilemedi"))?;
        // SAFETY: geçerli istek.
        unsafe { WinHttpReceiveResponse(istek.0, std::ptr::null_mut()) }.map_err(|_| win_hatasi("yanıt alınamadı"))?;

        // Son adres (WinHTTP'nin bildirdiği) yeniden denetlenir.
        let son = metin_oku(|tampon, bayt| {
            // SAFETY: geçerli istek; tampon (varsa) `bayt` uzunluğunda.
            unsafe { WinHttpQueryOption(istek.0, WINHTTP_OPTION_URL, tampon, bayt) }
        })
        .ok_or_else(|| AgHatasi::Izinsiz("son adres okunamadı".into()))?;
        let son_adres = izinli_adres(&son).map_err(AgHatasi::Izinsiz)?;
        if son_adres.ana != a.ana {
            return Err(AgHatasi::Izinsiz(format!("beklenmeyen ana makine: {}", son_adres.ana)));
        }

        let mut kod: u32 = 0;
        let mut boyut = 4u32;
        // SAFETY: `kod` 4 bayt; FLAG_NUMBER ile sayı olarak yazılır.
        unsafe {
            WinHttpQueryHeaders(istek.0, WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER, PCWSTR::null(),
                Some((&mut kod as *mut u32).cast()), &mut boyut, std::ptr::null_mut())
        }
        .map_err(|_| win_hatasi("durum kodu"))?;
        if matches!(kod, 301 | 302 | 303 | 307 | 308) {
            let konum = baslik(&istek, WINHTTP_QUERY_LOCATION).ok_or_else(|| AgHatasi::Izinsiz("yönlendirmede Location yok".into()))?;
            return yonlendirme_hedefi(a, &konum).map(Yanit::Yonlendir).map_err(AgHatasi::Izinsiz);
        }
        if kod != 200 {
            return Err(AgHatasi::Durum(kod));
        }
        if let Some(n) = baslik(&istek, WINHTTP_QUERY_CONTENT_LENGTH).and_then(|m| m.trim().parse::<u64>().ok()) {
            if n > sinir {
                return Err(AgHatasi::CokBuyuk);
            }
        }
        let mut toplam: u64 = 0;
        let mut tampon = vec![0u8; 64 * 1024];
        loop {
            let mut var: u32 = 0;
            // SAFETY: geçerli istek.
            unsafe { WinHttpQueryDataAvailable(istek.0, &mut var) }.map_err(|_| win_hatasi("veri okunamadı"))?;
            if var == 0 {
                break;
            }
            let okunacak = (var as usize).min(tampon.len()) as u32;
            let mut okunan: u32 = 0;
            // SAFETY: tampon en az `okunacak` bayt.
            unsafe { WinHttpReadData(istek.0, tampon.as_mut_ptr().cast(), okunacak, &mut okunan) }.map_err(|_| win_hatasi("veri okunamadı"))?;
            if okunan == 0 {
                break;
            }
            toplam += u64::from(okunan);
            if toplam > sinir {
                return Err(AgHatasi::CokBuyuk);
            }
            hedef.write_all(&tampon[..okunan as usize]).map_err(|h| AgHatasi::Ag(format!("yazılamadı: {h}")))?;
        }
        Ok(Yanit::Govde(toplam))
    }

    /// `url`'yi indirir (yönlendirmeleri izin listesiyle izleyerek); en çok `sinir` bayt. Dönen: bayt.
    pub fn getir(&self, url: &str, kabul: &str, sinir: u64, hedef: &mut dyn Write) -> Result<u64, AgHatasi> {
        let mut a = izinli_adres(url).map_err(AgHatasi::Izinsiz)?;
        for _ in 0..=EN_COK_YONLENDIRME {
            match self.tek_istek(&a, kabul, sinir, hedef)? {
                Yanit::Govde(n) => return Ok(n),
                Yanit::Yonlendir(y) => a = y,
            }
        }
        Err(AgHatasi::Izinsiz("çok fazla yönlendirme".into()))
    }
}

/// Yazarken SHA-256 hesaplar.
struct OzetliYazici<W: Write> {
    ic: W,
    ozet: Sha256,
}

impl<W: Write> Write for OzetliYazici<W> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        let n = self.ic.write(b)?;
        self.ozet.update(&b[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.ic.flush()
    }
}

// --- Denetim akışı ---

/// Bu süreçte doğrulanmış, kuruluma hazır sürüm: beklenen SHA-256 özeti tepsi sürecinin BELLEĞİNDE
/// tutulur (diskteki `durum.json`'a güvenilmez).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HazirSurum {
    pub surum: Surum,
    pub ozet: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DenetimSonucu {
    Guncel,
    /// Kuruluma hazır; `bildir`: "indirildi" balonu bu sürüm için henüz gösterilmedi.
    Hazir { hazir: HazirSurum, bildir: bool },
    /// Denetim/indirme başarısız. Önceden hazır olan sürüm (varsa) hazır kalır.
    Hata(String),
}

fn api_adresi() -> String {
    format!("https://api.github.com/repos/{DEPO}/releases/latest")
}

/// `releases/latest` yanıtını getirir.
fn son_surum_json(istemci: &Istemci) -> Result<String, String> {
    let mut v = Vec::new();
    istemci.getir(&api_adresi(), "application/vnd.github+json", EN_COK_API, &mut v).map_err(|h| h.to_string())?;
    String::from_utf8(v).map_err(|_| "sürüm bilgisi UTF-8 değil".into())
}

/// Dosyanın SHA-256 özeti.
pub fn dosya_ozeti(yol: &Path) -> Result<[u8; 32], String> {
    let mut y = OzetliYazici { ic: std::io::sink(), ozet: Sha256::new() };
    let mut f = std::fs::File::open(yol).map_err(|h| format!("{} açılamadı: {h}", yol.display()))?;
    std::io::copy(&mut f, &mut y).map_err(|h| format!("{} okunamadı: {h}", yol.display()))?;
    Ok(y.ozet.finalize().into())
}

/// GitHub'daki SHA256SUMS ile özeti alır; zip yerelde aynı özetle varsa yeniden indirilmez, yoksa
/// indirilip doğrulanır. Zip açılmaz, yalnız içeriği denetlenir. Dönen: beklenen özet.
fn indir_ve_dogrula(klasor: &Path, istemci: &Istemci, y: &Yayin) -> Result<[u8; 32], String> {
    let sk = surum_klasoru(klasor, y.surum);
    std::fs::create_dir_all(&sk).map_err(|h| format!("{} oluşturulamadı: {h}", sk.display()))?;
    let mut ozetler = Vec::new();
    istemci.getir(&y.ozet_url, "application/octet-stream", EN_COK_OZET, &mut ozetler).map_err(|h| format!("{OZET_DOSYASI}: {h}"))?;
    let ad = varlik_adi(y.surum);
    let beklenen = ozet_bul(&String::from_utf8_lossy(&ozetler), &ad).ok_or_else(|| format!("{OZET_DOSYASI} içinde {ad} için geçerli özet yok"))?;
    let zip = zip_yolu(klasor, y.surum);
    if dosya_ozeti(&zip).ok() != Some(beklenen) {
        let parca = sk.join(format!("{ad}.part"));
        let dosya = std::fs::File::create(&parca).map_err(|h| format!("{} yazılamadı: {h}", parca.display()))?;
        let mut yazici = OzetliYazici { ic: std::io::BufWriter::new(dosya), ozet: Sha256::new() };
        gunluk::yaz(&format!("güncelleme: Echora {} indiriliyor ({} bayt)", y.surum, y.zip_boyut));
        let indirme = istemci.getir(&y.zip_url, "application/octet-stream", EN_COK_INDIRME, &mut yazici);
        let yazma = yazici.flush();
        let gercek: [u8; 32] = yazici.ozet.finalize().into();
        drop(yazici.ic);
        if let Err(h) = indirme.map_err(|h| h.to_string()).and_then(|_| yazma.map_err(|h| h.to_string())) {
            let _ = std::fs::remove_file(&parca);
            return Err(format!("{ad} indirilemedi: {h}"));
        }
        if gercek != beklenen {
            let _ = std::fs::remove_file(&parca);
            return Err(format!("SHA-256 uyuşmuyor ({ad}): beklenen {}, inen {}", onaltilik(&beklenen), onaltilik(&gercek)));
        }
        let _ = std::fs::remove_file(&zip);
        std::fs::rename(&parca, &zip).map_err(|h| format!("{} yerine konamadı: {h}", zip.display()))?;
    }
    zip_denetle(&zip, y.surum)?;
    Ok(beklenen)
}

/// Tam denetim (ayrı iş parçacığında çağrılır; bloklar). `mevcut`: bu süreçte doğrulanmış hazır
/// sürüm; yeni sürümün indirilmesi başarısız olursa o hazır kalır.
pub fn denetle(klasor: &Path, mevcut: Option<HazirSurum>) -> DenetimSonucu {
    let simdiki = Surum::simdiki();
    surum_klasorlerini_temizle(klasor, simdiki, None);
    let onceki = GuncellemeDurumu::oku(&durum::durum_yolu(klasor)).ok().flatten().unwrap_or_default();
    let simdi = durum::simdi_unix();
    let sonuc = (|| -> Result<Option<HazirSurum>, (Option<Surum>, String)> {
        let istemci = Istemci::new().map_err(|h| (None, h.to_string()))?;
        let json = son_surum_json(&istemci).map_err(|h| (None, h))?;
        let y = match yayin_karari(&json, simdiki).map_err(|h| (None, h))? {
            YayinKarari::Guncel => return Ok(None),
            YayinKarari::Yeni(y) => y,
        };
        // Hazır sürüm varken durum "indiriliyor"a çekilmez (Stüdyo hazır olanı göstermeye devam eder).
        if mevcut.is_none() {
            durum_yaz(klasor, &GuncellemeDurumu {
                son_denetim: Some(simdi),
                durum: DurumTuru::Indiriliyor,
                surum: Some(y.surum.to_string()),
                hata: None,
                bildirildi: false,
            });
        }
        let ozet = indir_ve_dogrula(klasor, &istemci, &y).map_err(|h| (Some(y.surum), h))?;
        Ok(Some(HazirSurum { surum: y.surum, ozet }))
    })();
    match sonuc {
        Ok(None) => {
            durum_yaz(klasor, &GuncellemeDurumu { son_denetim: Some(simdi), durum: DurumTuru::Guncel, ..Default::default() });
            DenetimSonucu::Guncel
        }
        Ok(Some(h)) => {
            // Yeni hazır olunca diğer (eski hazır) sürüm klasörleri silinir.
            surum_klasorlerini_temizle(klasor, simdiki, Some(h.surum));
            let ayni = mevcut.map(|m| m.surum) == Some(h.surum)
                || (onceki.surum.as_deref() == Some(h.surum.to_string().as_str()) && onceki.durum == DurumTuru::Hazir && onceki.bildirildi);
            durum_yaz(klasor, &GuncellemeDurumu {
                son_denetim: Some(simdi),
                durum: DurumTuru::Hazir,
                surum: Some(h.surum.to_string()),
                hata: None,
                bildirildi: true,
            });
            DenetimSonucu::Hazir { hazir: h, bildir: !ayni }
        }
        Err((s, h)) => {
            let d = match mevcut.filter(|m| zip_yolu(klasor, m.surum).is_file()) {
                // Hazır olan düşmez; hata yalnız not edilir.
                Some(m) => GuncellemeDurumu {
                    son_denetim: Some(simdi),
                    durum: DurumTuru::Hazir,
                    surum: Some(m.surum.to_string()),
                    hata: Some(format!("daha yeni sürüm alınamadı: {h}")),
                    bildirildi: true,
                },
                None => GuncellemeDurumu {
                    son_denetim: Some(simdi),
                    durum: DurumTuru::Hata,
                    surum: s.map(|s| s.to_string()),
                    hata: Some(h.clone()),
                    bildirildi: false,
                },
            };
            durum_yaz(klasor, &d);
            DenetimSonucu::Hata(h)
        }
    }
}

// --- Kurulum (onaydan sonra) ---

/// Onaydan HEMEN sonra: zip'in özeti bellekteki beklenen özetle yeniden karşılaştırılır (indirme
/// ile kurulum arasında değiştirilmesin) ve zip YENİ, benzersiz bir klasöre açılır. Dönen: kurulum
/// kaynağı (içinde echora.exe, DLL'ler, model).
pub fn kurulum_kaynagini_hazirla(klasor: &Path, h: HazirSurum) -> Result<PathBuf, String> {
    let zip = zip_yolu(klasor, h.surum);
    let gercek = dosya_ozeti(&zip)?;
    if gercek != h.ozet {
        return Err(format!("indirilen dosya doğrulandıktan sonra değişmiş (SHA-256 {}); kurulmadı", onaltilik(&gercek)));
    }
    let zaman = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let hedef = surum_klasoru(klasor, h.surum).join(format!("kurulum-{}-{zaman}", std::process::id()));
    zip_ac(&zip, &hedef)?;
    let kaynak = hedef.join(format!("Echora-{}-win64", h.surum));
    kurulum::kaynak_dogrula(&kaynak)?;
    Ok(kaynak)
}

/// Onaylanmış kurulumu başlatır: hazır sürümün exe'si `--guncelle-uygula` ile başlatılır.
/// Çağıran ardından temiz çıkmalıdır.
pub fn guncelleyiciyi_baslat(kaynak: &Path, hedef: &Path) -> Result<(), String> {
    let exe = kaynak.join(kurulum::EXE_ADI);
    std::process::Command::new(&exe)
        .arg("--guncelle-uygula")
        .arg("--hedef")
        .arg(hedef)
        .arg("--bekle")
        .arg(std::process::id().to_string())
        .current_dir(kaynak)
        .env_remove("ECHORA_KURU")
        .spawn()
        .map(drop)
        .map_err(|h| format!("güncelleyici başlatılamadı: {h}"))
}

/// `pid` bitene dek en çok `sure` bekler. Süreç zaten yoksa `true`.
pub fn bitmesini_bekle(pid: u32, sure: Duration) -> bool {
    // SAFETY: yalnız bekleme izniyle açılır, aşağıda kapatılır.
    let Ok(h) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }) else { return true };
    // SAFETY: geçerli süreç tutamacı.
    let r = unsafe { WaitForSingleObject(h, sure.as_millis() as u32) };
    // SAFETY: OpenProcess ile açılan tutamaç.
    unsafe {
        let _ = CloseHandle(h);
    }
    r == WAIT_OBJECT_0
}

fn echora_baslat(klasor: &Path) {
    let exe = klasor.join(kurulum::EXE_ADI);
    if let Err(h) = std::process::Command::new(&exe).current_dir(klasor).env_remove("ECHORA_KURU").spawn() {
        gunluk::yaz(&format!("HATA: güncelleyici: {} başlatılamadı: {h}", exe.display()));
    }
}

/// `echora.exe --guncelle-uygula --hedef <klasör> --bekle <pid>`: bu exe'nin klasöründeki yeni
/// sürümü kurulum klasörüne kopyalar. Çıkış kodu: 0 başarı.
///
/// Reddedilenler (hiçbir şey yazılmaz): hedef kurulum klasörü değil; bu exe
/// `<guncelleme>\<kendi sürümü>\` altında değil; kurulu exe'nin sürümü okunamıyor ya da bu
/// sürümden yeni değil. Her başarısızlıkta kurulu Echora yeniden başlatılır (kullanıcı uygulamasız
/// kalmasın) ve hata `durum.json`'a yazılır (bir sonraki açılışta balonla bildirilir).
pub fn uygula(hedef: &Path, bekle: u32, klasor: &Path) -> i32 {
    let yeni = Surum::simdiki();
    let hata_yaz = |h: &str| {
        gunluk::yaz(&format!("HATA: güncelleme kurulamadı: {h}"));
        durum_yaz(klasor, &GuncellemeDurumu {
            son_denetim: Some(durum::simdi_unix()),
            durum: DurumTuru::Hata,
            surum: Some(yeni.to_string()),
            hata: Some(format!("{KURULUM_HATASI}: {h}")),
            bildirildi: false,
        });
    };
    let Some(kurulu) = kurulum::kurulum_klasoru() else {
        hata_yaz("%LOCALAPPDATA% bulunamadı");
        return 2;
    };
    // Yalnız kurulum klasörü güncellenir; başka bir hedef verilmişse hiçbir şey yazılmaz/başlatılmaz.
    if !ayni_yol(hedef, &kurulu) {
        hata_yaz(&format!("hedef kurulum klasörü değil: {}", hedef.display()));
        return 2;
    }
    let reddet = |h: &str| -> i32 {
        hata_yaz(h);
        echora_baslat(&kurulu);
        2
    };
    let Some(kaynak) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) else {
        return reddet("güncelleyici exe yolu alınamadı");
    };
    if !guncelleyici_kaynagi_gecerli(&kaynak, klasor, yeni) {
        return reddet(&format!("güncelleyici beklenen klasörden çalışmıyor: {}", kaynak.display()));
    }
    gunluk::yaz(&format!("güncelleyici: Echora {yeni} kuruluyor; eski süreç (#{bekle}) bekleniyor"));
    if !bitmesini_bekle(bekle, BEKLEME) {
        // Eski süreç hâlâ açık: başlatılan kurulu exe tek örnek kilidi yüzünden hemen çıkar,
        // eski süreç çalışmaya devam eder; hata bir sonraki açılışta bildirilir.
        return reddet("eski Echora 15 sn içinde kapanmadı");
    }
    match exe_surumu(&kurulu.join(kurulum::EXE_ADI)) {
        Some(k) if k < yeni => {}
        Some(k) => return reddet(&format!("kurulu sürüm ({k}) bu sürümden ({yeni}) yeni değil")),
        None => return reddet("kurulu echora.exe'nin sürümü okunamadı"),
    }
    match kurulum::dosyalari_kur(&kaynak, hedef) {
        Ok(o) => {
            gunluk::yaz(&format!(
                "güncelleyici: Echora {yeni} kuruldu ({} dosya{})",
                o.dosyalar.len(),
                if o.model_kopyalandi { ", model" } else { "" }
            ));
            durum_yaz(klasor, &GuncellemeDurumu {
                son_denetim: Some(durum::simdi_unix()),
                durum: DurumTuru::Guncel,
                ..Default::default()
            });
            echora_baslat(hedef);
            0
        }
        Err(h) => {
            // `dosyalari_kur` hata durumunda eski dosyaları yerine koydu: eski sürüm yeniden açılır.
            hata_yaz(&h);
            echora_baslat(hedef);
            1
        }
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    fn s(a: u64, b: u64, c: u64) -> Surum {
        Surum { ana: a, ara: b, yama: c }
    }

    #[test]
    fn surum_ayristirma_ve_siralama() {
        assert_eq!(Surum::coz("1.2.3"), Some(s(1, 2, 3)));
        assert_eq!(Surum::coz("v0.10.0"), Some(s(0, 10, 0)));
        assert_eq!(Surum::coz(" V2.0.1 "), Some(s(2, 0, 1)));
        for kotu in ["1.2", "1.2.3.4", "1.2.3-beta", "1.2.3-rc.1", "1.2.3+git", "v1.02.3", "1.a.3", "", "v", "1..3", "-1.2.3"] {
            assert_eq!(Surum::coz(kotu), None, "{kotu}");
        }
        assert!(s(0, 10, 0) > s(0, 9, 9));
        assert!(s(1, 0, 0) > s(0, 99, 99));
        assert!(s(0, 1, 1) > s(0, 1, 0));
        assert_eq!(s(1, 2, 3).to_string(), "1.2.3");
        assert_eq!(Surum::simdiki().to_string(), SURUM);
    }

    #[test]
    fn varlik_adi_paketleme_ile_ayni() {
        assert_eq!(varlik_adi(s(0, 2, 0)), "Echora-0.2.0-win64.zip");
    }

    #[test]
    fn adres_ayristirma() {
        let a = adres_coz("https://API.GitHub.com/repos/x/y/releases/latest").unwrap();
        assert_eq!(a, Adres { ana: "api.github.com".into(), yol: "/repos/x/y/releases/latest".into() });
        assert_eq!(adres_coz("https://github.com:443/a?b=1#parca").unwrap().yol, "/a?b=1");
        assert_eq!(adres_coz("https://github.com").unwrap().yol, "/");
        assert_eq!(adres_coz("https://github.com?x").unwrap().yol, "/?x");
        for kotu in [
            "http://github.com/a",
            "ftp://github.com",
            "https://user@github.com/a",
            "https://github.com:8443/a",
            "https://",
            "https://git hub.com/",
            "https://github.com/a b",
            "https://[::1]/",
            "github.com/a",
        ] {
            assert!(adres_coz(kotu).is_err(), "{kotu}");
        }
    }

    #[test]
    fn ana_makine_izin_listesi() {
        for iyi in [
            "https://api.github.com/repos/a",
            "https://github.com/a/b/releases/download/v1/x.zip",
            "https://objects.githubusercontent.com/x",
            "https://release-assets.githubusercontent.com/x?sig=1",
        ] {
            assert!(izinli_adres(iyi).is_ok(), "{iyi}");
        }
        for kotu in [
            "https://evil.com/",
            "https://github.com.evil.com/",
            "https://evilgithub.com/",
            "https://raw.githubusercontent.com/x",
            "https://api.github.com.:443/",
            "https://githubusercontent.com/x",
        ] {
            assert!(izinli_adres(kotu).is_err(), "{kotu}");
        }
    }

    #[test]
    fn yonlendirme_kurallari() {
        let m = adres_coz("https://github.com/a/b/releases/download/v1/x.zip").unwrap();
        assert_eq!(yonlendirme_hedefi(&m, "/baska/yol").unwrap().metin(), "https://github.com/baska/yol");
        assert!(yonlendirme_hedefi(&m, "https://release-assets.githubusercontent.com/x?a=1").is_ok());
        assert!(yonlendirme_hedefi(&m, "https://evil.example/x").is_err());
        assert!(yonlendirme_hedefi(&m, "http://objects.githubusercontent.com/x").is_err());
        assert!(yonlendirme_hedefi(&m, "//evil.example/x").is_err());
        assert!(yonlendirme_hedefi(&m, "goreli/yol").is_err());
    }

    #[test]
    fn sha256sums_ayristirma() {
        let h1 = "a".repeat(64);
        let h2 = "0123456789abcdef".repeat(4);
        let metin = format!("\u{feff}{h1}  Echora-0.2.0-win64.zip\n{h2} *SHA256SUMS.txt\n\n# yorum\n");
        assert_eq!(ozet_bul(&metin, "Echora-0.2.0-win64.zip"), Some([0xaa; 32]));
        assert_eq!(onaltilik(&ozet_bul(&metin, "SHA256SUMS.txt").unwrap()), h2);
        assert_eq!(ozet_bul(&metin, "Echora-0.2.0-win64.zip.part"), None);
        assert_eq!(ozet_bul(&metin, "yok.zip"), None);
        // Büyük harf onaltılık kabul.
        assert_eq!(ozet_bul(&format!("{}  x.zip", "AB".repeat(32)), "x.zip"), Some([0xab; 32]));
        // Kısa/bozuk özet.
        assert_eq!(ozet_bul("abc  x.zip", "x.zip"), None);
        assert_eq!(ozet_bul(&format!("{}  x.zip", "g".repeat(64)), "x.zip"), None);
        // Aynı ad iki farklı özetle: reddedilir; aynı özetle: kabul.
        assert_eq!(ozet_bul(&format!("{h1}  x.zip\n{h2}  x.zip\n"), "x.zip"), None);
        assert_eq!(ozet_bul(&format!("{h1}  x.zip\n{h1}  x.zip\n"), "x.zip"), Some([0xaa; 32]));
    }

    fn yanit(etiket: &str, varliklar: &[(&str, &str, u64)], on: bool) -> String {
        let v: Vec<serde_json::Value> = varliklar
            .iter()
            .map(|(n, u, b)| serde_json::json!({"name": n, "browser_download_url": u, "size": b, "fazla_alan": 1}))
            .collect();
        serde_json::json!({"tag_name": etiket, "draft": false, "prerelease": on, "assets": v, "body": "notlar"}).to_string()
    }

    #[test]
    fn yayin_karari_kurallari() {
        let k = |u: &str| format!("https://github.com/{DEPO}/releases/download/v0.2.0/{u}");
        let zip = k("Echora-0.2.0-win64.zip");
        let ozet = k("SHA256SUMS.txt");
        let iyi = yanit("v0.2.0", &[("Echora-0.2.0-win64.zip", &zip, 1000), ("SHA256SUMS.txt", &ozet, 100)], false);
        match yayin_karari(&iyi, s(0, 1, 0)).unwrap() {
            YayinKarari::Yeni(y) => {
                assert_eq!(y.surum, s(0, 2, 0));
                assert_eq!(y.zip_url, zip);
                assert_eq!(y.ozet_url, ozet);
            }
            d => panic!("{d:?}"),
        }
        // Aynı ya da eski sürüm.
        assert_eq!(yayin_karari(&iyi, s(0, 2, 0)).unwrap(), YayinKarari::Guncel);
        assert_eq!(yayin_karari(&iyi, s(1, 0, 0)).unwrap(), YayinKarari::Guncel);
        // Önsürüm bayrağı ya da önsürüm etiketi atlanır.
        let on = yanit("v0.2.0", &[("Echora-0.2.0-win64.zip", &zip, 1), ("SHA256SUMS.txt", &ozet, 1)], true);
        assert_eq!(yayin_karari(&on, s(0, 1, 0)).unwrap(), YayinKarari::Guncel);
        let beta = yanit("v0.3.0-beta", &[], false);
        assert_eq!(yayin_karari(&beta, s(0, 1, 0)).unwrap(), YayinKarari::Guncel);
        // Eksik varlık.
        let eksik = yanit("v0.2.0", &[("Echora-0.2.0-win64.zip", &zip, 1)], false);
        assert!(yayin_karari(&eksik, s(0, 1, 0)).is_err());
        // Başka depodan ya da izin listesi dışından adres.
        let baska = yanit("v0.2.0", &[("Echora-0.2.0-win64.zip", "https://github.com/kotu/depo/releases/download/v0.2.0/Echora-0.2.0-win64.zip", 1), ("SHA256SUMS.txt", &ozet, 1)], false);
        assert!(yayin_karari(&baska, s(0, 1, 0)).is_err());
        let http = yanit("v0.2.0", &[("Echora-0.2.0-win64.zip", &zip.replace("https", "http"), 1), ("SHA256SUMS.txt", &ozet, 1)], false);
        assert!(yayin_karari(&http, s(0, 1, 0)).is_err());
        // Boyut sınırı.
        let buyuk = yanit("v0.2.0", &[("Echora-0.2.0-win64.zip", &zip, EN_COK_INDIRME + 1), ("SHA256SUMS.txt", &ozet, 1)], false);
        assert!(yayin_karari(&buyuk, s(0, 1, 0)).is_err());
        // Bozuk JSON.
        assert!(yayin_karari("{", s(0, 1, 0)).is_err());
        assert!(yayin_karari(r#"{"message":"Not Found"}"#, s(0, 1, 0)).is_err());
    }

    #[test]
    fn zip_yol_guvenligi() {
        assert_eq!(zip_girdi_yolu("Echora-0.1.0-win64/echora.exe").unwrap(), (PathBuf::from("Echora-0.1.0-win64").join("echora.exe"), false));
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
        // "COM10" ve "console" ayrılmış değil.
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
    fn zip_yaz(yol: &Path, girdiler: &[(&str, &[u8])]) {
        use std::io::Write as _;
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
            // ek alan, açıklama, disk, iç öznitelik (2'şer), dış öznitelik (4).
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
        std::fs::File::create(yol).unwrap().write_all(&b).unwrap();
    }

    /// Sahte sürüm zip'i (kurulum için gereken dosya adlarıyla; içerik sahte).
    fn sahte_surum_zip(yol: &Path, s: Surum) {
        let k = format!("Echora-{s}-win64");
        let mut g: Vec<(String, &[u8])> = vec![(format!("{k}/echora.exe"), b"exe")];
        for d in kurulum::DLLLER {
            g.push((format!("{k}/{d}"), b"dll"));
        }
        g.push((format!("{k}/modeller/vosk-model-small-tr-0.3/final.mdl"), b"model"));
        let g: Vec<(&str, &[u8])> = g.iter().map(|(a, v)| (a.as_str(), *v)).collect();
        zip_yaz(yol, &g);
    }

    #[test]
    fn zip_acma_ve_kotu_girdi() {
        let t = crate::yollar::test_yardimci::GeciciKlasor::yeni();
        let iyi = t.0.join("iyi.zip");
        zip_yaz(&iyi, &[("Echora-0.2.0-win64/README.md", b"merhaba")]);
        let hedef = t.0.join("acik");
        assert_eq!(zip_ac(&iyi, &hedef).unwrap(), 7);
        assert_eq!(std::fs::read_to_string(hedef.join("Echora-0.2.0-win64").join("README.md")).unwrap(), "merhaba");

        let kotu = t.0.join("kotu.zip");
        zip_yaz(&kotu, &[("../disari.txt", b"x")]);
        let hedef2 = t.0.join("ic").join("acik");
        assert!(zip_ac(&kotu, &hedef2).unwrap_err().contains("yol geçişi"));
        assert!(!t.0.join("ic").join("disari.txt").exists());
        assert!(!t.0.join("disari.txt").exists());

        let ters = t.0.join("ters.zip");
        zip_yaz(&ters, &[("a\\..\\..\\disari2.txt", b"x")]);
        assert!(zip_ac(&ters, &hedef2).is_err());
        assert!(!t.0.join("disari2.txt").exists());
    }

    #[test]
    fn zip_yinelenen_girdi_reddedilir() {
        let t = crate::yollar::test_yardimci::GeciciKlasor::yeni();
        let z = t.0.join("yineli.zip");
        // Büyük/küçük harf ve ayırıcı farkı aynı dosyadır.
        zip_yaz(&z, &[("E/libvosk.dll", b"iyi"), ("e\\LIBVOSK.DLL", b"kotu")]);
        assert!(zip_ac(&z, &t.0.join("a")).unwrap_err().contains("yinelenen"));
        assert!(zip_denetle(&z, Surum::simdiki()).is_err());
        let z2 = t.0.join("tek.zip");
        zip_yaz(&z2, &[("E/a.dll", b"1"), ("E/b.dll", b"2")]);
        assert!(zip_ac(&z2, &t.0.join("b")).is_ok());
    }

    #[test]
    fn zip_denetimi_gerekli_dosyalari_ister() {
        let t = crate::yollar::test_yardimci::GeciciKlasor::yeni();
        let s = Surum { ana: 9, ara: 1, yama: 0 };
        let z = t.0.join("s.zip");
        sahte_surum_zip(&z, s);
        zip_denetle(&z, s).unwrap();
        // Başka sürümün klasör adı: eksik sayılır.
        assert!(zip_denetle(&z, Surum { ana: 9, ara: 2, yama: 0 }).is_err());
        let eksik = t.0.join("e.zip");
        zip_yaz(&eksik, &[("Echora-9.1.0-win64/echora.exe", b"x")]);
        assert!(zip_denetle(&eksik, s).unwrap_err().contains("libvosk.dll"));
    }

    #[test]
    fn onaydan_once_ozet_yeniden_denetlenir_ve_yeni_klasore_acilir() {
        let t = crate::yollar::test_yardimci::GeciciKlasor::yeni();
        let k = &t.0;
        let s = Surum { ana: 9, ara: 1, yama: 0 };
        let zip = zip_yolu(k, s);
        std::fs::create_dir_all(zip.parent().unwrap()).unwrap();
        sahte_surum_zip(&zip, s);
        let h = HazirSurum { surum: s, ozet: dosya_ozeti(&zip).unwrap() };
        let k1 = kurulum_kaynagini_hazirla(k, h).unwrap();
        assert!(k1.join("echora.exe").is_file());
        assert!(guncelleyici_kaynagi_gecerli(&k1, k, s));
        // Her kurulum yeni bir klasöre açılır; zip silinmez.
        std::thread::sleep(Duration::from_millis(2));
        let k2 = kurulum_kaynagini_hazirla(k, h).unwrap();
        assert_ne!(k1, k2);
        assert!(zip.is_file());
        // Doğrulamadan sonra değiştirilen zip kurulmaz.
        zip_yaz(&zip, &[("Echora-9.1.0-win64/echora.exe", b"degisti")]);
        assert!(kurulum_kaynagini_hazirla(k, h).unwrap_err().contains("değişmiş"));
    }

    #[test]
    fn guncelleyici_kaynak_kurali() {
        let k = Path::new(r"C:\Users\x\AppData\Local\Echora\guncelleme");
        let s = Surum { ana: 0, ara: 2, yama: 0 };
        assert!(guncelleyici_kaynagi_gecerli(&k.join(r"0.2.0\kurulum-1-2\Echora-0.2.0-win64"), k, s));
        assert!(guncelleyici_kaynagi_gecerli(Path::new(r"c:\users\X\appdata\local\echora\GUNCELLEME\0.2.0\k"), k, s));
        // Başka sürümün klasörü, klasörün kendisi, benzer önek ya da başka yer reddedilir.
        assert!(!guncelleyici_kaynagi_gecerli(&k.join(r"0.3.0\k"), k, s));
        assert!(!guncelleyici_kaynagi_gecerli(&k.join("0.2.0"), k, s));
        assert!(!guncelleyici_kaynagi_gecerli(&k.join(r"0.2.0x\k"), k, s));
        assert!(!guncelleyici_kaynagi_gecerli(Path::new(r"C:\Users\x\Downloads\Echora-0.2.0-win64"), k, s));
    }

    #[test]
    fn exe_surumu_okunur() {
        // Test ikilisi de `echora` ikilisinin kaynağıyla bağlanır: kendi sürümünü okur.
        assert_eq!(exe_surumu(&std::env::current_exe().unwrap()), Some(Surum::simdiki()));
        assert_eq!(exe_surumu(Path::new(r"C:\olmayan\echora.exe")), None);
        // Derlenmiş uygulama ikilisi varsa kendi sürümünü taşır.
        let exe = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(r"..\..\target\release\echora.exe");
        if exe.is_file() {
            assert_eq!(exe_surumu(&exe), Some(Surum::simdiki()));
        }
    }

    #[test]
    fn otomatik_denetim_zamani() {
        let gun = 24 * 3600;
        assert!(denetim_zamani_mi(None, 1000));
        assert!(!denetim_zamani_mi(Some(1000), 1000 + gun - 1));
        assert!(denetim_zamani_mi(Some(1000), 1000 + gun));
        // Uyku sonrası uzun aralık.
        assert!(denetim_zamani_mi(Some(1000), 1000 + 3 * gun));
        // Saat geri alınmış.
        assert!(denetim_zamani_mi(Some(10_000), 1000));
        assert!(!denetim_zamani_mi(Some(1500), 1000));
    }

    #[test]
    fn kurulum_karar_tablosu() {
        let k = PathBuf::from(r"C:\Users\x\AppData\Local\Programs\Echora");
        assert_eq!(kurulum_karari(&k.join("echora.exe"), Some(&k)), KurulumKarari::Kurulu(k.clone()));
        assert_eq!(kurulum_karari(Path::new(r"C:\kod\target\release\echora.exe"), Some(&k)), KurulumKarari::Gelistirme);
        assert_eq!(kurulum_karari(Path::new(r"C:\Users\x\Downloads\Echora-0.1.0-win64\echora.exe"), Some(&k)), KurulumKarari::KuruluDegil);
    }

    #[test]
    fn surum_klasoru_temizligi() {
        let t = crate::yollar::test_yardimci::GeciciKlasor::yeni();
        let k = &t.0;
        let simdiki = Surum::simdiki();
        let y1 = Surum { ana: simdiki.ana + 1, ara: 0, yama: 0 };
        let y2 = Surum { ana: simdiki.ana + 2, ara: 0, yama: 0 };
        for s in [simdiki, y1, y2] {
            std::fs::create_dir_all(surum_klasoru(k, s)).unwrap();
        }
        std::fs::create_dir_all(k.join("baska")).unwrap();
        // Denetim başında: yalnız kurulu sürüme eşit/eski olanlar.
        surum_klasorlerini_temizle(k, simdiki, None);
        assert!(!surum_klasoru(k, simdiki).exists());
        assert!(surum_klasoru(k, y1).exists() && surum_klasoru(k, y2).exists());
        // Yeni sürüm hazır olunca eski hazır sürüm silinir.
        surum_klasorlerini_temizle(k, simdiki, Some(y2));
        assert!(!surum_klasoru(k, y1).exists());
        assert!(surum_klasoru(k, y2).exists());
        assert!(k.join("baska").exists());
    }

    #[test]
    fn baslangicta_durum_toparlanir() {
        let t = crate::yollar::test_yardimci::GeciciKlasor::yeni();
        let k = &t.0;
        let yol = durum::durum_yolu(k);
        assert_eq!(baslangic_durumu(k), None);
        // Yarım kalmış indirme ve bu süreçte doğrulanmamış "hazır": güncel'e döner.
        for d in [DurumTuru::Indiriliyor, DurumTuru::Hazir] {
            GuncellemeDurumu { son_denetim: Some(7), durum: d, surum: Some("9.0.0".into()), hata: None, bildirildi: true }.yaz(&yol).unwrap();
            assert_eq!(baslangic_durumu(k), None);
            let g = GuncellemeDurumu::oku(&yol).unwrap().unwrap();
            assert_eq!((g.durum, g.surum, g.son_denetim), (DurumTuru::Guncel, None, Some(7)));
        }
        // Kurulum hatası bir kez bildirilir.
        let h = format!("{KURULUM_HATASI}: eski Echora 15 sn içinde kapanmadı");
        GuncellemeDurumu { son_denetim: Some(7), durum: DurumTuru::Hata, surum: Some("9.0.0".into()), hata: Some(h.clone()), bildirildi: false }.yaz(&yol).unwrap();
        assert_eq!(baslangic_durumu(k), Some(h));
        assert_eq!(baslangic_durumu(k), None);
        // Ağ hatası açılışta bildirilmez.
        GuncellemeDurumu { durum: DurumTuru::Hata, hata: Some("ağ hatası".into()), ..Default::default() }.yaz(&yol).unwrap();
        assert_eq!(baslangic_durumu(k), None);
        assert_eq!(son_denetim(k), None);
    }

    /// Paketleme ↔ güncelleyici uyumu: `araclar/paketle.ps1`'in ürettiği yerel zip ve
    /// SHA256SUMS.txt güncelleyicinin kendi işlevleriyle doğrulanır ve açılır (ağ yok).
    /// `cargo test -- --ignored yerel_paket` (önce paketle.ps1 çalıştırılmalı).
    #[test]
    #[ignore = "dist/ paketi gerekir"]
    fn yerel_paket_dogrulanir_ve_acilir() {
        let dist = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("dist");
        let s = Surum::simdiki();
        let ad = varlik_adi(s);
        let zip = dist.join(&ad);
        let ozetler = std::fs::read_to_string(dist.join(OZET_DOSYASI)).expect("SHA256SUMS.txt");
        let beklenen = ozet_bul(&ozetler, &ad).expect("özet satırı");
        let mut y = OzetliYazici { ic: std::io::sink(), ozet: Sha256::new() };
        std::io::copy(&mut std::fs::File::open(&zip).unwrap(), &mut y).unwrap();
        let gercek: [u8; 32] = y.ozet.finalize().into();
        assert_eq!(gercek, beklenen, "SHA-256");
        let t = crate::yollar::test_yardimci::GeciciKlasor::yeni();
        let hedef = t.0.join("hazir");
        let n = zip_ac(&zip, &hedef).unwrap();
        let kaynak = hedef.join(format!("Echora-{s}-win64"));
        kurulum::kaynak_dogrula(&kaynak).unwrap();
        eprintln!("açıldı: {n} bayt, kaynak doğrulandı: {}", kaynak.display());
    }

    /// Gerçek GitHub'a TEK salt-okuma isteği: depo henüz yok → 404 hatası düzgün işlenmeli.
    /// Ağ yoksa (CI, çevrimdışı) ağ hatası da kabul. `cargo test -- --ignored ag_` ile çalışır.
    #[test]
    #[ignore = "ağa çıkar"]
    fn ag_olmayan_depo_404() {
        let i = Istemci::new().unwrap();
        let mut v = Vec::new();
        match i.getir(&api_adresi(), "application/vnd.github+json", EN_COK_API, &mut v) {
            Err(AgHatasi::Durum(404)) => {}
            Err(AgHatasi::Ag(m)) => eprintln!("ağ yok: {m}"),
            d => panic!("beklenmeyen: {d:?}"),
        }
    }
}
