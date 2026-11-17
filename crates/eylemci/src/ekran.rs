//! Monitör sorguları ve DDC/CI güç kipi (VCP 0xD6) okuma/yazma.

use std::mem::size_of;
use std::sync::{Mutex, MutexGuard};
use std::thread::sleep;
use std::time::Duration;

use windows::Win32::Devices::Display::{
    CapabilitiesRequestAndCapabilitiesReply, DestroyPhysicalMonitors, GetCapabilitiesStringLength,
    GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR,
    GetVCPFeatureAndVCPFeatureReply, SetVCPFeature, PHYSICAL_MONITOR,
};
use crate::Dikdortgen;
use windows::core::BOOL;
use windows::Win32::Foundation::{HANDLE, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};

/// Süreç içi tek DDC/CI kilidi: DDC/CI I2C hattı üzerinde çalışır ve eşzamanlı konuşmalar
/// birbirini bozar. Bu crate'teki TÜM DDC okuma/yazma giriş noktaları (güç kipi, yetenek dizesi,
/// yakınlık sensörü) bu kilidi tutar. Kilidi tutarken başka kilit BEKLEMEYİN; kilit giriş
/// noktalarında alınır, iç yardımcılar (`guc_oku`, `vcp_oku`...) kilitsizdir, iç içe çağrılmaz.
static DDC_KILIDI: Mutex<()> = Mutex::new(());

/// Süreçler arası DDC kilidinin adı: Echora (arka plan) ile Stüdyo ayrı süreçlerdir ama aynı
/// I2C hattını kullanır. Oturum yereldir (`Local\`); iki süreç de kullanıcının oturumundadır.
const SISTEM_KILIDI_ADI: windows::core::PCWSTR = windows::core::w!("Local\\Echora.DDC");
/// Süreçler arası kilit için en uzun bekleme. Dolarsa yine de devam edilir (kilitlenme yerine
/// nadir bir çakışma riski kabul edilir; süreç içi kilit her zaman tutulur).
const SISTEM_KILIDI_BEKLEME_MS: u32 = 5000;

/// Süreç içi + süreçler arası DDC kilidi. Drop'ta ikisi de bırakılır. `Send` değildir: adlı
/// mutex onu bekleyen iş parçacığına aittir ve aynı iş parçacığında bırakılmalıdır.
pub(crate) struct DdcKilidi {
    _ic: MutexGuard<'static, ()>,
    sistem: Option<HANDLE>,
}

impl Drop for DdcKilidi {
    fn drop(&mut self) {
        if let Some(h) = self.sistem {
            // SAFETY: h bu iş parçacığının beklediği (sahip olduğu) adlı mutex.
            let _ = unsafe { windows::Win32::System::Threading::ReleaseMutex(h) };
        }
    }
}

/// Adlı mutex tutamacı (süreç boyunca açık kalır); oluşturulamazsa None.
fn sistem_kilidi() -> Option<HANDLE> {
    static TUTAMAC: std::sync::OnceLock<Option<isize>> = std::sync::OnceLock::new();
    let h = (*TUTAMAC.get_or_init(|| {
        // SAFETY: sabit, NUL sonlu ad; varsayılan güvenlik; ilk sahiplik istenmez.
        unsafe { windows::Win32::System::Threading::CreateMutexW(None, false, SISTEM_KILIDI_ADI) }
            .ok()
            .map(|h| h.0 as isize)
    }))?;
    Some(HANDLE(h as *mut _))
}

/// DDC kilidini alır; zehirlenmişse (tutan iş parçacığı panik etti) kilidi kurtarır. Önce süreç
/// içi kilit, sonra süreçler arası adlı mutex (en çok `SISTEM_KILIDI_BEKLEME_MS`; terk edilmiş
/// mutex, sahibi öldüğü için, alınmış sayılır).
pub(crate) fn ddc_kilidi() -> DdcKilidi {
    use windows::Win32::Foundation::{WAIT_ABANDONED, WAIT_OBJECT_0};
    let ic = DDC_KILIDI.lock().unwrap_or_else(|e| e.into_inner());
    let sistem = sistem_kilidi().filter(|&h| {
        // SAFETY: h geçerli adlı mutex tutamacı.
        let r = unsafe { windows::Win32::System::Threading::WaitForSingleObject(h, SISTEM_KILIDI_BEKLEME_MS) };
        r == WAIT_OBJECT_0 || r == WAIT_ABANDONED
    });
    DdcKilidi { _ic: ic, sistem }
}

/// VCP kodu: güç kipi. Değerler: 1 = açık, 4 = bekleme (DPM kapalı), 5 = kapalı (güç düğmesine
/// basılmış gibi).
const VCP_GUC: u8 = 0xD6;
const GUC_ACIK: u32 = 1;
const GUC_BEKLEME: u32 = 4;
const GUC_KAPALI: u32 = 5;

/// Bir monitöre sırayla denenecek kapatma değerlerini, DDC/CI yetenek dizesinden çıkarır.
///
/// Monitörler 0xD6'nın yalnız bir kısmını destekler ve desteklemedikleri değeri SESSİZCE yok
/// sayar (ölçüldü: ASUS XG27UCDMG ve AOC C27G2AE `D6(01 05)` bildiriyor; eskiden yazılan 4'ü yok
/// sayıp açık kaldılar). Sıra: bekleme (4) varsa önce o — monitör DDC'yi canlı tutar, sesle geri
/// açılması kesindir; yoksa kapalı (5).
/// - Dize okunamadıysa (`None`) ya da 0xD6 dizede değer listesiyle geçmiyorsa: [4, 5] (önce yumuşak).
/// - 0xD6 listeleniyor ama ne 4 ne 5 varsa: boş — monitör DDC ile kapatılamaz.
pub(crate) fn kapatma_adaylari(yetenek: Option<&str>) -> Vec<u32> {
    let varsayilan = vec![GUC_BEKLEME, GUC_KAPALI];
    let Some(dize) = yetenek else { return varsayilan };
    let Some(degerler) = vcp_degerleri(dize, 0xD6) else { return varsayilan };
    [GUC_BEKLEME, GUC_KAPALI].into_iter().filter(|d| degerler.contains(d)).collect()
}

/// MCCS yetenek dizesinin `vcp(...)` bölümünden bir kodun değer listesini çıkarır:
/// `vcp(02 04 D6(01 05) E1(00 01))` → `D6` için `Some([1, 5])`. Kod değersiz geçiyorsa
/// (`... D6 ...`) ya da hiç yoksa `None`. Büyük/küçük harf duyarsız; iç içe parantezleri sayar.
pub(crate) fn vcp_degerleri(dize: &str, kod: u8) -> Option<Vec<u32>> {
    let kucuk = dize.to_ascii_lowercase();
    let bas = kucuk.find("vcp(")? + 4;
    let govde = &kucuk[bas..];
    // vcp bölümünün sonu: parantez derinliği 0'a inince.
    let mut derinlik = 1i32;
    let mut son = govde.len();
    for (i, c) in govde.char_indices() {
        match c {
            '(' => derinlik += 1,
            ')' => {
                derinlik -= 1;
                if derinlik == 0 {
                    son = i;
                    break;
                }
            }
            _ => {}
        }
    }
    let vcp = &govde[..son];
    let aranan = format!("{kod:02x}");
    let b = vcp.as_bytes();
    let mut i = 0;
    while i < b.len() {
        // Yalnız en üst düzeydeki belirteçlere bak: değer listesi içindeki "d6" kod sayılmaz.
        if b[i] == b'(' {
            let mut d = 1;
            i += 1;
            while i < b.len() && d > 0 {
                match b[i] {
                    b'(' => d += 1,
                    b')' => d -= 1,
                    _ => {}
                }
                i += 1;
            }
            continue;
        }
        if b[i].is_ascii_hexdigit() {
            let bas = i;
            while i < b.len() && b[i].is_ascii_hexdigit() {
                i += 1;
            }
            if vcp[bas..i] == aranan && i < b.len() && b[i] == b'(' {
                let ic_bas = i + 1;
                let ic_son = vcp[ic_bas..].find(')').map(|k| ic_bas + k)?;
                let degerler = vcp[ic_bas..ic_son]
                    .split_whitespace()
                    .filter_map(|s| u32::from_str_radix(s, 16).ok())
                    .collect();
                return Some(degerler);
            }
            continue;
        }
        i += 1;
    }
    None
}

/// DDC/CI yetenek dizesini okur (yavaş olabilir: yüzlerce ms). Okunamazsa `None`.
fn yetenek_dizesi(h: HANDLE) -> Option<String> {
    let mut uzunluk = 0u32;
    // SAFETY: h geçerli fiziksel monitör tutamacı; uzunluk geçerli bir çıktı işaretçisi.
    if unsafe { GetCapabilitiesStringLength(h, &mut uzunluk) } == 0 || uzunluk == 0 || uzunluk > 64 * 1024 {
        return None;
    }
    let mut tampon = vec![0u8; uzunluk as usize];
    // SAFETY: tampon tam `uzunluk` bayt ve yazılabilir; işlev sonlandırıcı sıfırı da içine yazar.
    if unsafe { CapabilitiesRequestAndCapabilitiesReply(h, &mut tampon) } == 0 {
        return None;
    }
    let son = tampon.iter().position(|&c| c == 0).unwrap_or(tampon.len());
    Some(String::from_utf8_lossy(&tampon[..son]).into_owned())
}

/// Bir monitörün (ilk okunabilen fiziksel monitörünün) kapatma adaylarını DDC'den okur.
pub(crate) fn monitor_kapatma_adaylari(hmon: HMONITOR) -> Vec<u32> {
    monitor_kapatma_adaylari_okunan(hmon).0
}

/// `monitor_kapatma_adaylari` ve yetenek dizesinin gerçekten okunup okunmadığı. Okunamadıysa
/// adaylar varsayılandır (önbelleğe alınmamalı: geçici bir okuma hatası kalıcı olmasın).
pub(crate) fn monitor_kapatma_adaylari_okunan(hmon: HMONITOR) -> (Vec<u32>, bool) {
    let _kilit = ddc_kilidi();
    let yetenek = Fiziksel::al(hmon).ok().and_then(|f| f.tutamaclar().into_iter().find_map(yetenek_dizesi));
    (kapatma_adaylari(yetenek.as_deref()), yetenek.is_some())
}

/// `MONITORINFOF_PRIMARY` (Gdi sabiti özellik adlarına bağlı olmasın diye yerel).
const MONITORINFOF_PRIMARY: u32 = 1;

#[derive(Debug, Clone)]
pub struct EkranBilgisi {
    /// `\\.\DISPLAY1` biçiminde aygıt adı.
    pub aygit_adi: String,
    pub ana: bool,
    pub sol: i32,
    pub ust: i32,
    pub genislik: i32,
    pub yukseklik: i32,
}

/// Bir ekran ve o anki tutamacı. Tutamaçlar yalnız çağrı süresince geçerli sayılır
/// (ekran dizilimi değişebilir), bu yüzden saklanmaz; her işlemde yeniden bulunur.
pub(crate) struct Monitor {
    pub hmon: HMONITOR,
    pub bilgi: EkranBilgisi,
    /// Çalışma alanı (görev çubuğu hariç), sanal masaüstü koordinatlarında.
    pub calisma: Dikdortgen,
}

unsafe extern "system" fn say_geri_cagri(
    hmon: HMONITOR,
    _hdc: HDC,
    _rc: *mut RECT,
    veri: LPARAM,
) -> BOOL {
    // SAFETY: veri, monitorleri_bul içindeki Vec<HMONITOR>'a işaret eder ve sayım sürerken canlıdır.
    let liste = unsafe { &mut *(veri.0 as *mut Vec<HMONITOR>) };
    liste.push(hmon);
    BOOL(1)
}

fn wide_den_string(w: &[u16]) -> String {
    let uzunluk = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..uzunluk])
}

pub(crate) fn monitorleri_bul() -> Vec<Monitor> {
    let mut tutamaclar: Vec<HMONITOR> = Vec::new();
    // SAFETY: geri çağrı yalnız LPARAM'daki Vec'e yazar; Vec çağrı boyunca canlı.
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(say_geri_cagri),
            LPARAM(&mut tutamaclar as *mut Vec<HMONITOR> as isize),
        );
    }

    let mut sonuc = Vec::with_capacity(tutamaclar.len());
    for hmon in tutamaclar {
        let mut bilgi = MONITORINFOEXW::default();
        bilgi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        // SAFETY: MONITORINFOEXW, MONITORINFO ile başlar; cbSize genişletilmiş boyutu bildirir.
        let tamam = unsafe {
            GetMonitorInfoW(hmon, &mut bilgi as *mut MONITORINFOEXW as *mut MONITORINFO)
        };
        if !tamam.as_bool() {
            continue;
        }
        let rc = bilgi.monitorInfo.rcMonitor;
        let cl = bilgi.monitorInfo.rcWork;
        sonuc.push(Monitor {
            hmon,
            calisma: Dikdortgen { sol: cl.left, ust: cl.top, sag: cl.right, alt: cl.bottom },
            bilgi: EkranBilgisi {
                aygit_adi: wide_den_string(&bilgi.szDevice),
                ana: bilgi.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
                sol: rc.left,
                ust: rc.top,
                genislik: rc.right - rc.left,
                yukseklik: rc.bottom - rc.top,
            },
        });
    }
    sonuc
}

/// Ekran seçimini ekran listesindeki bir indekse çevirir (saf). Ekranlar `EkranBilgisi.ana`
/// ile birincili bildirir; merkez x'i `sol + genislik/2`dir.
///
/// - `Ana`: birincil ekran.
/// - `Sol`/`Sag`: merkezi en solda/en sağda olan (eşitlikte listede önce gelen).
/// - `Ikinci`: ana dışındakilerin soldan ilki.
///
/// Tek ekranda ana dışı her seçim ("ikinci ekran yok") hata verir.
pub fn ekran_sec(ekranlar: &[EkranBilgisi], secim: crate::EkranSecimi) -> Result<usize, String> {
    use crate::EkranSecimi::*;
    let merkez = |e: &EkranBilgisi| e.sol as i64 * 2 + e.genislik as i64;
    let ana = ekranlar.iter().position(|e| e.ana);
    if secim == Ana {
        return ana.ok_or_else(|| "ana monitör bulunamadı".to_string());
    }
    if ekranlar.len() < 2 {
        return Err("ikinci ekran yok".to_string());
    }
    let mut sirali: Vec<usize> = (0..ekranlar.len()).collect();
    sirali.sort_by_key(|&i| (merkez(&ekranlar[i]), i));
    match secim {
        Sol => Ok(sirali[0]),
        Sag => {
            // Eşit merkezlerde listede önce gelen kazansın.
            let en = merkez(&ekranlar[sirali[sirali.len() - 1]]);
            Ok(*sirali.iter().find(|&&i| merkez(&ekranlar[i]) == en).unwrap_or(&sirali[0]))
        }
        Ikinci => sirali
            .into_iter()
            .find(|&i| Some(i) != ana)
            .ok_or_else(|| "ikinci ekran yok".to_string()),
        Ana => unreachable!(),
    }
}

/// Bağlı tüm ekranlar (salt-okuma).
pub fn ekranlari_listele() -> Vec<EkranBilgisi> {
    monitorleri_bul().into_iter().map(|m| m.bilgi).collect()
}

// ---------------------------------------------------------------------------
// DDC/CI
// ---------------------------------------------------------------------------

/// Bir HMONITOR'a bağlı fiziksel monitör tutamaçları; düşerken `DestroyPhysicalMonitors`.
pub(crate) struct Fiziksel(Vec<PHYSICAL_MONITOR>);

impl Fiziksel {
    pub(crate) fn al(hmon: HMONITOR) -> Result<Self, String> {
        let mut sayi = 0u32;
        // SAFETY: sayi geçerli bir çıktı işaretçisidir.
        unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut sayi) }
            .map_err(|e| format!("fiziksel monitör sayısı alınamadı: {e}"))?;
        if sayi == 0 {
            return Err("fiziksel monitör bulunamadı".to_string());
        }
        let mut dizi = vec![PHYSICAL_MONITOR::default(); sayi as usize];
        // SAFETY: dizi tam `sayi` eleman uzunluğunda ve yazılabilir.
        unsafe { GetPhysicalMonitorsFromHMONITOR(hmon, &mut dizi) }
            .map_err(|e| format!("fiziksel monitör tutamacı alınamadı: {e}"))?;
        Ok(Fiziksel(dizi))
    }

    pub(crate) fn tutamaclar(&self) -> Vec<HANDLE> {
        // Alan packed(1); değer olarak kopyalanır, başvuru alınmaz.
        self.0.iter().map(|p| p.hPhysicalMonitor).collect()
    }
}

impl Drop for Fiziksel {
    fn drop(&mut self) {
        // SAFETY: dizi, GetPhysicalMonitorsFromHMONITOR ile doldurulmuş tutamaçları içerir.
        unsafe {
            let _ = DestroyPhysicalMonitors(&self.0);
        }
    }
}

/// Rastgele bir VCP kodunun şimdiki değerini okur. KİLİTSİZDİR: çağıran `ddc_kilidi()` tutmalı.
pub(crate) fn vcp_oku(h: HANDLE, kod: u8) -> Result<u32, String> {
    let mut simdiki = 0u32;
    let mut en_buyuk = 0u32;
    // SAFETY: h geçerli fiziksel monitör tutamacı; iki çıktı işaretçisi de geçerli.
    let r = unsafe { GetVCPFeatureAndVCPFeatureReply(h, kod, None, &mut simdiki, Some(&mut en_buyuk)) };
    if r != 0 {
        Ok(simdiki)
    } else {
        Err(format!("DDC/CI VCP 0x{kod:02X} okunamadı"))
    }
}

/// Rastgele bir VCP koduna değer yazar. KİLİTSİZDİR: çağıran `ddc_kilidi()` tutmalı.
pub(crate) fn vcp_yaz(h: HANDLE, kod: u8, deger: u32) -> Result<(), String> {
    // SAFETY: h geçerli fiziksel monitör tutamacı.
    let r = unsafe { SetVCPFeature(h, kod, deger) };
    if r != 0 {
        Ok(())
    } else {
        Err(format!("DDC/CI VCP 0x{kod:02X} yazılamadı"))
    }
}

/// VCP 0xD6 değerini okur: (şimdiki, en büyük).
fn guc_oku(h: HANDLE) -> Result<u32, String> {
    let mut simdiki = 0u32;
    let mut en_buyuk = 0u32;
    // SAFETY: h geçerli fiziksel monitör tutamacı; iki çıktı işaretçisi de geçerli.
    let r = unsafe {
        GetVCPFeatureAndVCPFeatureReply(h, VCP_GUC, None, &mut simdiki, Some(&mut en_buyuk))
    };
    if r != 0 {
        Ok(simdiki)
    } else {
        Err("DDC/CI güç kipi okunamadı".to_string())
    }
}

fn guc_yaz(h: HANDLE, deger: u32) -> Result<(), String> {
    // SAFETY: h geçerli fiziksel monitör tutamacı.
    let r = unsafe { SetVCPFeature(h, VCP_GUC, deger) };
    if r != 0 {
        Ok(())
    } else {
        Err("DDC/CI güç kipi yazılamadı".to_string())
    }
}

pub(crate) fn hmonitor_bul(aygit_adi: &str) -> Result<HMONITOR, String> {
    monitorleri_bul()
        .into_iter()
        .find(|m| m.bilgi.aygit_adi == aygit_adi)
        .map(|m| m.hmon)
        .ok_or_else(|| format!("{aygit_adi} bulunamadı"))
}

/// DDC/CI güç kipi (VCP 0xD6) OKUNABİLİYOR mu ve şu anki değeri. Yalnız okur, yazmaz.
pub fn ddc_guc_durumu(aygit_adi: &str) -> Result<u32, String> {
    let _kilit = ddc_kilidi();
    let hmon = hmonitor_bul(aygit_adi)?;
    let fiziksel = Fiziksel::al(hmon)?;
    let mut son_hata = "DDC/CI güç kipi okunamadı".to_string();
    for h in fiziksel.tutamaclar() {
        match guc_oku(h) {
            Ok(d) => return Ok(d),
            Err(e) => son_hata = e,
        }
    }
    Err(son_hata)
}

/// `ddc_kapat` sonucu. `yazildi`, monitöre güç-kapat komutunun EN AZ BİR kez başarıyla
/// yazıldığını söyler: doğrulama sonucundan bağımsız olarak monitör artık kapanmış
/// olabilir ve çağıran onu kayda almış olmalıdır.
pub(crate) struct DdcKapatSonucu {
    pub yazildi: bool,
    pub sonuc: Result<(), String>,
    /// Tüm aday değerler yazıldı ve monitör hepsinden sonra "açık" yanıtı verdi: bu monitör
    /// DDC güç komutunu gerçekten yok sayıyor (perde öğrenilebilir).
    pub hepsi_yok_sayildi: bool,
    /// Başarılı kapatmada kullanılan değer (5 ise monitör DDC ile geri açılamayabilir).
    pub kullanilan: Option<u32>,
}

/// Geri okuma deneme sayısı ve aralığı. Monitör komutu hemen işlemeyebilir; "hâlâ 1"
/// okunursa beklemeye devam edilir. Toplam bekleme ~1,2 sn (UI iş parçacığını bloklar:
/// Faz 1 için kabul edildi).
const DOGRULAMA_DENEME: u32 = 3;
const DOGRULAMA_ARALIK: Duration = Duration::from_millis(400);

/// Monitörü DDC/CI ile kapatır: `adaylar` (bkz. `kapatma_adaylari`) sırayla yazılır, monitör
/// "açık" yanıtı vermeyi sürdürürse bir sonraki denenir. 5 (kapalı) bazı monitörlerde DDC'yi
/// tümden keser ve monitör ancak kendi güç düğmesiyle açılır; bu yüzden 4 destekleniyorsa
/// önce o denenir ve 5 kullanıldıysa çağıran kullanıcıyı uyarır.
///
/// `kayit_oncesi`, ilk yazmadan ÖNCE tam bir kez çağrılır (kalıcı kurtarma kaydı).
/// `Err` dönerse HİÇBİR ŞEY yazılmaz ve DDC başarısız sayılır.
///
/// KARAR (geri okuma yanıtsız ise): BAŞARI sayılır.
/// Gerekçe: yazmadan ÖNCE değer okunabildi (monitör DDC konuşuyor) ve SetVCPFeature
/// başarılı döndü (komut monitöre iletildi). Güç kipi 4/5'e geçen birçok monitör, bekleme
/// durumunda DDC'ye yanıt vermeyi keser; yanıtsızlık tam da beklenen sonuçtur. Bunu
/// "başarısız" sayıp perde açmak, kapanmış monitörün üstüne gereksiz bir perde bindirir.
/// Başarısızlık yalnız şu durumlarda: yazma öncesi okuma yok, yazma reddedildi, ya da
/// 3 geri okumanın hepsi yanıt verip güç "açık"(1) dedi. Son durumda bile `yazildi` true
/// kalır: monitör birkaç saniye sonra yine de kapanabilir.
pub(crate) fn ddc_kapat(
    hmon: HMONITOR,
    adaylar: &[u32],
    kayit_oncesi: &mut dyn FnMut() -> Result<(), String>,
) -> DdcKapatSonucu {
    // Kilit tüm yazma + doğrulama süresince (~1,2 sn) tutulur; `kayit_oncesi` DDC'ye dokunmaz.
    let _kilit = ddc_kilidi();
    let basarisiz = |yazildi, e: String| DdcKapatSonucu { yazildi, sonuc: Err(e), hepsi_yok_sayildi: false, kullanilan: None };
    if adaylar.is_empty() {
        return basarisiz(false, "monitör DDC/CI ile kapatmayı desteklemiyor (yetenek listesinde 4 ya da 5 yok)".into());
    }
    let fiziksel = match Fiziksel::al(hmon) {
        Ok(f) => f,
        Err(e) => return basarisiz(false, e),
    };
    let mut son_hata = "DDC/CI ile konuşulamadı".to_string();
    let mut kullanilan = None;
    let mut yazildi = false;
    let mut kayit_yapildi = false;
    let mut hepsi_yok_sayildi = false;

    for h in fiziksel.tutamaclar() {
        // Okunamıyorsa DDC yok say: yazma sessizce yutulabilir.
        if let Err(e) = guc_oku(h) {
            son_hata = e;
            continue;
        }
        // Kurtarma güvencesi olmadan monitör kapatılmaz.
        if !kayit_yapildi {
            if let Err(e) = kayit_oncesi() {
                return basarisiz(yazildi, e);
            }
            kayit_yapildi = true;
        }
        let mut yok_sayilan = 0usize;
        for &deger in adaylar {
            if let Err(e) = guc_yaz(h, deger) {
                son_hata = e;
                continue;
            }
            yazildi = true;
            if kapandi_mi(h) {
                kullanilan = Some(deger);
                break;
            }
            yok_sayilan += 1;
            son_hata = format!("monitör DDC/CI güç değeri {deger}'i yok saydı (hâlâ açık)");
        }
        if kullanilan.is_some() {
            break;
        }
        if yok_sayilan == adaylar.len() {
            hepsi_yok_sayildi = true;
        }
    }

    DdcKapatSonucu {
        yazildi,
        sonuc: if kullanilan.is_some() { Ok(()) } else { Err(son_hata) },
        hepsi_yok_sayildi: kullanilan.is_none() && hepsi_yok_sayildi,
        kullanilan,
    }
}

/// Yazmadan sonra geri okuyarak kapanıp kapanmadığına bakar (bkz. yanıtsızlık kararı).
fn kapandi_mi(h: HANDLE) -> bool {
    for _ in 0..DOGRULAMA_DENEME {
        sleep(DOGRULAMA_ARALIK);
        match guc_oku(h) {
            Ok(GUC_ACIK) => continue,
            // Değer değişti ya da yanıtsız: başarı.
            Ok(_) | Err(_) => return true,
        }
    }
    false
}

/// Monitöre güç açık (VCP 0xD6 = 1) yazar; yalnız yazma sonucuna bakar.
/// En az bir fiziksel monitöre yazılabildiyse Ok.
pub(crate) fn ddc_ac_yaz(hmon: HMONITOR) -> Result<(), String> {
    let _kilit = ddc_kilidi();
    let fiziksel = Fiziksel::al(hmon)?;
    let mut son_hata = "DDC/CI ile konuşulamadı".to_string();
    let mut basarili = false;
    for h in fiziksel.tutamaclar() {
        match guc_yaz(h, GUC_ACIK) {
            Ok(()) => basarili = true,
            Err(e) => son_hata = e,
        }
    }
    if basarili {
        Ok(())
    } else {
        Err(son_hata)
    }
}

/// Açma doğrulaması: D6 okunur, 1 ise Ok. Yanıtsız ya da başka değer: Err
/// (açık monitör yanıt verir; doğrulanamayan monitör açılmış sayılmaz).
pub(crate) fn ddc_acik_dogrula(hmon: HMONITOR) -> Result<(), String> {
    let _kilit = ddc_kilidi();
    let fiziksel = Fiziksel::al(hmon)?;
    let mut son_hata = "DDC/CI güç kipi okunamadı".to_string();
    for h in fiziksel.tutamaclar() {
        match guc_oku(h) {
            Ok(GUC_ACIK) => return Ok(()),
            Ok(d) => son_hata = format!("güç kipi hâlâ {d}"),
            Err(e) => son_hata = e,
        }
    }
    Err(son_hata)
}

/// Açma yazısından sonra doğrulama okumasına kadar beklenen süre.
pub(crate) const ACMA_SONRASI_BEKLEME: Duration = Duration::from_millis(400);
/// Açılışın doğrulanması için deneme sayısı: 5 (kapalı) kipinden dönen monitör birkaç saniye
/// DDC'ye yanıt vermeyebilir.
const ACMA_DOGRULAMA_DENEME: u32 = 8;

/// Güç açık yazıldıktan sonra monitör "açık" diyene kadar en çok ~3,2 sn bekler.
pub(crate) fn acildi_mi_bekle(hmon: HMONITOR) -> Result<(), String> {
    let mut son = Err("DDC/CI güç kipi okunamadı".to_string());
    for _ in 0..ACMA_DOGRULAMA_DENEME {
        sleep(ACMA_SONRASI_BEKLEME);
        son = ddc_acik_dogrula(hmon);
        if son.is_ok() {
            return son;
        }
    }
    son
}


#[cfg(test)]
mod testler {
    use super::*;

    // Kullanıcının monitörlerinden okunan gerçek yetenek dizesi biçimleri (kısaltılmış).
    const ASUS_XG27: &str = "(prot(monitor)type(LCD)model(XG27UCDMG)cmds(01 02 03 07 0C F3)vcp(02 04 05 08 10 12 14(05 06 08 0B) 16 18 1A 60(0F 11 12) 62 6C 6E 70 8D(01 02) AC AE B6 C6 C8 C9 CC(01 02 03 04 05 06 07 08 09 0A 0C 0D 11 12 14 1A 1E 1F 20) D6(01 05) DF E1(00 01))mccs_ver(2.2))";
    const ASUS_VG24: &str = "(prot(monitor)type(LCD)model(VG24VQ)vcp(02 04 05 08 10 12 14(05 06 08 0B) 16 18 1A 60(01 03 11 0F) 62 6C 6E 70 8D(01 02) AC AE B6 C6 C8 C9 CC(01 02 03) DF)mccs_ver(2.2))";

    #[test]
    fn vcp_degerleri_gercek_dizeler() {
        assert_eq!(vcp_degerleri(ASUS_XG27, 0xD6), Some(vec![1, 5]));
        assert_eq!(vcp_degerleri(ASUS_XG27, 0xE1), Some(vec![0, 1]));
        assert_eq!(vcp_degerleri(ASUS_XG27, 0x14), Some(vec![5, 6, 8, 0x0B]));
        // Değersiz geçen kod ve hiç olmayan kod.
        assert_eq!(vcp_degerleri(ASUS_XG27, 0x10), None);
        assert_eq!(vcp_degerleri(ASUS_VG24, 0xD6), None);
        // Değer listesi içindeki "D6" benzeri belirteç kod sayılmaz.
        assert_eq!(vcp_degerleri("vcp(14(D6 01) 10)", 0xD6), None);
        // Küçük harf, fazladan boşluk, vcp yok.
        assert_eq!(vcp_degerleri("vcp( 02 d6( 01  04 05 ) )", 0xD6), Some(vec![1, 4, 5]));
        assert_eq!(vcp_degerleri("model(X)", 0xD6), None);
        assert_eq!(vcp_degerleri("", 0xD6), None);
        // Kapanmamış parantez panik etmez.
        assert_eq!(vcp_degerleri("vcp(D6(01 05", 0xD6), None);
    }

    #[test]
    fn kapatma_adaylari_yetenege_gore() {
        // Kullanıcının ana monitörü: yalnız 5 destekleniyor (eskiden yazılan 4 yok sayılıyordu).
        assert_eq!(kapatma_adaylari(Some(ASUS_XG27)), vec![5]);
        // 4 de destekleniyorsa önce 4 (DDC canlı kalır).
        assert_eq!(kapatma_adaylari(Some("vcp(D6(01 04 05))")), vec![4, 5]);
        assert_eq!(kapatma_adaylari(Some("vcp(D6(01 04))")), vec![4]);
        // D6 listeleniyor ama kapatma değeri yok: DDC ile kapatılamaz.
        assert_eq!(kapatma_adaylari(Some("vcp(D6(01))")), Vec::<u32>::new());
        // Dize yok ya da D6 değer listesiz/yok: önce yumuşak, sonra sert dene.
        assert_eq!(kapatma_adaylari(None), vec![4, 5]);
        assert_eq!(kapatma_adaylari(Some(ASUS_VG24)), vec![4, 5]);
    }

    #[test]
    fn en_az_bir_ekran_ve_tek_ana() {
        let ekranlar = ekranlari_listele();
        assert!(!ekranlar.is_empty(), "hiç ekran dönmedi");
        assert_eq!(ekranlar.iter().filter(|e| e.ana).count(), 1, "tam bir ana ekran olmalı");
        for e in &ekranlar {
            assert!(e.genislik > 0 && e.yukseklik > 0);
            assert!(e.aygit_adi.starts_with("\\\\.\\"));
        }
    }

    fn ek(ad: &str, ana: bool, sol: i32, gen: i32) -> EkranBilgisi {
        EkranBilgisi { aygit_adi: ad.to_string(), ana, sol, ust: 0, genislik: gen, yukseklik: 1080 }
    }

    use crate::EkranSecimi::*;

    #[test]
    fn uc_ekran_sol_ana_sag() {
        // Sıra karışık: 0 = ana (ortada), 1 = sağda, 2 = solda.
        let e = [ek("A", true, 0, 1920), ek("B", false, 1920, 1920), ek("C", false, -1920, 1920)];
        assert_eq!(ekran_sec(&e, Ana), Ok(0));
        assert_eq!(ekran_sec(&e, Sol), Ok(2));
        assert_eq!(ekran_sec(&e, Sag), Ok(1));
        // Ana dışındakilerin soldan ilki.
        assert_eq!(ekran_sec(&e, Ikinci), Ok(2));
    }

    #[test]
    fn uc_ekran_ana_solda() {
        let e = [ek("A", true, 0, 1920), ek("B", false, 1920, 1920), ek("C", false, 3840, 1920)];
        assert_eq!(ekran_sec(&e, Sol), Ok(0));
        assert_eq!(ekran_sec(&e, Sag), Ok(2));
        assert_eq!(ekran_sec(&e, Ikinci), Ok(1));
    }

    #[test]
    fn iki_ekran() {
        let e = [ek("A", true, 0, 1920), ek("B", false, 1920, 2560)];
        assert_eq!(ekran_sec(&e, Ana), Ok(0));
        assert_eq!(ekran_sec(&e, Ikinci), Ok(1));
        assert_eq!(ekran_sec(&e, Sol), Ok(0));
        assert_eq!(ekran_sec(&e, Sag), Ok(1));
        // Ana sağdaysa ikinci soldadır.
        let e = [ek("A", true, 1920, 1920), ek("B", false, 0, 1920)];
        assert_eq!(ekran_sec(&e, Ikinci), Ok(1));
        assert_eq!(ekran_sec(&e, Sol), Ok(1));
    }

    #[test]
    fn tek_ekranda_ana_disi_hata() {
        let e = [ek("A", true, 0, 1920)];
        assert_eq!(ekran_sec(&e, Ana), Ok(0));
        for s in [Ikinci, Sol, Sag] {
            let h = ekran_sec(&e, s).unwrap_err();
            assert!(h.contains("ikinci ekran yok"), "{h}");
        }
    }

    #[test]
    fn bos_liste_ve_anasiz() {
        assert!(ekran_sec(&[], Ana).is_err());
        assert!(ekran_sec(&[], Sol).is_err());
        // Ana bildirilmemişse Ikinci yine soldan ilk.
        let e = [ek("A", false, 0, 1920), ek("B", false, 1920, 1920)];
        assert!(ekran_sec(&e, Ana).is_err());
        assert_eq!(ekran_sec(&e, Ikinci), Ok(0));
    }

    #[test]
    fn farkli_genislikte_merkez_hesabi() {
        // 0: [0,1000) merkez 500; 1: [100,1500) merkez 800 -> Sol=0, Sag=1.
        let e = [ek("A", true, 0, 1000), ek("B", false, 100, 1400)];
        assert_eq!(ekran_sec(&e, Sol), Ok(0));
        assert_eq!(ekran_sec(&e, Sag), Ok(1));
    }
}
