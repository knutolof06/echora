//! Kurulum: dosyaları `%LOCALAPPDATA%\Programs\Echora`'ya kopyalama (yedekli, geri alınabilir),
//! Başlat menüsü kısayolları, eski "Yankı" kurulumunun temizliği ve mikrofon izninin OKUNMASI.
//!
//! Hem kurulum sihirbazı (Stüdyo) hem güncelleyici (`echora.exe --guncelle-uygula`) aynı
//! `dosyalari_kur`'u kullanır. Saf kararlar (`konum`, `izin_karari`, `kopyalanacaklar`) birim testlidir.
//!
//! Gizlilik ayarı (mikrofon erişimi) burada YALNIZ OKUNUR: Windows'un kullanıcı onay kapısıdır,
//! programla değiştirilmez; kapalıysa kullanıcı ayar sayfasına yönlendirilir.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};
use windows::core::{w, Interface, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Programs, IShellLinkW, SHGetKnownFolderPath, ShellExecuteW, ShellLink, KF_FLAG_DEFAULT,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use crate::ipc::ayni_yol;

pub const EXE_ADI: &str = "echora.exe";
/// Vosk ve MinGW çalışma zamanı kütüphaneleri (exe'nin yanında olmalı).
pub const DLLLER: [&str; 4] = ["libvosk.dll", "libstdc++-6.dll", "libgcc_s_seh-1.dll", "libwinpthread-1.dll"];
/// Varsa kopyalanan belgeler.
pub const BELGELER: [&str; 2] = ["README.md", "UCUNCU-TARAF.txt"];
/// Exe'ye göre model klasörü.
pub const MODEL_GORELI: &str = r"modeller\vosk-model-small-tr-0.3";
/// `%LOCALAPPDATA%` altındaki kurulum klasörü.
pub const KURULUM_GORELI: &str = r"Programs\Echora";
/// Eski adla kurulum klasörü ve exe'si.
pub const ESKI_KURULUM_GORELI: &str = r"Programs\Yanki";
pub const ESKI_EXE_ADI: &str = "yanki.exe";
/// Başlat menüsü klasör adları.
pub const MENU_KLASORU: &str = "Echora";
pub const ESKI_MENU_KLASORU: &str = "Yankı";

/// Örnek/duman sınaması kipinde kurulum hiçbir koşulda dosya yazmaz (iç bayrak).
static YAZMA_KAPALI: AtomicBool = AtomicBool::new(false);

pub(crate) fn yazmayi_kapat() {
    YAZMA_KAPALI.store(true, Ordering::SeqCst);
}

pub fn yazma_kapali_mi() -> bool {
    YAZMA_KAPALI.load(Ordering::SeqCst)
}

fn yerel() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).filter(|p| p.is_absolute())
}

/// `%LOCALAPPDATA%\Programs\Echora`.
pub fn kurulum_klasoru() -> Option<PathBuf> {
    yerel().map(|l| l.join(KURULUM_GORELI))
}

/// `%LOCALAPPDATA%\Programs\Yanki`.
pub fn eski_kurulum_klasoru() -> Option<PathBuf> {
    yerel().map(|l| l.join(ESKI_KURULUM_GORELI))
}

/// Çalışan exe'nin konumu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Konum {
    /// Kurulum klasöründen çalışıyor.
    Kurulu,
    /// `cargo` derleme klasöründen (`\target\`) çalışıyor: kurulmaz, güncellenmez.
    Gelistirme,
    /// Başka bir yerden (ör. açılmış sürüm zip'i): kurulum sihirbazı açılır.
    Kurulmamis,
}

/// Saf karar. `kurulum`: `kurulum_klasoru()`.
pub fn konum(exe: &Path, kurulum: Option<&Path>) -> Konum {
    if let (Some(k), Some(ust)) = (kurulum, exe.parent()) {
        if ayni_yol(ust, k) {
            return Konum::Kurulu;
        }
    }
    let e = exe.to_string_lossy().replace('/', "\\").to_lowercase();
    if e.contains(r"\target\") {
        return Konum::Gelistirme;
    }
    Konum::Kurulmamis
}

/// Kaynak klasörde kurulum için gereken dosyalar var mı?
pub fn kaynak_dogrula(kaynak: &Path) -> Result<(), String> {
    if !kaynak.join(EXE_ADI).is_file() {
        return Err(format!("{} içinde {EXE_ADI} yok", kaynak.display()));
    }
    for d in DLLLER {
        if !kaynak.join(d).is_file() {
            return Err(format!("{} içinde {d} yok", kaynak.display()));
        }
    }
    if !kaynak.join(MODEL_GORELI).join("final.mdl").is_file() {
        return Err(format!("{} içinde tanıma modeli ({MODEL_GORELI}) yok", kaynak.display()));
    }
    Ok(())
}

/// Kopyalanacak tek tek dosyalar (göreli): exe, DLL'ler ve kaynakta bulunan belgeler. Model ayrı
/// (klasör olarak, yalnız içerik değiştiyse) kopyalanır.
pub fn kopyalanacaklar(kaynak: &Path) -> Vec<&'static str> {
    let mut v = vec![EXE_ADI];
    v.extend(DLLLER);
    v.extend(BELGELER.into_iter().filter(|b| kaynak.join(b).is_file()));
    v
}

/// Klasörün içerik özeti (SHA-256, onaltılık): göreli yollar (küçük harf, `/`) sıralı; her
/// dosyanın yolu, boyutu ve içeriği katılır. Klasör yoksa `Ok(None)`.
pub fn klasor_ozeti(klasor: &Path) -> io::Result<Option<String>> {
    if !klasor.is_dir() {
        return Ok(None);
    }
    let mut dosyalar = Vec::new();
    let mut bekleyen = vec![klasor.to_path_buf()];
    while let Some(k) = bekleyen.pop() {
        for g in std::fs::read_dir(&k)? {
            let g = g?;
            let t = g.file_type()?;
            if t.is_dir() {
                bekleyen.push(g.path());
            } else if t.is_file() {
                dosyalar.push(g.path());
            }
        }
    }
    let goreli = |p: &Path| p.strip_prefix(klasor).unwrap_or(p).to_string_lossy().replace('\\', "/").to_lowercase();
    dosyalar.sort_by_key(|p| goreli(p));
    let mut h = Sha256::new();
    for p in &dosyalar {
        let ad = goreli(p);
        let veri = std::fs::read(p)?;
        h.update((ad.len() as u64).to_le_bytes());
        h.update(ad.as_bytes());
        h.update((veri.len() as u64).to_le_bytes());
        h.update(&veri);
    }
    Ok(Some(h.finalize().iter().map(|b| format!("{b:02x}")).collect()))
}

/// Dosya kilidi (çalışan exe, virüs tarayıcısı) geçici olabilir: paylaşım/erişim hatalarında
/// kısa aralıklarla yeniden denenir.
fn yeniden_dene<T>(mut is: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    const DENEME: u32 = 10;
    let mut son = None;
    for i in 0..DENEME {
        match is() {
            Ok(t) => return Ok(t),
            Err(h) => {
                // 5 erişim engellendi, 32 paylaşım ihlali, 33 kilit ihlali.
                let gecici = matches!(h.raw_os_error(), Some(5 | 32 | 33)) || h.kind() == io::ErrorKind::PermissionDenied;
                if !gecici || i + 1 == DENEME {
                    return Err(h);
                }
                son = Some(h);
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }
    Err(son.unwrap_or_else(|| io::Error::other("yeniden deneme")))
}

/// Adın sonuna ek koyar (`with_extension` "0.3" gibi noktalı adlarda uzantıyı değiştirirdi).
fn ekli(p: &Path, ek: &str) -> PathBuf {
    let mut ad = p.file_name().map(|a| a.to_os_string()).unwrap_or_default();
    ad.push(ek);
    p.with_file_name(ad)
}

fn eski_yolu(p: &Path) -> PathBuf {
    ekli(p, ".eski")
}

fn klasor_kopyala(kaynak: &Path, hedef: &Path) -> io::Result<()> {
    std::fs::create_dir_all(hedef)?;
    for g in std::fs::read_dir(kaynak)? {
        let g = g?;
        let t = g.file_type()?;
        let h = hedef.join(g.file_name());
        if t.is_dir() {
            klasor_kopyala(&g.path(), &h)?;
        } else if t.is_file() {
            std::fs::copy(g.path(), h)?;
        }
    }
    Ok(())
}

/// Kurulumun sonucu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KurulumOzeti {
    pub dosyalar: Vec<String>,
    /// Model klasörü kopyalandı mı (yoksa içerik aynıydı).
    pub model_kopyalandi: bool,
}

/// Geri alma kaydı: (hedefteki yol, kenara alınan eski kopya varsa yolu).
type Yapilanlar = Vec<(PathBuf, Option<PathBuf>)>;

fn dosyalari_geri_al(yapilanlar: &[(PathBuf, Option<PathBuf>)]) {
    for (yeni, eski) in yapilanlar.iter().rev() {
        let _ = yeniden_dene(|| match std::fs::remove_file(yeni) {
            Err(h) if h.kind() == io::ErrorKind::NotFound => Ok(()),
            r => r,
        });
        if let Some(e) = eski {
            let _ = yeniden_dene(|| std::fs::rename(e, yeni));
        }
    }
}

/// Tek dosyayı değiştirir: var olan `<ad>.eski` olarak kenara alınır, yenisi kopyalanır.
fn dosya_degistir(k: &Path, h: &Path, yapilanlar: &mut Yapilanlar) -> Result<(), String> {
    let eski = if h.exists() {
        let e = eski_yolu(h);
        let _ = std::fs::remove_file(&e);
        yeniden_dene(|| std::fs::rename(h, &e)).map_err(|x| format!("{} kenara alınamadı (Echora çalışıyor olabilir): {x}", h.display()))?;
        Some(e)
    } else {
        None
    };
    yapilanlar.push((h.to_path_buf(), eski));
    yeniden_dene(|| std::fs::copy(k, h)).map(drop).map_err(|x| format!("{} kopyalanamadı: {x}", h.display()))
}

/// Model değişimi: `(yeni model yerine kondu mu, kenara alınan eski model)`.
struct ModelDegisimi {
    kondu: bool,
    eski: Option<PathBuf>,
}

fn model_geri_al(h_model: &Path, m: &ModelDegisimi) {
    if m.kondu {
        let _ = std::fs::remove_dir_all(h_model);
    }
    if let Some(e) = &m.eski {
        if !h_model.exists() {
            let _ = yeniden_dene(|| std::fs::rename(e, h_model));
        }
    }
}

/// `kaynak`taki DLL'leri, modeli (yalnız içeriği farklıysa), belgeleri ve exe'yi `hedef`e kopyalar.
///
/// Sıra bilerek DLL'ler → model → belgeler → EXE EN SON: yarıda kesilirse eski exe yerinde kalır
/// (yeni exe eski DLL'lerle hiç açılmaz). Var olan her dosya önce `<ad>.eski` olarak kenara alınır;
/// bir adım başarısız olursa yapılanlar geri alınır ve hata döner. Başarıda `.eski`ler silinir.
/// Süreç bu arada ölürse kalan `.eski`ler bir sonraki açılışta `yarim_kalanlari_onar` ile toparlanır.
pub fn dosyalari_kur(kaynak: &Path, hedef: &Path) -> Result<KurulumOzeti, String> {
    if yazma_kapali_mi() {
        return Err("örnek kipinde kurulum yapılmaz".into());
    }
    kaynak_dogrula(kaynak)?;
    if ayni_yol(kaynak, hedef) {
        return Err("kaynak ve hedef aynı klasör".into());
    }
    std::fs::create_dir_all(hedef).map_err(|h| format!("{} oluşturulamadı: {h}", hedef.display()))?;

    let mut yapilanlar: Yapilanlar = Vec::new();
    let belgeler: Vec<&str> = BELGELER.into_iter().filter(|b| kaynak.join(b).is_file()).collect();

    // 1. DLL'ler
    for ad in DLLLER {
        if let Err(h) = dosya_degistir(&kaynak.join(ad), &hedef.join(ad), &mut yapilanlar) {
            dosyalari_geri_al(&yapilanlar);
            return Err(h);
        }
    }

    // 2. Model: yalnız içerik değiştiyse. Önce `.yeni`ye kopyalanır, sonra yer değiştirilir.
    let k_model = kaynak.join(MODEL_GORELI);
    let h_model = hedef.join(MODEL_GORELI);
    let farkli = match (klasor_ozeti(&k_model), klasor_ozeti(&h_model)) {
        (Ok(a), Ok(b)) => a != b,
        (Err(x), _) => {
            dosyalari_geri_al(&yapilanlar);
            return Err(format!("model klasörü okunamadı: {x}"));
        }
        (_, Err(_)) => true,
    };
    let mut model = ModelDegisimi { kondu: false, eski: None };
    if farkli {
        let yeni = ekli(&h_model, ".yeni");
        let _ = std::fs::remove_dir_all(&yeni);
        let sonuc = klasor_kopyala(&k_model, &yeni).and_then(|()| {
            if h_model.exists() {
                let e = ekli(&h_model, ".eski");
                let _ = std::fs::remove_dir_all(&e);
                yeniden_dene(|| std::fs::rename(&h_model, &e))?;
                model.eski = Some(e);
            }
            yeniden_dene(|| std::fs::rename(&yeni, &h_model))?;
            model.kondu = true;
            Ok(())
        });
        if let Err(x) = sonuc {
            let _ = std::fs::remove_dir_all(&yeni);
            model_geri_al(&h_model, &model);
            dosyalari_geri_al(&yapilanlar);
            return Err(format!("tanıma modeli kopyalanamadı: {x}"));
        }
    }

    // 3. Belgeler, 4. exe (en son).
    for ad in belgeler.iter().copied().chain(std::iter::once(EXE_ADI)) {
        if let Err(h) = dosya_degistir(&kaynak.join(ad), &hedef.join(ad), &mut yapilanlar) {
            dosyalari_geri_al(&yapilanlar);
            model_geri_al(&h_model, &model);
            return Err(h);
        }
    }

    // Başarı: kenara alınanlar silinir (silinemezse zararsız; açılışta toparlanır).
    for (_, eski) in &yapilanlar {
        if let Some(e) = eski {
            let _ = std::fs::remove_file(e);
        }
    }
    if let Some(e) = model.eski {
        let _ = std::fs::remove_dir_all(e);
    }
    let dosyalar = yapilanlar.iter().filter_map(|(y, _)| y.file_name().map(|a| a.to_string_lossy().into_owned())).collect();
    Ok(KurulumOzeti { dosyalar, model_kopyalandi: farkli })
}

/// Yarıda kesilmiş bir kurulumun artıklarını toparlar (açılışta, kurulu klasör için): bir
/// `<ad>.eski` dosyasının karşılığı yoksa ya da 0 baytsa eski dosya geri konur, değilse `.eski`
/// silinir. Model klasörü için de aynı kural (`.eski` / `.yeni`). Dönen: yapılanların notları.
pub fn yarim_kalanlari_onar(hedef: &Path) -> Vec<String> {
    let mut notlar = Vec::new();
    if yazma_kapali_mi() {
        return notlar;
    }
    if let Ok(liste) = std::fs::read_dir(hedef) {
        for g in liste.flatten() {
            let yol = g.path();
            let ad = g.file_name().to_string_lossy().into_owned();
            let Some(asil_ad) = ad.strip_suffix(".eski") else { continue };
            if !yol.is_file() || asil_ad.is_empty() {
                continue;
            }
            let asil = hedef.join(asil_ad);
            let bozuk = std::fs::metadata(&asil).map(|m| m.len() == 0).unwrap_or(true);
            if bozuk {
                let _ = std::fs::remove_file(&asil);
                if std::fs::rename(&yol, &asil).is_ok() {
                    notlar.push(format!("yarım kalan kurulumdan {asil_ad} geri yüklendi"));
                }
            } else if std::fs::remove_file(&yol).is_ok() {
                notlar.push(format!("artık {ad} silindi"));
            }
        }
    }
    let model = hedef.join(MODEL_GORELI);
    let _ = std::fs::remove_dir_all(ekli(&model, ".yeni"));
    let eski_model = ekli(&model, ".eski");
    if eski_model.is_dir() {
        if !model.join("final.mdl").is_file() {
            let _ = std::fs::remove_dir_all(&model);
            if std::fs::rename(&eski_model, &model).is_ok() {
                notlar.push("yarım kalan kurulumdan tanıma modeli geri yüklendi".into());
            }
        } else if std::fs::remove_dir_all(&eski_model).is_ok() {
            notlar.push("artık model kopyası silindi".into());
        }
    }
    notlar
}

// --- Süreçler ---

fn surec_yolu(pid: u32) -> Option<PathBuf> {
    // SAFETY: yalnız sınırlı sorgu izniyle açılır, aşağıda kapatılır.
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut tampon = vec![0u16; 1024];
    let mut uzunluk = tampon.len() as u32;
    // SAFETY: tampon `uzunluk` karakter; çağrı süresince yaşar.
    let sonuc = unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(tampon.as_mut_ptr()), &mut uzunluk) };
    // SAFETY: `OpenProcess` ile açılan tutamaç.
    unsafe {
        let _ = CloseHandle(h);
    }
    sonuc.ok()?;
    Some(PathBuf::from(String::from_utf16_lossy(&tampon[..uzunluk as usize])))
}

struct Anlik(HANDLE);
impl Drop for Anlik {
    fn drop(&mut self) {
        // SAFETY: CreateToolhelp32Snapshot'ın döndürdüğü tutamaç.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Görüntüsü tam olarak `exe` olan çalışan süreçlerin kimlikleri (salt okuma; bu süreç hariç).
pub fn calisan_surecler(exe: &Path) -> Vec<u32> {
    let ad = exe.file_name().map(|a| a.to_string_lossy().to_lowercase()).unwrap_or_default();
    let kendi = std::process::id();
    let mut v = Vec::new();
    // SAFETY: salt-okuma anlık görüntüsü; tutamaç `Anlik` düşerken kapanır.
    let Ok(h) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else { return v };
    let anlik = Anlik(h);
    let mut g = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    // SAFETY: `g` doğru boyutla başlatıldı.
    let mut var = unsafe { Process32FirstW(anlik.0, &mut g) }.is_ok();
    while var {
        let son = g.szExeFile.iter().position(|&c| c == 0).unwrap_or(g.szExeFile.len());
        let surec_adi = String::from_utf16_lossy(&g.szExeFile[..son]).to_lowercase();
        if surec_adi == ad && g.th32ProcessID != kendi {
            if surec_yolu(g.th32ProcessID).is_some_and(|y| ayni_yol(&y, exe)) {
                v.push(g.th32ProcessID);
            }
        }
        // SAFETY: yukarıdakiyle aynı.
        var = unsafe { Process32NextW(anlik.0, &mut g) }.is_ok();
    }
    v
}

// --- Başlat menüsü ---

fn genis(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn genis_yol(p: &Path) -> Vec<u16> {
    genis(&p.to_string_lossy())
}

/// Kullanıcının Başlat menüsü "Programlar" klasörü.
pub fn programlar_klasoru() -> Result<PathBuf, String> {
    // SAFETY: dönen PWSTR CoTaskMemFree ile bırakılır.
    unsafe {
        let p = SHGetKnownFolderPath(&FOLDERID_Programs, KF_FLAG_DEFAULT, None).map_err(|h| format!("Başlat menüsü klasörü bulunamadı: {h}"))?;
        let yol = p.to_string().map_err(|h| h.to_string());
        CoTaskMemFree(Some(p.0 as *const _));
        yol.map(PathBuf::from)
    }
}

/// Bu iş parçacığında COM'u açar; düşerken kapatır (yalnız açılış başarılıysa).
struct Com(bool);
impl Com {
    fn ac() -> Self {
        // SAFETY: iş parçacığı başına; eşleşen CoUninitialize Drop'ta.
        Com(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: başarılı CoInitializeEx ile eşleşir.
            unsafe { CoUninitialize() };
        }
    }
}

fn kisayol_yaz(lnk: &Path, hedef: &Path, arguman: &str, aciklama: &str) -> Result<(), String> {
    let calisma = hedef.parent().unwrap_or(Path::new("."));
    let (h, a, c, d, l) = (genis_yol(hedef), genis(arguman), genis_yol(calisma), genis(aciklama), genis_yol(lnk));
    // SAFETY: tüm metinler NUL'lu ve çağrı boyunca yaşar; COM nesnesi bu iş parçacığında.
    unsafe {
        let k: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(|x| format!("kısayol nesnesi: {x}"))?;
        k.SetPath(PCWSTR(h.as_ptr())).map_err(|x| x.to_string())?;
        k.SetArguments(PCWSTR(a.as_ptr())).map_err(|x| x.to_string())?;
        k.SetWorkingDirectory(PCWSTR(c.as_ptr())).map_err(|x| x.to_string())?;
        k.SetDescription(PCWSTR(d.as_ptr())).map_err(|x| x.to_string())?;
        k.SetIconLocation(PCWSTR(h.as_ptr()), 0).map_err(|x| x.to_string())?;
        let p: IPersistFile = k.cast().map_err(|x| x.to_string())?;
        p.Save(PCWSTR(l.as_ptr()), true).map_err(|x| format!("{} yazılamadı: {x}", lnk.display()))
    }
}

/// Başlat menüsüne "Echora" ve "Echora Stüdyo" kısayollarını ekler; klasörü döner.
pub fn kisayollari_olustur(exe: &Path) -> Result<PathBuf, String> {
    if yazma_kapali_mi() {
        return Err("örnek kipinde kısayol yazılmaz".into());
    }
    let _com = Com::ac();
    let menu = programlar_klasoru()?.join(MENU_KLASORU);
    std::fs::create_dir_all(&menu).map_err(|h| format!("{} oluşturulamadı: {h}", menu.display()))?;
    kisayol_yaz(&menu.join("Echora.lnk"), exe, "", "Sesli komutla bilgisayar kontrolü")?;
    kisayol_yaz(&menu.join("Echora Stüdyo.lnk"), exe, "--studyo", "Echora ayarları ve efekt önizlemesi")?;
    Ok(menu)
}

/// Eski "Yankı" kurulumunu (çalışmıyorsa) kaldırır: kurulum klasörü, Başlat menüsü klasörü ve
/// eski kurulumu gösteren `Run\Yanki` değeri. Ayar klasörleri silinmez (Echora ilk açılışta
/// taşır). Dönen: kullanıcıya gösterilecek notlar.
pub fn eski_yanki_temizle() -> Result<Vec<String>, String> {
    if yazma_kapali_mi() {
        return Err("örnek kipinde eski kurulum temizlenmez".into());
    }
    let mut notlar = Vec::new();
    let Some(eski) = eski_kurulum_klasoru() else { return Ok(notlar) };
    let eski_exe = eski.join(ESKI_EXE_ADI);
    // Yalnız gerçekten eski Yankı kurulumu olan klasör silinir (yanki.exe yoksa dokunulmaz).
    if !eski_exe.is_file() {
        return Ok(notlar);
    }
    if !calisan_surecler(&eski_exe).is_empty() {
        notlar.push("Eski Yankı çalışıyor; kaldırılmadı. Tepsi menüsünden çıkıp kurulumu yeniden çalıştırın.".into());
        return Ok(notlar);
    }
    if eski.exists() {
        std::fs::remove_dir_all(&eski).map_err(|h| format!("{} silinemedi: {h}", eski.display()))?;
        notlar.push(format!("Eski Yankı kurulumu kaldırıldı: {}", eski.display()));
    }
    if let Ok(p) = programlar_klasoru() {
        let menu = p.join(ESKI_MENU_KLASORU);
        if menu.exists() && std::fs::remove_dir_all(&menu).is_ok() {
            notlar.push("Eski Başlat menüsü girdileri silindi.".into());
        }
    }
    if crate::kayit::eski_degeri_temizle(&eski_exe)? {
        notlar.push("Eski \"Windows ile başlat\" kaydı silindi.".into());
    }
    Ok(notlar)
}

/// Eski kurulum klasörü var mı (bilgi için).
pub fn eski_yanki_var_mi() -> bool {
    eski_kurulum_klasoru().is_some_and(|e| e.join(ESKI_EXE_ADI).is_file())
}

// --- Mikrofon izni (yalnız okuma) ---

const IZIN_ANAHTARI: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MikrofonIzni {
    Acik,
    /// "Bu cihazda mikrofon erişimi" kapalı (yönetici ayarı).
    CihazKapali,
    /// "Uygulamaların mikrofonunuza erişmesine izin verin" kapalı.
    UygulamalarKapali,
    /// "Masaüstü uygulamalarının mikrofonunuza erişmesine izin verin" kapalı.
    MasaustuKapali,
}

impl MikrofonIzni {
    pub fn acik_mi(self) -> bool {
        self == MikrofonIzni::Acik
    }

    pub fn aciklama(self) -> &'static str {
        match self {
            MikrofonIzni::Acik => "Masaüstü uygulamalarının mikrofona erişimi açık.",
            MikrofonIzni::CihazKapali => "Bu cihazda mikrofon erişimi kapalı.",
            MikrofonIzni::UygulamalarKapali => "Uygulamaların mikrofona erişimi kapalı.",
            MikrofonIzni::MasaustuKapali => "Masaüstü uygulamalarının mikrofona erişimi kapalı.",
        }
    }
}

/// Saf karar: değerler `ConsentStore\microphone` (`Value`) altından; yoksa açık sayılır
/// (Windows'un varsayılanı). Yalnız "Deny" kapalıdır.
pub fn izin_karari(cihaz: Option<&str>, genel: Option<&str>, masaustu: Option<&str>) -> MikrofonIzni {
    let kapali = |d: Option<&str>| d.is_some_and(|d| d.trim().eq_ignore_ascii_case("deny"));
    if kapali(cihaz) {
        MikrofonIzni::CihazKapali
    } else if kapali(genel) {
        MikrofonIzni::UygulamalarKapali
    } else if kapali(masaustu) {
        MikrofonIzni::MasaustuKapali
    } else {
        MikrofonIzni::Acik
    }
}

/// Kayıt defterinden YALNIZ OKUR: HKLM (cihaz), HKCU (uygulamalar) ve HKCU `NonPackaged`
/// (masaüstü uygulamaları). Okunamayan değer "yok" sayılır.
pub fn mikrofon_izni_oku() -> MikrofonIzni {
    let oku = |k, a: &str| crate::kayit::metin_oku(k, a, "Value").ok().flatten();
    let masaustu_anahtari = format!(r"{IZIN_ANAHTARI}\NonPackaged");
    izin_karari(
        oku(HKEY_LOCAL_MACHINE, IZIN_ANAHTARI).as_deref(),
        oku(HKEY_CURRENT_USER, IZIN_ANAHTARI).as_deref(),
        oku(HKEY_CURRENT_USER, &masaustu_anahtari).as_deref(),
    )
}

/// Windows'un mikrofon gizlilik sayfasını açar (ayar kullanıcı tarafından değiştirilir).
pub fn mikrofon_ayarlarini_ac() -> Result<(), String> {
    // SAFETY: sabit metinler; ShellExecuteW eşzamansız başlatır.
    let r = unsafe { ShellExecuteW(None, w!("open"), w!("ms-settings:privacy-microphone"), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    if (r.0 as isize) <= 32 {
        return Err("Ayarlar uygulaması açılamadı".into());
    }
    Ok(())
}

/// Sabit bir https adresini varsayılan tarayıcıda açar (ör. sürüm notları).
pub fn adresi_ac(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("yalnız https adresleri açılır".into());
    }
    let u = genis(url);
    // SAFETY: metin NUL'lu ve çağrı boyunca yaşar.
    let r = unsafe { ShellExecuteW(None, w!("open"), PCWSTR(u.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    if (r.0 as isize) <= 32 {
        return Err("tarayıcı açılamadı".into());
    }
    Ok(())
}

#[cfg(test)]
mod testler {
    use super::*;

    struct Gecici(PathBuf);
    impl Gecici {
        fn yeni(ad: &str) -> Self {
            let k = std::env::temp_dir().join(format!("studyo-test-kurulum-{ad}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&k);
            std::fs::create_dir_all(&k).unwrap();
            Gecici(k)
        }
    }
    impl Drop for Gecici {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Sahte sürüm klasörü (gerçek exe değil; yalnız dosya kopyalama sınanır).
    fn sahte_kaynak(k: &Path, surum: &str) {
        std::fs::create_dir_all(k.join(MODEL_GORELI).join("ivector")).unwrap();
        std::fs::write(k.join(EXE_ADI), format!("exe {surum}")).unwrap();
        for d in DLLLER {
            std::fs::write(k.join(d), format!("{d} {surum}")).unwrap();
        }
        std::fs::write(k.join("README.md"), "oku").unwrap();
        std::fs::write(k.join(MODEL_GORELI).join("final.mdl"), "model").unwrap();
        std::fs::write(k.join(MODEL_GORELI).join("ivector").join("final.ie"), "ie").unwrap();
    }

    #[test]
    fn konum_karari() {
        let k = PathBuf::from(r"C:\Users\x\AppData\Local\Programs\Echora");
        assert_eq!(konum(&k.join("echora.exe"), Some(&k)), Konum::Kurulu);
        assert_eq!(konum(Path::new(r"c:\users\X\appdata\local\programs\echora\ECHORA.exe"), Some(&k)), Konum::Kurulu);
        assert_eq!(konum(Path::new(r"C:\kod\ekran\target\release\echora.exe"), Some(&k)), Konum::Gelistirme);
        assert_eq!(konum(Path::new(r"C:\kod\Target\Debug\echora.exe"), Some(&k)), Konum::Gelistirme);
        assert_eq!(konum(Path::new(r"C:\Users\x\Downloads\Echora-0.1.0-win64\echora.exe"), Some(&k)), Konum::Kurulmamis);
        // Kurulum klasörünün alt klasörü kurulu sayılmaz.
        assert_eq!(konum(&k.join("alt").join("echora.exe"), Some(&k)), Konum::Kurulmamis);
        assert_eq!(konum(Path::new(r"C:\x\echora.exe"), None), Konum::Kurulmamis);
    }

    #[test]
    fn izin_karar_tablosu() {
        assert_eq!(izin_karari(None, None, None), MikrofonIzni::Acik);
        assert_eq!(izin_karari(Some("Allow"), Some("Allow"), Some("Allow")), MikrofonIzni::Acik);
        assert_eq!(izin_karari(Some("Deny"), Some("Allow"), Some("Allow")), MikrofonIzni::CihazKapali);
        assert_eq!(izin_karari(None, Some("deny"), None), MikrofonIzni::UygulamalarKapali);
        assert_eq!(izin_karari(None, Some("Allow"), Some(" Deny ")), MikrofonIzni::MasaustuKapali);
        assert_eq!(izin_karari(None, Some("Prompt"), None), MikrofonIzni::Acik);
    }

    #[test]
    fn mikrofon_izni_okunabilir() {
        // Yalnız okuma; sonuç makineye bağlı, çökmemeli.
        let _ = mikrofon_izni_oku().aciklama();
    }

    #[test]
    fn kopyalanacaklar_belgeleri_varsa_ekler() {
        let t = Gecici::yeni("liste");
        sahte_kaynak(&t.0, "1");
        let v = kopyalanacaklar(&t.0);
        assert_eq!(v[0], EXE_ADI);
        assert!(v.contains(&"libvosk.dll"));
        assert!(v.contains(&"README.md"));
        assert!(!v.contains(&"UCUNCU-TARAF.txt"));
    }

    #[test]
    fn kurulum_kopyalar_gunceller_ve_modeli_gerekirse_kopyalar() {
        let t = Gecici::yeni("kur");
        let (k1, k2, h) = (t.0.join("k1"), t.0.join("k2"), t.0.join("hedef"));
        sahte_kaynak(&k1, "1");
        sahte_kaynak(&k2, "2");
        let o = dosyalari_kur(&k1, &h).unwrap();
        assert!(o.model_kopyalandi);
        assert_eq!(std::fs::read_to_string(h.join(EXE_ADI)).unwrap(), "exe 1");
        assert_eq!(std::fs::read_to_string(h.join(MODEL_GORELI).join("ivector").join("final.ie")).unwrap(), "ie");
        // Aynı model: kopyalanmaz; exe güncellenir; .eski kalmaz.
        let o = dosyalari_kur(&k2, &h).unwrap();
        assert!(!o.model_kopyalandi);
        assert_eq!(std::fs::read_to_string(h.join(EXE_ADI)).unwrap(), "exe 2");
        assert!(!h.join("echora.exe.eski").exists());
        // Model değişince yer değiştirir.
        std::fs::write(k1.join(MODEL_GORELI).join("final.mdl"), "model yeni").unwrap();
        assert!(dosyalari_kur(&k1, &h).unwrap().model_kopyalandi);
        assert_eq!(std::fs::read_to_string(h.join(MODEL_GORELI).join("final.mdl")).unwrap(), "model yeni");
        assert!(!ekli(&h.join(MODEL_GORELI), ".eski").exists());
        // Kaynak = hedef reddedilir.
        assert!(dosyalari_kur(&h, &h).is_err());
    }

    #[test]
    fn kurulum_hatasinda_eskiler_geri_doner() {
        let t = Gecici::yeni("geri");
        let (k1, k2, h) = (t.0.join("k1"), t.0.join("k2"), t.0.join("hedef"));
        sahte_kaynak(&k1, "1");
        sahte_kaynak(&k2, "2");
        dosyalari_kur(&k1, &h).unwrap();
        // Yeni sürümde model değişti; ama hedefte modelin ".yeni" adında bir DOSYA var: model
        // kopyası başarısız olur → o ana dek değiştirilen exe ve DLL'ler geri alınmalı.
        std::fs::write(k2.join(MODEL_GORELI).join("final.mdl"), "model 2").unwrap();
        let engel = ekli(&h.join(MODEL_GORELI), ".yeni");
        std::fs::write(&engel, "engel").unwrap();
        assert!(dosyalari_kur(&k2, &h).is_err());
        assert_eq!(std::fs::read_to_string(h.join(EXE_ADI)).unwrap(), "exe 1");
        for d in DLLLER {
            assert_eq!(std::fs::read_to_string(h.join(d)).unwrap(), format!("{d} 1"));
            assert!(!ekli(&h.join(d), ".eski").exists());
        }
        assert!(!h.join("echora.exe.eski").exists());
        assert_eq!(std::fs::read_to_string(h.join(MODEL_GORELI).join("final.mdl")).unwrap(), "model");
        // Engel kalkınca aynı kurulum başarılı.
        std::fs::remove_file(&engel).unwrap();
        assert!(dosyalari_kur(&k2, &h).unwrap().model_kopyalandi);
        assert_eq!(std::fs::read_to_string(h.join(EXE_ADI)).unwrap(), "exe 2");
    }

    #[test]
    fn exe_en_son_kopyalanir() {
        let t = Gecici::yeni("sira");
        let (k1, k2, h) = (t.0.join("k1"), t.0.join("k2"), t.0.join("hedef"));
        sahte_kaynak(&k1, "1");
        sahte_kaynak(&k2, "2");
        dosyalari_kur(&k1, &h).unwrap();
        // Belge adımı başarısız (hedefte README'nin ".eski" adında silinemeyen, boş olmayan bir
        // klasör var → kenara alma başarısız): DLL'ler ve model yeni, ama exe hâlâ eski sürüm ve
        // geri almadan sonra her şey eski haline döner.
        std::fs::write(k2.join(MODEL_GORELI).join("final.mdl"), "model 2").unwrap();
        std::fs::create_dir_all(h.join("README.md.eski").join("dolu")).unwrap();
        std::fs::write(h.join("README.md.eski").join("dolu").join("x"), "x").unwrap();
        assert!(dosyalari_kur(&k2, &h).is_err());
        assert_eq!(std::fs::read_to_string(h.join(EXE_ADI)).unwrap(), "exe 1");
        assert_eq!(std::fs::read_to_string(h.join(DLLLER[0])).unwrap(), format!("{} 1", DLLLER[0]));
        assert_eq!(std::fs::read_to_string(h.join(MODEL_GORELI).join("final.mdl")).unwrap(), "model");
    }

    #[test]
    fn yarim_kalan_kurulum_onarilir() {
        let t = Gecici::yeni("onar");
        let h = t.0.join("hedef");
        sahte_kaynak(&h, "1");
        // exe yarım kaldı (0 bayt) → eskisi geri gelir; DLL tam → .eski silinir; asıl yoksa geri gelir.
        std::fs::write(h.join("echora.exe.eski"), "exe eski").unwrap();
        std::fs::write(h.join(EXE_ADI), "").unwrap();
        std::fs::write(h.join("libvosk.dll.eski"), "dll eski").unwrap();
        std::fs::write(h.join("README.md.eski"), "oku eski").unwrap();
        std::fs::remove_file(h.join("README.md")).unwrap();
        let m = h.join(MODEL_GORELI);
        std::fs::create_dir_all(ekli(&m, ".yeni")).unwrap();
        std::fs::create_dir_all(ekli(&m, ".eski")).unwrap();
        let notlar = yarim_kalanlari_onar(&h);
        assert!(!notlar.is_empty());
        assert_eq!(std::fs::read_to_string(h.join(EXE_ADI)).unwrap(), "exe eski");
        assert_eq!(std::fs::read_to_string(h.join("libvosk.dll")).unwrap(), "libvosk.dll 1");
        assert_eq!(std::fs::read_to_string(h.join("README.md")).unwrap(), "oku eski");
        assert!(!h.join("libvosk.dll.eski").exists() && !h.join("echora.exe.eski").exists());
        assert!(!ekli(&m, ".yeni").exists() && !ekli(&m, ".eski").exists());
        assert!(m.join("final.mdl").is_file());
        assert!(yarim_kalanlari_onar(&h).is_empty());
    }

    #[test]
    fn eksik_kaynak_reddedilir_hedefe_dokunulmaz() {
        let t = Gecici::yeni("eksik");
        let (k, h) = (t.0.join("k"), t.0.join("h"));
        sahte_kaynak(&k, "1");
        std::fs::remove_file(k.join(DLLLER[2])).unwrap();
        assert!(dosyalari_kur(&k, &h).unwrap_err().contains(DLLLER[2]));
        assert!(!h.exists());
    }
    #[test]
    fn klasor_ozeti_icerige_ve_ada_duyarli() {
        let t = Gecici::yeni("ozet");
        let a = t.0.join("a");
        assert_eq!(klasor_ozeti(&a).unwrap(), None);
        std::fs::create_dir_all(a.join("alt")).unwrap();
        std::fs::write(a.join("x"), "1").unwrap();
        std::fs::write(a.join("alt").join("y"), "2").unwrap();
        let o1 = klasor_ozeti(&a).unwrap();
        assert_eq!(klasor_ozeti(&a).unwrap(), o1);
        std::fs::write(a.join("x"), "3").unwrap();
        let o2 = klasor_ozeti(&a).unwrap();
        assert_ne!(o1, o2);
        std::fs::rename(a.join("x"), a.join("z")).unwrap();
        assert_ne!(klasor_ozeti(&a).unwrap(), o2);
    }

    #[test]
    fn kendi_surecimiz_listelenmez() {
        let exe = std::env::current_exe().unwrap();
        assert!(calisan_surecler(&exe).is_empty());
    }
}
