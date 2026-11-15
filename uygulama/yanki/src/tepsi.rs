//! UI iş parçacığı: gizli pencere, tepsi simgesi, menü ve Win32 mesaj döngüsü.
//! Eylemler (`Sistem`), efekt motoru (`sahne::Sahne`), sahne yürütme, geri al yığını ve Stüdyo
//! IPC alıcısı yalnız burada, bu iş parçacığında çalışır.
//!
//! Komut akışı: komut gelir → efekti oynatılır → eylem çalışır. Ekranı kapatan / oturumu kilitleyen /
//! uyutan eylemler (`efekt_sonrasi_mi`) efekt BİTİNCE çalışır: motorun `bitince` geri çağrısı bu
//! pencereye `WM_EFEKT_BITTI` atar; o an duraklatılmış ya da kapanıyorsak eylem çalışmaz.
//! Eylem efekte bağımlı değildir: grafik başarısızsa `bitince` hemen gelir. Efektler ayarda
//! kapalıysa (`efekt_oynasin_mi`) efekt oynatılmaz ve bu eylemler de beklemeden hemen çalışır.
//!
//! Güncelleme: `guncelleme` modülü ayrı iş parçacığında denetler/indirir; sonuç `WM_GUNCELLEME` ile
//! gelir. Kurulum yalnız kullanıcı onayıyla: menü, balon ve Stüdyo isteği aynı evet/hayır kutusundan geçer.
//!
//! Kısayollar (`RegisterHotKey` → `WM_HOTKEY`), bas-konuş tuş penceresi, zamanlı komutlar
//! (`SetTimer`) ve çift alkış da komutu aynı yoldan (`komutu_baslat`) çalıştırır: bekleyen
//! efekt-sonrası eylem kuralları, efektlerin kapalı olması, kuru kip ve geri al kaydı hepsine
//! uygulanır. Kısayol ve zamanlayıcı dinleme duraklatılmışken de çalışır (bkz. `KomutKaynagi`).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use komut::efekt::{Efekt, EfektAyari, Olay};
use komut::{Ayarlar, Komut};
use sahne::{Hedef, Istek, Sahne};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_ERROR, NIIF_INFO,
    NIM_ADD, NIM_DELETE, NIM_MODIFY, NIN_BALLOONHIDE, NIN_BALLOONTIMEOUT, NIN_BALLOONUSERCLICK, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, GetWindowLongPtrW, KillTimer, LoadIconW,
    LoadImageW, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    SetForegroundWindow, SetTimer, SetWindowLongPtrW, TrackPopupMenu, TranslateMessage, GWLP_USERDATA, HICON,
    IDI_APPLICATION, IDI_ERROR, IDI_INFORMATION, IDI_WARNING, IDYES, IMAGE_ICON, LR_DEFAULTCOLOR, MB_ICONQUESTION,
    MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO, MENU_ITEM_FLAGS, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR,
    MF_STRING, MSG, SM_CXSMICON, SM_CYSMICON, SW_SHOWNORMAL, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE,
    WM_APP, WM_COPYDATA, WM_DESTROY, WM_ENDSESSION, WM_LBUTTONUP, WM_NULL, WM_QUERYENDSESSION, WM_RBUTTONUP,
    WM_TIMER, WNDCLASSW, WS_OVERLAPPED, EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PBT_APMRESUMEAUTOMATIC,
    WM_CLOSE, WM_HOTKEY, WM_POWERBROADCAST,
};

use crate::dagitim::{
    bekleyen_karari, dagit, eylem_ozeti, komut_karari, BekleyenKarari, Etki, GeriAlSonucu, GeriAlYigini, GeriKaydi,
    Kayit, KomutKaynagi, KomutKarari, PencereImzaci, Sonuc, Ters,
};
use crate::kisayollar::{self, KisayolHedefi, KisayolKayitlari};
use crate::tus_penceresi::{BasisKarari, TusPenceresi};
use crate::zamanlayici::{self, DolumKarari, EklemeHatasi, Zamanlayici, ZamanlayiciListesi};
use crate::dinleyici::{DinleyiciKolu, Durum, UiIleti, UiKuyrugu, WM_DINLEYICI};
use crate::guncelleme::{self, DenetimSonucu, KurulumKarari, Surum};
use crate::gunluk;
use crate::ipc::{self, IpcIstegi};
use crate::sahne_akisi::{SahneYurutucu, SiradakiAdim};
use crate::sistem::SistemKutusu;

/// Tepsi simgesinden gelen fare iletisi.
const WM_TEPSI: u32 = WM_APP + 1;
/// Efekt motorundan: efekt bitti; wParam bekleyen eylemin jetonu (0 = bekleyen yok).
const WM_EFEKT_BITTI: u32 = WM_APP + 3;
/// Sahnenin sıradaki adımı; wParam sahne nesli (bayat iletiler atılır).
const WM_SAHNE_ADIMI: u32 = WM_APP + 4;
/// Ayar dosyası değişti (wParam 0: izleyici) ya da Stüdyo yeniden yükle istedi (wParam 1).
const WM_AYAR_DEGISTI: u32 = WM_APP + 5;
/// Stüdyo'dan gelen komut(lar) kuyrukta.
const WM_IPC_KOMUT: u32 = WM_APP + 6;
/// Güncelleme denetimi bitti (sonuç `Uygulama::denetim_sonucu`nda).
const WM_GUNCELLEME: u32 = WM_APP + 7;
/// Onaylanmış güncellemeyi kur (Stüdyo IPC'si; `WM_COPYDATA` içinde pencere yok edilmesin diye ertelenir).
const WM_GUNCELLEME_KUR: u32 = WM_APP + 8;

const SIMGE_NO: u32 = 1;
const ZAMANLAYICI_SURE: usize = 1;
/// Tepsi simgesi eklenemediyse yeniden deneme zamanlayicisi.
const SIMGE_ZAMANLAYICI: usize = 2;
/// Sahnenin bekleme adımı.
const SAHNE_ZAMANLAYICI: usize = 3;
/// Bekleyen efekt-sonrası eylemin süre dolumu (`bitince` hiç gelmezse).
const BEKLEYEN_ZAMANLAYICI: usize = 4;
/// Güncelleme denetimi: açılıştan 60 sn sonra, sonra 24 saatte bir.
const GUNCELLEME_ZAMANLAYICI: usize = 5;
const GUNCELLEME_ILK_MS: u32 = 60_000;
/// Sonraki tetiklenişler saatlik; denetim kararı `son_denetim`e göre (24 saat).
const GUNCELLEME_SAATLIK_MS: u32 = 60 * 60 * 1000;
/// Zamanlı komutların ipucu/uyarı tıkı (yalnız zamanlayıcı varken kurulur).
const ZAMANLAYICI_TIK: usize = 6;
/// Bas-konuş tuş penceresinin süre dolumu.
const TUS_PENCERESI_ZAMANLAYICI: usize = 7;
/// Zamanlı komutların dolum zamanlayıcıları: `DOLUM_TABAN + kimlik % DOLUM_ARALIK`.
const DOLUM_TABAN: usize = 0x1000;
const DOLUM_ARALIK: usize = 0x1000;
/// Efekt süresine eklenen pay: bu süre içinde `bitince` gelmezse bekleyen atılır.
const BEKLEYEN_PAYI: Duration = Duration::from_secs(3);
/// Pencere sınıfları. Stüdyo gerçek örneği `Echora.Tepsi` ile, kuru örneği `Echora.Tepsi.Kuru` ile bulur
/// (kuru örnekten açılan Stüdyo'ya `ECHORA_KURU=1` geçirilir): kuru sınama gerçek örneğe ulaşmaz.
const SINIF_GERCEK: PCWSTR = w!("Echora.Tepsi");
const SINIF_KURU: PCWSTR = w!("Echora.Tepsi.Kuru");
const SIMGE_DENEME_ARALIGI_MS: u32 = 2000;
const SIMGE_EN_COK_DENEME: u32 = 5;

const KOMUT_DURAKLAT_SURDUR: usize = 1;
const KOMUT_AYAR_AC: usize = 2;
const KOMUT_YENIDEN_YUKLE: usize = 3;
const KOMUT_GUNLUK_AC: usize = 4;
const KOMUT_CIKIS: usize = 5;
const KOMUT_STUDYO: usize = 6;
const KOMUT_EFEKTLER: usize = 7;
const KOMUT_IPUCU_EFEKTLERI: usize = 8;
const KOMUT_GUNCELLEME_DENETLE: usize = 9;
const KOMUT_GUNCELLEME_KUR: usize = 10;
const KOMUT_ZAMANLAYICI_HEPSI: usize = 11;
const KOMUT_PALET: usize = 12;
/// "Zamanlayıcılar" alt menüsündeki i. öğe: `KOMUT_ZAMANLAYICI_TABAN + i` (en çok `MENU_ZAMANLAYICI`).
const KOMUT_ZAMANLAYICI_TABAN: usize = 100;
const MENU_ZAMANLAYICI: usize = 20;

/// Tepsi simgelerinin kaynak kimlikleri (`build.rs` `SIMGELER` ile aynı).
const SIMGE_NORMAL: u16 = 101;
const SIMGE_DURAKLAT: u16 = 102;
const SIMGE_HATA: u16 = 103;

/// "TaskbarCreated" iletisinin numarası (Explorer yeniden başlayınca simge yeniden eklenir).
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// Efekti bitince çalışacak eylem.
pub struct Bekleyen {
    jeton: u64,
    komut: Komut,
    /// Bu andan sonra gelen "efekt bitti" bayattır (UI uzun süre meşgul kaldı).
    son_tarih: Instant,
    /// Komutun kaynağı: Stüdyo/kısayol/zamanlayıcıdan geldiyse dinlemenin duraklatılmış olması onu
    /// durdurmaz (bkz. `KomutKaynagi::duraklatmayi_asar`).
    kaynak: KomutKaynagi,
}

/// Ayarların yeniden yüklenme nedeni.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kaynak {
    /// Menü ya da sesli komut: başarıda da balon gösterilir.
    Kullanici,
    /// Dosya izleyici: içerik özeti aynıysa hiçbir şey yapılmaz; başarıda balon yok.
    Dosya,
    /// Stüdyo isteği: her zaman yüklenir; başarıda balon yok.
    Studyo,
    /// Tepsi menüsündeki bir anahtar dosyayı yazdı: her zaman yüklenir; başarıda balon yok.
    Tepsi,
}

/// Pencerenin `GWLP_USERDATA`'sında tutulan UI durumu. Sahibi `main`'dir.
pub struct Uygulama {
    /// Cikista ilk once dusurulur (perdeleri kaldirir, DDC ile kapananlari acar): `None` yapilir.
    pub sistem: Option<SistemKutusu>,
    pub kol: Option<DinleyiciKolu>,
    pub ui: Option<UiKuyrugu>,
    pub durum: Durum,
    pub ayar_yolu: PathBuf,
    /// `--ayar` ile verilen yol; Stüdyo aynısıyla başlatılır.
    pub ayar_arg: Option<PathBuf>,
    pub kuru: bool,
    /// UI'nın ayar kopyası (efekt seçimi, sahneler, IPC eşlemesi). Dinleyici kendi kopyasını tutar.
    pub ayarlar: Ayarlar,
    /// Son yüklenen (ya da yüklenmeye çalışılan) ayar dosyası içeriğinin özeti.
    pub son_ozet: Option<u64>,
    /// Efekt motoru. Hiç efekt oynamazken iş parçacığı/aygıt yoktur.
    pub efekt: Sahne,
    efekt_hatasi_yazildi: bool,
    sayac: u64,
    bekleyen: Option<Bekleyen>,
    geri_al: GeriAlYigini,
    /// Geri al için pencere imzası (kuru kipte sabit).
    imzaci: Box<dyn PencereImzaci>,
    /// Bir ekranı DDC/CI ile kapattık; dinleyici tam ekran engelini bu sırada uygulamaz.
    pub ddc_kapali: Arc<AtomicBool>,
    sahne: Option<SahneYurutucu>,
    sahne_nesil: u64,
    sahne_bekliyor: bool,
    /// Çalışan sahne Stüdyo'dan başlatıldı: dinleme duraklatması onu durdurmaz.
    sahne_studyodan: bool,
    /// `WM_ENDSESSION`: süreç sonlanacak; çıkışta efekt/dinleyici beklenmez.
    pub oturum_bitiyor: bool,
    ipc_kuyrugu: VecDeque<Komut>,
    pub hwnd: HWND,
    pub simge_ekli: bool,
    /// `duraklat` cagrildigi AN ayarlanir; aciksa kuyruktan gelen komutlar atilir.
    pub duraklatildi: bool,
    /// Cikis basladi: kuyrukta kalan komutlar atilir.
    pub kapaniyor: bool,
    /// Simge ekleme yeniden denemesi sayaci.
    pub simge_deneme: u32,
    simgeler: Simgeler,
    /// `%LOCALAPPDATA%\Echora\guncelleme` (`--ayar` ile o klasörün `guncelleme` alt klasörü).
    guncelleme_klasoru: PathBuf,
    /// Aynı anda tek denetim.
    denetim_suruyor: Arc<AtomicBool>,
    /// İş parçacığının bıraktığı sonuç ve elle mi istendiği.
    denetim_sonucu: Arc<Mutex<Option<(DenetimSonucu, bool)>>>,
    /// Bu süreçte indirilmiş/doğrulanmış, kurulmayı bekleyen sürüm ve beklenen SHA-256 (bellekte).
    hazir: Option<guncelleme::HazirSurum>,
    /// İlk (açılış) denetim zamanlayıcısı tetiklendi mi.
    ilk_denetim_yapildi: bool,
    /// Son gösterilen balon "indirildi, kurmak için tıklayın" balonu mu (tıklanınca onay sorulur).
    guncelleme_balonu: bool,
    /// `RegisterHotKey` kayıtları.
    kisayollar: KisayolKayitlari,
    /// Zamanlı komutlar (kalıcı değil).
    zamanlayicilar: ZamanlayiciListesi,
    /// Bas-konuş tuş penceresi.
    tus_penceresi: TusPenceresi,
    /// Son kayıtta başka uygulamanın tuttuğu kısayollar (görünüm, sıralı); balon yalnız bu küme
    /// değişince gösterilir.
    mesgul_kisayollar: Vec<String>,
    /// Tuşla kipinde dinleme tuşu kaydedilemedi: dinleyici geçici olarak sürekli kipte.
    dinleme_tusu_yok: bool,
}

/// Tepsi simgeleri: exe'ye gömülü `.ico` kaynaklarından, küçük simge boyutunda (DPI'ye göre)
/// yüklenir. Kaynak yoksa (test ikilisi) stok simgelere düşülür.
struct Simgeler {
    normal: Option<HICON>,
    duraklat: Option<HICON>,
    hata: Option<HICON>,
}

impl Simgeler {
    fn yukle() -> Self {
        // SAFETY: salt-okuma sorguları; modül tutamacı süreç boyunca geçerli.
        let (cx, cy, ornek) = unsafe {
            (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON), GetModuleHandleW(None).ok().map(HINSTANCE::from))
        };
        let yukle = |kimlik: u16| -> Option<HICON> {
            // SAFETY: MAKEINTRESOURCE biçimi (kimlik işaretçi olarak); dönen simge Drop'ta yok edilir.
            let h = unsafe {
                LoadImageW(ornek, PCWSTR(kimlik as usize as *const u16), IMAGE_ICON, cx, cy, LR_DEFAULTCOLOR)
            }
            .ok()?;
            (!h.is_invalid()).then_some(HICON(h.0))
        };
        Simgeler { normal: yukle(SIMGE_NORMAL), duraklat: yukle(SIMGE_DURAKLAT), hata: yukle(SIMGE_HATA) }
    }
}

impl Drop for Simgeler {
    fn drop(&mut self) {
        for h in [self.normal, self.duraklat, self.hata].into_iter().flatten() {
            // SAFETY: LoadImageW ile (LR_SHARED olmadan) yüklenmiş simge.
            let _ = unsafe { DestroyIcon(h) };
        }
    }
}

impl Uygulama {
    pub fn new(
        mut sistem: SistemKutusu,
        ayarlar: Ayarlar,
        ayar_yolu: PathBuf,
        ayar_arg: Option<PathBuf>,
        kuru: bool,
        guncelleme_klasoru: PathBuf,
    ) -> Self {
        sistem.yontemleri_uygula(&[], &ayarlar.ekran_yontemleri);
        let imzaci = sistem.imzaci();
        Uygulama {
            simgeler: Simgeler::yukle(),
            guncelleme_klasoru,
            denetim_suruyor: Arc::new(AtomicBool::new(false)),
            denetim_sonucu: Arc::new(Mutex::new(None)),
            hazir: None,
            ilk_denetim_yapildi: false,
            guncelleme_balonu: false,
            kisayollar: KisayolKayitlari::default(),
            zamanlayicilar: ZamanlayiciListesi::default(),
            tus_penceresi: TusPenceresi::default(),
            mesgul_kisayollar: Vec::new(),
            dinleme_tusu_yok: false,
            imzaci,
            ddc_kapali: Arc::new(AtomicBool::new(false)),
            sahne_studyodan: false,
            oturum_bitiyor: false,
            sistem: Some(sistem),
            kol: None,
            ui: None,
            durum: Durum::Dinliyor,
            son_ozet: crate::izleyici::dosya_ozeti(&ayar_yolu),
            ayar_yolu,
            ayar_arg,
            kuru,
            ayarlar,
            efekt: Sahne::new(),
            efekt_hatasi_yazildi: false,
            sayac: 0,
            bekleyen: None,
            geri_al: GeriAlYigini::default(),
            sahne: None,
            sahne_nesil: 0,
            sahne_bekliyor: false,
            ipc_kuyrugu: VecDeque::new(),
            hwnd: HWND::default(),
            simge_ekli: false,
            duraklatildi: false,
            kapaniyor: false,
            simge_deneme: 0,
        }
    }
}

fn genis(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `dizi`'ye NUL'lu kopyalar, sığmayanı keser.
fn dizi_doldur(dizi: &mut [u16], metin: &str) {
    let mut i = 0;
    for u in metin.encode_utf16() {
        if i + 1 >= dizi.len() {
            break;
        }
        dizi[i] = u;
        i += 1;
    }
    // Kesim bir vekil ciftinin ortasina denk geldiyse (yuksek vekil yazildi, dusuk sigmadi) yarim
    // cift gecersiz UTF-16 olur; o birimi geri al.
    if i > 0 && (0xD800..=0xDBFF).contains(&dizi[i - 1]) {
        i -= 1;
    }
    dizi[i] = 0;
}

fn ipucu(d: Durum) -> &'static str {
    match d {
        Durum::Dinliyor => "Echora — dinliyor",
        Durum::Duraklatildi => "Echora — duraklatıldı",
        Durum::MikrofonYok => "Echora — mikrofon yok",
        Durum::ModelYok => "Echora — model bulunamadı",
        Durum::DinleyiciYok => "Echora — dinleyici başlatılamadı",
        Durum::SessizSaat => "Echora — sessiz saat",
        Durum::TamEkran => "Echora — tam ekran uygulama; dinleme bekliyor",
    }
}

/// Tepsi ipucu: durum satırı (tuş penceresi açıkken "dinliyor…", tuşla kipinde dinleme tuşu) ve
/// varsa en yakın zamanlayıcı satırı.
/// `tus_yok`: tuşla kipinde dinleme tuşu kaydedilemedi, sürekli dinleniyor.
fn ipucu_olustur(d: Durum, tusla: Option<&str>, tus_yok: bool, pencere_acik: bool, zamanlayici: Option<String>) -> String {
    let mut s = match (d, pencere_acik, tusla) {
        (Durum::Dinliyor | Durum::SessizSaat | Durum::TamEkran, true, _) => "Echora — dinliyor…".to_string(),
        (Durum::Dinliyor, false, _) if tus_yok => "Echora — dinleme tuşu kullanılamıyor; sürekli dinleniyor".to_string(),
        (Durum::Dinliyor, false, Some(t)) => format!("Echora — tuşla dinleme ({t})"),
        _ => ipucu(d).to_string(),
    };
    if let Some(z) = zamanlayici {
        s.push('\n');
        s.push_str(&z);
    }
    s
}

fn ipucu_metni(u: &Uygulama) -> String {
    let tusla = match (u.ayarlar.dinleme_kipi, &u.ayarlar.dinleme_tusu) {
        (komut::DinlemeKipi::Tusla, Some(k)) => Some(kisayollar::gorunum(&k.metin)),
        _ => None,
    };
    let zaman = u.zamanlayicilar.ipucu_satiri(Instant::now());
    ipucu_olustur(u.durum, tusla.as_deref(), u.dinleme_tusu_yok, u.tus_penceresi.acik().is_some(), zaman)
}

/// Dinleyiciye gönderilen ayarlar: dinleme tuşu kaydedilemediyse kip geçici olarak sürekli.
pub fn dinleyici_ayarlari(u: &Uygulama) -> Ayarlar {
    let mut a = u.ayarlar.clone();
    if u.dinleme_tusu_yok {
        a.dinleme_kipi = komut::DinlemeKipi::Surekli;
    }
    a
}

fn simge_yukle(u: &Uygulama) -> HICON {
    let d = u.durum;
    let gomulu = match d {
        Durum::Dinliyor => u.simgeler.normal,
        Durum::Duraklatildi | Durum::SessizSaat | Durum::TamEkran => u.simgeler.duraklat,
        Durum::MikrofonYok | Durum::ModelYok | Durum::DinleyiciYok => u.simgeler.hata,
    };
    if let Some(h) = gomulu {
        return h;
    }
    let kimlik = match d {
        Durum::Dinliyor => IDI_APPLICATION,
        Durum::Duraklatildi | Durum::SessizSaat | Durum::TamEkran => IDI_INFORMATION,
        Durum::MikrofonYok => IDI_WARNING,
        Durum::ModelYok | Durum::DinleyiciYok => IDI_ERROR,
    };
    // SAFETY: stok simgeler paylaşılır, serbest bırakılmaz.
    unsafe { LoadIconW(None, kimlik).unwrap_or_default() }
}

fn bildirim_verisi(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: SIMGE_NO,
        ..Default::default()
    }
}

fn simge_ekle(u: &mut Uygulama) {
    let mut v = bildirim_verisi(u.hwnd);
    v.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    v.uCallbackMessage = WM_TEPSI;
    v.hIcon = simge_yukle(u);
    dizi_doldur(&mut v.szTip, &ipucu_metni(u));
    // SAFETY: v tam ve geçerli doldurulmuş bir NOTIFYICONDATAW.
    u.simge_ekli = unsafe { Shell_NotifyIconW(NIM_ADD, &v).as_bool() };
    if u.simge_ekli {
        u.simge_deneme = 0;
        // SAFETY: pencere bu is parcacigina ait.
        let _ = unsafe { KillTimer(Some(u.hwnd), SIMGE_ZAMANLAYICI) };
    } else {
        gunluk::yaz("HATA: tepsi simgesi eklenemedi; yeniden denenecek");
        // SAFETY: pencere bu is parcacigina ait; ayni kimlikli zamanlayici yenilenir.
        let _ = unsafe { SetTimer(Some(u.hwnd), SIMGE_ZAMANLAYICI, SIMGE_DENEME_ARALIGI_MS, None) };
    }
}

/// Simge yeniden deneme zamanlayicisi: en cok `SIMGE_EN_COK_DENEME`; olmazsa temiz cikis. Simgesiz
/// surec mikrofonu dinlemeye devam eder ve kullanici cikamaz.
fn simge_yeniden_dene(hwnd: HWND) {
    let Some(u) = uyg(hwnd) else { return };
    if u.simge_ekli {
        // SAFETY: pencere bu is parcacigina ait.
        let _ = unsafe { KillTimer(Some(hwnd), SIMGE_ZAMANLAYICI) };
        return;
    }
    u.simge_deneme += 1;
    simge_ekle(u);
    if !u.simge_ekli && u.simge_deneme >= SIMGE_EN_COK_DENEME {
        gunluk::yaz("HATA: tepsi simgesi eklenemedi; temiz çıkılıyor");
        cikis_hazirla(u);
        // SAFETY: pencere bu is parcacigina ait.
        unsafe {
            let _ = KillTimer(Some(hwnd), SIMGE_ZAMANLAYICI);
            let _ = DestroyWindow(hwnd);
        }
    }
}

fn simge_guncelle(u: &Uygulama) {
    if !u.simge_ekli {
        return;
    }
    let mut v = bildirim_verisi(u.hwnd);
    v.uFlags = NIF_ICON | NIF_TIP;
    v.hIcon = simge_yukle(u);
    dizi_doldur(&mut v.szTip, &ipucu_metni(u));
    // SAFETY: yukarıdakiyle aynı.
    let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &v) };
}

fn simge_sil(u: &mut Uygulama) {
    if u.simge_ekli {
        let v = bildirim_verisi(u.hwnd);
        // SAFETY: yukarıdakiyle aynı.
        let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &v) };
        u.simge_ekli = false;
    }
}

/// Hata ya da bilgi balonu. Yalnız gerçek hata/uyarılar için; ortam sesiyle balon yağmamalı.
pub fn balon(u: &mut Uygulama, baslik: &str, metin: &str, hata: bool) {
    if !u.simge_ekli {
        return;
    }
    // Yeni balon öncekinin yerini alır: tıklama artık güncelleme balonuna ait değil.
    u.guncelleme_balonu = false;
    let mut v = bildirim_verisi(u.hwnd);
    v.uFlags = NIF_INFO;
    v.dwInfoFlags = if hata { NIIF_ERROR } else { NIIF_INFO };
    dizi_doldur(&mut v.szInfoTitle, baslik);
    dizi_doldur(&mut v.szInfo, metin);
    // SAFETY: yukarıdakiyle aynı.
    let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &v) };
}

/// Pencerenin durumunu döndürür. Dönen başvuru yalnız kısa süre tutulmalı: menü gibi iç içe
/// mesaj döngüleri aynı duruma yeniden ulaşabilir.
fn uyg<'a>(hwnd: HWND) -> Option<&'a mut Uygulama> {
    // SAFETY: GWLP_USERDATA yalnız `pencere_olustur`un verdiği, `main`in sahip olduğu ve pencere
    // yaşadığı sürece geçerli olan `Uygulama` işaretçisini (ya da 0'ı) taşır.
    let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Uygulama;
    if p.is_null() {
        None
    } else {
        Some(unsafe { &mut *p })
    }
}

fn durum_ayarla(u: &mut Uygulama, d: Durum) {
    if u.durum != d {
        u.durum = d;
        simge_guncelle(u);
    }
}

fn ileti_at(hwnd: HWND, ileti: u32, wp: usize) {
    // SAFETY: kendi penceremize; pencere yoksa hata döner ve yok sayılır.
    let _ = unsafe { PostMessageW(Some(hwnd), ileti, WPARAM(wp), LPARAM(0)) };
}

/// Dosyayı varsayılan uygulamayla açar; ilişkili uygulama yoksa Not Defteri'yle.
fn dosya_ac(yol: &std::path::Path) {
    let metin = genis(&yol.to_string_lossy());
    // SAFETY: metin NUL'lu ve çağrı boyunca yaşıyor.
    let sonuc = unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(metin.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL)
    };
    // ShellExecute başarıda 32'den büyük bir değer döndürür.
    if (sonuc.0 as isize) <= 32 {
        if let Err(h) = std::process::Command::new("notepad.exe").arg(yol).spawn() {
            gunluk::yaz(&format!("HATA: {} açılamadı: {h}", yol.display()));
        }
    }
}

fn menu_goster(hwnd: HWND) {
    let Some(u) = uyg(hwnd) else { return };
    let duraklat_metni = if u.durum.dinliyor_mu() && !u.duraklatildi {
        "Dinlemeyi duraklat"
    } else {
        "Dinlemeyi sürdür"
    };
    let efektler = u.ayarlar.efektler_acik;
    let ipucu_efektleri = u.ayarlar.olay_efektleri_acik;
    let denetleniyor = u.denetim_suruyor.load(Ordering::SeqCst);
    let hazir = u.hazir.map(|h| h.surum);
    let isaret = |acik: bool| if acik { MF_CHECKED } else { MENU_ITEM_FLAGS(0) };
    let simdi = Instant::now();
    // Menü açıkken zamanlayıcı dolabilir: seçim kimlikle eşlenir (yoksa iptal zararsızca atlanır).
    let zamanlar: Vec<(u64, Vec<u16>)> = u
        .zamanlayicilar
        .sirali()
        .iter()
        .take(MENU_ZAMANLAYICI)
        .map(|z| {
            let kalan = zamanlayici::kalan_metni(z.bitis.saturating_duration_since(simdi));
            (z.kimlik, genis(&format!("{} — {kalan} (iptal et)", z.gosterim)))
        })
        .collect();
    let palet_metni = match &u.ayarlar.palet_tusu {
        Some(k) => format!("Komut paleti\t{}", kisayollar::gorunum(&k.metin)),
        None => "Komut paleti".to_string(),
    };

    // SAFETY: menü bu işlevde oluşturulup yok edilir (alt menü ana menüyle birlikte); metinler
    // çağrı boyunca yaşar.
    let secim = unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let d = genis(duraklat_metni);
        let s = genis("Stüdyo'yu aç");
        let a = genis("Ayar dosyasını aç");
        let y = genis("Ayarları yeniden yükle");
        let g = genis("Günlüğü aç");
        let c = genis("Çıkış");
        let ef = genis("Efektler");
        let ef1 = genis("Efektler açık");
        let ef2 = genis("İpucu efektleri açık");
        let gd = genis(if denetleniyor { "Güncellemeler denetleniyor…" } else { "Güncellemeleri denetle" });
        let gk = genis(&hazir.map(|s| format!("Güncellemeyi kur (v{s})")).unwrap_or_default());
        let zm = genis("Zamanlayıcılar");
        let zh = genis("Tümünü iptal et");
        let pl = genis(&palet_metni);
        let _ = AppendMenuW(menu, MF_STRING, KOMUT_DURAKLAT_SURDUR, PCWSTR(d.as_ptr()));
        if !zamanlar.is_empty() {
            if let Ok(alt) = CreatePopupMenu() {
                for (i, (_, metin)) in zamanlar.iter().enumerate() {
                    let _ = AppendMenuW(alt, MF_STRING, KOMUT_ZAMANLAYICI_TABAN + i, PCWSTR(metin.as_ptr()));
                }
                if zamanlar.len() > 1 {
                    let _ = AppendMenuW(alt, MF_SEPARATOR, 0, PCWSTR::null());
                    let _ = AppendMenuW(alt, MF_STRING, KOMUT_ZAMANLAYICI_HEPSI, PCWSTR(zh.as_ptr()));
                }
                let _ = AppendMenuW(menu, MF_POPUP, alt.0 as usize, PCWSTR(zm.as_ptr()));
            }
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, KOMUT_PALET, PCWSTR(pl.as_ptr()));
        let _ = AppendMenuW(menu, MF_STRING, KOMUT_STUDYO, PCWSTR(s.as_ptr()));
        if let Ok(alt) = CreatePopupMenu() {
            let _ = AppendMenuW(alt, MF_STRING | isaret(efektler), KOMUT_EFEKTLER, PCWSTR(ef1.as_ptr()));
            let gri = if efektler { MENU_ITEM_FLAGS(0) } else { MF_GRAYED };
            let _ = AppendMenuW(alt, MF_STRING | isaret(ipucu_efektleri) | gri, KOMUT_IPUCU_EFEKTLERI, PCWSTR(ef2.as_ptr()));
            let _ = AppendMenuW(menu, MF_POPUP, alt.0 as usize, PCWSTR(ef.as_ptr()));
        }
        let _ = AppendMenuW(menu, MF_STRING, KOMUT_AYAR_AC, PCWSTR(a.as_ptr()));
        let _ = AppendMenuW(menu, MF_STRING, KOMUT_YENIDEN_YUKLE, PCWSTR(y.as_ptr()));
        let _ = AppendMenuW(menu, MF_STRING, KOMUT_GUNLUK_AC, PCWSTR(g.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let gri = if denetleniyor { MF_GRAYED } else { MENU_ITEM_FLAGS(0) };
        let _ = AppendMenuW(menu, MF_STRING | gri, KOMUT_GUNCELLEME_DENETLE, PCWSTR(gd.as_ptr()));
        if hazir.is_some() {
            let _ = AppendMenuW(menu, MF_STRING, KOMUT_GUNCELLEME_KUR, PCWSTR(gk.as_ptr()));
        }
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, KOMUT_CIKIS, PCWSTR(c.as_ptr()));

        let mut nokta = Default::default();
        let _ = GetCursorPos(&mut nokta);
        // Bilinen tepsi kuralı: menüden önce öne getir, sonra WM_NULL gönder; yoksa menü
        // dışına tıklayınca kapanmaz.
        let _ = SetForegroundWindow(hwnd);
        // `u` bu noktadan sonra kullanılmaz: menü iç içe mesaj döngüsü çalıştırır.
        let secim = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, nokta.x, nokta.y, Some(0), hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        secim.0 as usize
    };

    let Some(u) = uyg(hwnd) else { return };
    match secim {
        KOMUT_DURAKLAT_SURDUR => durdur_surdur(u),
        KOMUT_STUDYO => studyo_ac(u, false),
        KOMUT_AYAR_AC => {
            let yol = u.ayar_yolu.clone();
            dosya_ac(&yol);
        }
        KOMUT_YENIDEN_YUKLE => ayarlari_yeniden_yukle(u, Kaynak::Kullanici),
        KOMUT_EFEKTLER => efekt_anahtari_degistir(u, EfektAnahtari::Hepsi),
        KOMUT_IPUCU_EFEKTLERI => efekt_anahtari_degistir(u, EfektAnahtari::Ipucu),
        KOMUT_GUNCELLEME_DENETLE => guncelleme_baslat(u, true),
        KOMUT_GUNCELLEME_KUR => {
            guncellemeyi_kur(hwnd);
        }
        KOMUT_GUNLUK_AC => match gunluk::yol() {
            Some(yol) if yol.exists() => dosya_ac(&yol),
            _ => balon(u, "Echora", "Günlük dosyası henüz yok.", false),
        },
        KOMUT_CIKIS => {
            cikis_hazirla(u);
            // SAFETY: pencere bu iş parçacığına ait.
            let _ = unsafe { DestroyWindow(hwnd) };
        }
        KOMUT_PALET => palet_ac(u),
        KOMUT_ZAMANLAYICI_HEPSI => {
            zamanlayicilari_iptal(u, "tepsi menüsü");
        }
        n if (KOMUT_ZAMANLAYICI_TABAN..KOMUT_ZAMANLAYICI_TABAN + MENU_ZAMANLAYICI).contains(&n) => {
            if let Some((kimlik, _)) = zamanlar.get(n - KOMUT_ZAMANLAYICI_TABAN) {
                zamanlayici_iptal(u, *kimlik, "tepsi menüsü");
            }
        }
        _ => {}
    }
}

/// Çalışan sahneyi keser, bekleyen efekt-sonrası eylemi atar, zamanlayıcıyı durdurur. Kapanış ve
/// duraklatma yolları ortak kullanır.
fn bekleyenleri_kes(u: &mut Uygulama, neden: &str) {
    if u.sahne.is_some() {
        sahneyi_bitir(u, true);
    }
    if let Some(b) = bekleyeni_al(u) {
        gunluk::yaz(&format!("\"{}\" çalıştırılmadı ({neden})", b.komut.gosterim));
    }
}

/// Bekleyeni çıkarır ve süre dolumu zamanlayıcısını durdurur. Gecikmiş `WM_EFEKT_BITTI` jeton
/// denetimiyle atılır.
fn bekleyeni_al(u: &mut Uygulama) -> Option<Bekleyen> {
    let b = u.bekleyen.take();
    if b.is_some() {
        // SAFETY: pencere bu iş parçacığına ait.
        let _ = unsafe { KillTimer(Some(u.hwnd), BEKLEYEN_ZAMANLAYICI) };
    }
    b
}

/// `bitince` süresinde gelmedi (efekt motoru takıldı): bekleyen atılır, eylem ÇALIŞTIRILMAZ
/// (efekt bitmedi; kullanıcı ekranı izliyor olabilir).
fn bekleyen_suresi_doldu(u: &mut Uygulama) {
    if let Some(b) = bekleyeni_al(u) {
        gunluk::yaz(&format!("HATA: efekt tamamlanamadı; \"{}\" çalıştırılmadı", b.komut.gosterim));
        balon(
            u,
            "Echora: efekt tamamlanamadı",
            &format!("Efekt tamamlanamadı; \"{}\" çalıştırılmadı (ekran kapatılmadı).", b.komut.gosterim),
            true,
        );
    }
}

/// Çıkış başlıyor: kuyruktakiler ve bekleyenler atılır. Tekrar çağrılması zararsızdır.
fn cikis_hazirla(u: &mut Uygulama) {
    if u.kapaniyor {
        return;
    }
    u.kapaniyor = true;
    bekleyenleri_kes(u, "çıkılıyor");
    u.ipc_kuyrugu.clear();
    u.tus_penceresi.hepsini_kapat();
    // Zamanlayıcılar kalıcı değildir: kapanınca kaybolur.
    let kalan = u.zamanlayicilar.hepsini_iptal();
    if !kalan.is_empty() {
        let adlar: Vec<&str> = kalan.iter().map(|z| z.gosterim.as_str()).collect();
        gunluk::yaz(&format!("{} zamanlayıcı çalıştırılmadı (çıkılıyor): {}", kalan.len(), adlar.join(", ")));
    }
}

fn duraklat(u: &mut Uygulama) {
    // Bayrak, dinleyiciye haber gitmeden ONCE ayarlanir: kuyrukta bekleyen komutlar atilsin.
    u.duraklatildi = true;
    bekleyenleri_kes(u, "dinleme duraklatıldı");
    // Zamanlayıcılar duraklatmada sürer (önceden kurulmuş, menüden iptal edilebilir).
    tus_penceresi_kapat(u, None, "dinleme duraklatıldı");
    // `Sahne::kapat` burada ÇAĞRILMAZ (UI'yı bloklayabilir): sürmekte olan efekt kendiliğinden
    // biter, gecikmiş "bitince"si jeton denetimiyle atılır; motor 2 sn boşta kalınca kapanır.
    if let Some(kol) = &u.kol {
        kol.duraklat();
    }
}

fn surdur(u: &mut Uygulama) {
    u.duraklatildi = false;
    if let Some(kol) = &u.kol {
        kol.surdur();
    }
}

/// Dinliyorsa duraklatir; aksi halde (duraklatildi, mikrofon yok, model yok) surdurmeyi dener.
fn durdur_surdur(u: &mut Uygulama) {
    if u.durum.dinliyor_mu() && !u.duraklatildi {
        duraklat(u);
    } else {
        surdur(u);
    }
}

/// Basarisiz baslangic gibi durumlari disaridan bildirmek icin.
pub fn durum_bildir(hwnd: HWND, d: Durum) {
    if let Some(u) = uyg(hwnd) {
        durum_ayarla(u, d);
    }
}

/// Yeni (geçerli) ayarları UI'ya, sisteme ve dinleyiciye uygular.
fn ayarlari_uygula(u: &mut Uygulama, a: Ayarlar) {
    if let Some(s) = u.sistem.as_mut() {
        s.yontemleri_uygula(&u.ayarlar.ekran_yontemleri, &a.ekran_yontemleri);
    }
    // Otomatik denetim sonradan açıldıysa zamanlayıcı yeniden kurulur (kapatılınca zamanlayıcı
    // bir sonraki tetiklenişte kendini durdurur).
    let otomatik_acildi = a.guncelleme_otomatik && !u.ayarlar.guncelleme_otomatik;
    u.ayarlar = a;
    if otomatik_acildi && !u.kuru {
        // SAFETY: pencere bu iş parçacığına ait; aynı kimlikli zamanlayıcı yenilenir.
        let _ = unsafe { SetTimer(Some(u.hwnd), GUNCELLEME_ZAMANLAYICI, GUNCELLEME_ILK_MS, None) };
    }
    // Kısayollar dinleyiciden ÖNCE: dinleme tuşu kaydedilemezse dinleyici sürekli kipe düşer.
    kisayollari_kaydet(u);
    if let Some(kol) = &u.kol {
        kol.ayar_gonder(dinleyici_ayarlari(u));
    }
    // Dinleme kipi / tuşu değişmiş olabilir.
    simge_guncelle(u);
}

// --- Tepsi menüsündeki efekt anahtarları ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EfektAnahtari {
    /// `efektler_acik`
    Hepsi,
    /// `olay_efektleri_acik`
    Ipucu,
}

/// Anahtarı ayar dosyasında çevirir: dosya ham şemayla okunur, alan değiştirilir, doğrulanıp
/// atomik yazılır (Stüdyo'nun kuralı: ilk yazımdan önce `ayarlar.toml.bak` yedeği), sonra yeniden
/// yüklenir. Yeni değer kullanıcının menüde gördüğü değerin tersidir.
fn efekt_anahtari_degistir(u: &mut Uygulama, anahtar: EfektAnahtari) {
    use studyo::dosya::{self, Yukleme};
    let yol = u.ayar_yolu.clone();
    let mut ham = match dosya::ayar_yukle(&yol) {
        Yukleme::Tamam(h) | Yukleme::Yok(h) => h,
        Yukleme::Bozuk(h) => {
            gunluk::yaz(&format!("HATA: efekt anahtarı yazılamadı: {h}"));
            balon(u, "Echora: ayar dosyası geçersiz", &h, true);
            return;
        }
    };
    let (ad, yeni) = match anahtar {
        EfektAnahtari::Hepsi => {
            ham.efektler_acik = !u.ayarlar.efektler_acik;
            ("efektler", ham.efektler_acik)
        }
        EfektAnahtari::Ipucu => {
            ham.olay_efektleri_acik = !u.ayarlar.olay_efektleri_acik;
            ("ipucu efektleri", ham.olay_efektleri_acik)
        }
    };
    let sonuc = dosya::yedek_al(&yol).and_then(|_| dosya::ayar_kaydet(&yol, &ham).map(drop));
    match sonuc {
        Ok(()) => {
            gunluk::yaz(&format!("{ad} {} (tepsi menüsü)", if yeni { "açıldı" } else { "kapatıldı" }));
            ayarlari_yeniden_yukle(u, Kaynak::Tepsi);
        }
        Err(h) => {
            gunluk::yaz(&format!("HATA: efekt anahtarı yazılamadı: {h}"));
            balon(u, "Echora: ayar yazılamadı", &h, true);
        }
    }
}

// --- Güncelleme ---

/// Açılışta: kuru kipte hiçbir şey yapılmaz (ağa çıkılmaz). Aksi halde durum dosyası toparlanır
/// (yarım "indiriliyor" ve bu süreçte doğrulanmamış "hazır" güncel'e döner; hazır zip bir sonraki
/// denetimde GitHub'daki SHA256SUMS ile yeniden doğrulanır), önceki kurulum hatası bir kez
/// bildirilir ve ilk denetim 60 sn sonraya kurulur.
pub fn guncelleme_zamanla(hwnd: HWND) {
    let Some(u) = uyg(hwnd) else { return };
    if u.kuru {
        gunluk::yaz("[kuru] güncelleyici kapalı: kuru kipte ağa çıkılmaz");
        return;
    }
    if let Some(h) = guncelleme::baslangic_durumu(&u.guncelleme_klasoru) {
        gunluk::yaz(&format!("UYARI: önceki güncelleme uygulanamadı: {h}"));
        balon(u, "Echora: güncelleme uygulanamadı", &h, true);
    }
    // SAFETY: pencere bu iş parçacığına ait.
    let _ = unsafe { SetTimer(Some(hwnd), GUNCELLEME_ZAMANLAYICI, GUNCELLEME_ILK_MS, None) };
}

/// Zamanlayıcı (ilk: 60 sn, sonra saatlik) ve uyku dönüşü. İlk tetiklenişte otomatik açıksa
/// denetlenir; sonrakilerde yalnız son denetimden 24 saat geçtiyse (uyku/hazırda bekletmede
/// zamanlayıcı saymaz; karar `son_denetim`e göre verilir). Otomatik kapalıysa yalnız eski indirmeler
/// temizlenir.
fn guncelleme_zamani(hwnd: HWND) {
    let Some(u) = uyg(hwnd) else { return };
    if u.kuru || u.kapaniyor {
        // SAFETY: pencere bu iş parçacığına ait.
        let _ = unsafe { KillTimer(Some(hwnd), GUNCELLEME_ZAMANLAYICI) };
        return;
    }
    let ilk = !std::mem::replace(&mut u.ilk_denetim_yapildi, true);
    // SAFETY: pencere bu iş parçacığına ait; aynı kimlikli zamanlayıcı yenilenir.
    let _ = unsafe { SetTimer(Some(hwnd), GUNCELLEME_ZAMANLAYICI, GUNCELLEME_SAATLIK_MS, None) };
    if u.ayarlar.guncelleme_otomatik {
        let zamani = ilk
            || guncelleme::denetim_zamani_mi(guncelleme::son_denetim(&u.guncelleme_klasoru), studyo::guncelleme::simdi_unix());
        if zamani {
            guncelleme_baslat(u, false);
        }
    } else if ilk {
        let klasor = u.guncelleme_klasoru.clone();
        if klasor.is_dir() {
            let _ = std::thread::Builder::new()
                .name("guncelleme-temizlik".into())
                .spawn(move || guncelleme::surum_klasorlerini_temizle(&klasor, Surum::simdiki(), None));
        }
    }
}

/// Uykudan/hazırda beklemeden dönüş: ağ hazır olsun diye karar 60 sn sonra aynı kuralla verilir.
fn uyku_donusu(hwnd: HWND) {
    let Some(u) = uyg(hwnd) else { return };
    if u.kuru || u.kapaniyor || !u.ayarlar.guncelleme_otomatik {
        return;
    }
    // SAFETY: pencere bu iş parçacığına ait; aynı kimlikli zamanlayıcı yenilenir.
    let _ = unsafe { SetTimer(Some(hwnd), GUNCELLEME_ZAMANLAYICI, GUNCELLEME_ILK_MS, None) };
}

/// Denetimi ayrı iş parçacığında başlatır (aynı anda tek denetim). `elle`: menü ya da Stüdyo
/// istedi; sonuç her durumda balonla bildirilir. Kuru kipte AĞA ÇIKILMAZ.
fn guncelleme_baslat(u: &mut Uygulama, elle: bool) {
    if u.kuru {
        gunluk::yaz("[kuru] güncelleme denetimi istendi; kuru kipte ağa çıkılmaz");
        if elle {
            balon(u, "Echora", "Kuru kipte güncelleme denetlenmez.", false);
        }
        return;
    }
    if u.denetim_suruyor.swap(true, Ordering::SeqCst) {
        if elle {
            balon(u, "Echora", "Güncelleme denetimi zaten sürüyor.", false);
        }
        return;
    }
    gunluk::yaz(if elle { "güncelleme denetimi başladı (elle)" } else { "güncelleme denetimi başladı" });
    let klasor = u.guncelleme_klasoru.clone();
    let mevcut = u.hazir;
    let bayrak = Arc::clone(&u.denetim_suruyor);
    let kutu = Arc::clone(&u.denetim_sonucu);
    let hedef = u.hwnd.0 as isize;
    let r = std::thread::Builder::new().name("guncelleme".into()).spawn(move || {
        let s = guncelleme::denetle(&klasor, mevcut);
        if let Ok(mut k) = kutu.lock() {
            *k = Some((s, elle));
        }
        bayrak.store(false, Ordering::SeqCst);
        // SAFETY: PostMessageW her iş parçacığından çağrılabilir; pencere yoksa hata döner.
        let _ = unsafe { PostMessageW(Some(HWND(hedef as *mut _)), WM_GUNCELLEME, WPARAM(0), LPARAM(0)) };
    });
    if let Err(h) = r {
        u.denetim_suruyor.store(false, Ordering::SeqCst);
        gunluk::yaz(&format!("HATA: güncelleme iş parçacığı başlatılamadı: {h}"));
    }
}

fn guncelleme_sonucu(hwnd: HWND) {
    let Some(u) = uyg(hwnd) else { return };
    let Some((s, elle)) = u.denetim_sonucu.lock().ok().and_then(|mut k| k.take()) else { return };
    if u.kapaniyor {
        return;
    }
    match s {
        DenetimSonucu::Guncel => {
            u.hazir = None;
            gunluk::yaz(&format!("güncelleme: Echora güncel ({})", guncelleme::SURUM));
            if elle {
                balon(u, "Echora güncel", &format!("Echora {} en son sürüm.", guncelleme::SURUM), false);
            }
        }
        DenetimSonucu::Hazir { hazir, bildir } => {
            let surum = hazir.surum;
            u.hazir = Some(hazir);
            gunluk::yaz(&format!("güncelleme: Echora {surum} indirildi ve doğrulandı; kurulum onay bekliyor"));
            if bildir || elle {
                balon(u, "Echora güncellemesi", &format!("Echora {surum} indirildi — kurmak için tıklayın"), false);
                u.guncelleme_balonu = true;
            }
        }
        // Önceden hazır olan sürüm (varsa) hazır kalır.
        DenetimSonucu::Hata(h) => {
            gunluk::yaz(&format!("UYARI: güncelleme denetlenemedi: {h}"));
            if elle {
                balon(u, "Echora: güncelleme denetlenemedi", &h, true);
            }
        }
    }
}

/// Kurulum ön koşulları: bu süreçte doğrulanmış hazır sürüm ve kurulu konumdan çalışma. Dönen:
/// (hazır sürüm, kurulum klasörü).
fn kurulum_hazirligi(u: &Uygulama) -> Result<(guncelleme::HazirSurum, PathBuf), String> {
    if u.kuru {
        return Err("kuru kipte güncelleme kurulmaz".into());
    }
    let h = u.hazir.ok_or("Kuruluma hazır (bu oturumda doğrulanmış) güncelleme yok; güncellemeleri denetleyin.")?;
    let exe = std::env::current_exe().map_err(|x| format!("exe yolu alınamadı: {x}"))?;
    let hedef = match guncelleme::kurulum_karari(&exe, studyo::kurulum::kurulum_klasoru().as_deref()) {
        KurulumKarari::Kurulu(k) => k,
        KurulumKarari::Gelistirme => return Err("Geliştirme sürümü güncellenmez (target klasöründen çalışıyor).".into()),
        KurulumKarari::KuruluDegil => {
            return Err("Güncelleme yalnız kurulu Echora'ya (%LOCALAPPDATA%\\Programs\\Echora) uygulanır.".into());
        }
    };
    Ok((h, hedef))
}

/// Kurulum onayı (evet/hayır). Menü, balon ve Stüdyo yolunun TEK onay kapısı. İç içe ileti
/// döngüsü çalıştırır: çağıran `Uygulama` başvurusunu bundan sonra yeniden almalıdır.
fn kurulum_onayi(hwnd: HWND, s: Surum) -> bool {
    let metin = genis(&format!(
        "Echora {s} kurulsun mu?\n\nEchora kapanır, yeni sürüm kurulu klasöre kopyalanır ve Echora yeniden açılır. Kapatılan ekranlar önce açılır. Kurulum başarısız olursa eski sürüm yerinde kalır."
    ));
    let baslik = genis("Echora güncellemesi");
    // SAFETY: metinler NUL'lu ve çağrı boyunca yaşar.
    let r = unsafe {
        MessageBoxW(Some(hwnd), PCWSTR(metin.as_ptr()), PCWSTR(baslik.as_ptr()), MB_YESNO | MB_ICONQUESTION | MB_SETFOREGROUND | MB_TOPMOST)
    };
    r == IDYES
}

/// Aynı exe'den açılmış Stüdyo süreçlerinin pencerelerine `WM_CLOSE` gönderir ve en çok 5 sn
/// kapanmalarını bekler. Kaydedilmemiş değişiklik sorusu çıkarsa Stüdyo kapanmaz: `Err`.
fn studyo_surecleri_kapat() -> Result<(), String> {
    let Ok(exe) = std::env::current_exe() else { return Ok(()) };
    let surecler = studyo::kurulum::calisan_surecler(&exe);
    if surecler.is_empty() {
        return Ok(());
    }
    unsafe extern "system" fn kapat(h: HWND, l: LPARAM) -> windows::core::BOOL {
        // SAFETY: `l`, aşağıda bu çağrı boyunca yaşayan süreç listesini gösterir.
        let liste = unsafe { &*(l.0 as *const Vec<u32>) };
        let mut pid = 0u32;
        // SAFETY: salt okuma; kapatma iletisi yalnız listedeki süreçlerin pencerelerine.
        unsafe {
            GetWindowThreadProcessId(h, Some(&mut pid));
            if liste.contains(&pid) && IsWindowVisible(h).as_bool() {
                let _ = PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        windows::core::BOOL(1)
    }
    // SAFETY: liste EnumWindows boyunca yaşar; geri çağrı yalnız okur.
    let _ = unsafe { EnumWindows(Some(kapat), LPARAM(&surecler as *const Vec<u32> as isize)) };
    let bitis = Instant::now() + Duration::from_secs(5);
    for pid in surecler {
        let kalan = bitis.saturating_duration_since(Instant::now());
        if !guncelleme::bitmesini_bekle(pid, kalan) {
            return Err("Echora Stüdyo açık (kaydedilmemiş değişiklik olabilir). Önce Stüdyo'yu kapatın, sonra yeniden deneyin.".into());
        }
    }
    Ok(())
}

/// Hazır güncellemeyi kurar. Kurulum ASLA onaysız başlamaz: menü, balon ve Stüdyo isteği aynı
/// evet/hayır kutusundan geçer. Onaydan sonra zip'in özeti bellekteki özetle yeniden denetlenir ve
/// yeni bir klasöre açılır; açık Stüdyo kapatılır; güncelleyici süreç başlatılır ve bu süreç temiz
/// çıkar (perdeler kalkar, DDC ile kapanan monitörler açılır: olağan çıkış yolu).
fn guncellemeyi_kur(hwnd: HWND) -> bool {
    let Some(u) = uyg(hwnd) else { return false };
    let (h, hedef) = match kurulum_hazirligi(u) {
        Ok(x) => x,
        Err(x) => {
            gunluk::yaz(&format!("güncelleme kurulmadı: {x}"));
            balon(u, "Echora", &x, false);
            return false;
        }
    };
    if !kurulum_onayi(hwnd, h.surum) {
        gunluk::yaz(&format!("güncelleme: Echora {} kurulumu ertelendi (kullanıcı)", h.surum));
        return false;
    }
    let Some(u) = uyg(hwnd) else { return false };
    if u.kapaniyor {
        return false;
    }
    let sonuc = guncelleme::kurulum_kaynagini_hazirla(&u.guncelleme_klasoru, h)
        .and_then(|kaynak| studyo_surecleri_kapat().map(|()| kaynak))
        .and_then(|kaynak| guncelleme::guncelleyiciyi_baslat(&kaynak, &hedef));
    match sonuc {
        Ok(()) => {
            gunluk::yaz(&format!("güncelleme: Echora {} kuruluyor; uygulama yeniden açılacak", h.surum));
            cikis_hazirla(u);
            // SAFETY: pencere bu iş parçacığına ait.
            let _ = unsafe { DestroyWindow(hwnd) };
            true
        }
        Err(x) => {
            gunluk::yaz(&format!("HATA: güncelleme kurulamadı: {x}"));
            if x.contains("değişmiş") {
                // Doğrulanmış dosya değişti: hazır sayılmaz, bir sonraki denetim yeniden indirir.
                u.hazir = None;
            }
            balon(u, "Echora: güncelleme kurulamadı", &x, true);
            false
        }
    }
}

fn ayarlari_yeniden_yukle(u: &mut Uygulama, kaynak: Kaynak) {
    let bayt = std::fs::read(&u.ayar_yolu);
    let ozet = bayt.as_ref().ok().map(|b| crate::izleyici::ozet(b));
    if kaynak == Kaynak::Dosya && ozet == u.son_ozet {
        return;
    }
    u.son_ozet = ozet;
    let sonuc = match bayt {
        Ok(b) => String::from_utf8(b)
            .map_err(|_| format!("{}: geçerli UTF-8 değil", u.ayar_yolu.display()))
            .and_then(|m| crate::ayar_metninden(&u.ayar_yolu, &m)),
        Err(h) => Err(format!("{} okunamadı: {h}", u.ayar_yolu.display())),
    };
    match sonuc {
        Ok(a) => {
            ayarlari_uygula(u, a);
            gunluk::yaz(match kaynak {
                Kaynak::Kullanici => "ayarlar yeniden yüklendi",
                Kaynak::Dosya => "ayarlar yeniden yüklendi (dosya değişti)",
                Kaynak::Studyo => "ayarlar yeniden yüklendi (Stüdyo)",
                Kaynak::Tepsi => "ayarlar yeniden yüklendi (tepsi menüsü)",
            });
            if kaynak == Kaynak::Kullanici {
                balon(u, "Echora", "Ayarlar yeniden yüklendi.", false);
            }
        }
        Err(h) => {
            gunluk::yaz(&format!("HATA: ayarlar yeniden yüklenemedi, eski ayarla devam: {h}"));
            balon(u, "Echora: ayar hatası (eski ayarla devam)", &h, true);
        }
    }
}

// --- Efektler ---

fn tohum(u: &mut Uygulama) -> u64 {
    u.sayac = u.sayac.wrapping_add(1);
    let zaman = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    zaman ^ u.sayac.rotate_left(32)
}

/// Motorun son hatasını alır; günlüğe yalnız İLK hata yazılır (eylemler etkilenmez).
fn efekt_hatasini_denetle(u: &mut Uygulama) {
    if let Some(h) = sahne::son_hata() {
        if !u.efekt_hatasi_yazildi {
            u.efekt_hatasi_yazildi = true;
            gunluk::yaz(&format!("UYARI: efekt çizilemedi (eylemler yine çalışır): {h}"));
        }
    }
}

/// Efekti oynatır; bitince bu pencereye `WM_EFEKT_BITTI(jeton)` atılır. Efektler kapalıyken
/// (`efekt_oynasin_mi` false) jetonsuz istekler hiç oynatılmaz; jetonlu (bekleyenli) istek yalnız
/// efektler açıkken oluşur, yine de kapalıysa `bitince` hemen gönderilir (eylem takılmasın).
fn efekt_oynat(u: &mut Uygulama, efekt: Efekt, ayar: EfektAyari, metin: Option<String>, jeton: u64) {
    efekt_hatasini_denetle(u);
    if !u.ayarlar.efekt_oynasin_mi() {
        if u.kuru {
            gunluk::yaz(&format!("[kuru] efekt oynatılmadı (efektler kapalı): {}", efekt.ad()));
        }
        if jeton != 0 {
            ileti_at(u.hwnd, WM_EFEKT_BITTI, jeton as usize);
        }
        return;
    }
    // Bekleyen efekt-sonrası eylem varken başka efekt oynatılmaz: `Sahne::oynat` onun efektini
    // keser ve `bitince`'ini hemen çağırırdı (ekran erken kapanırdı).
    if jeton == 0 && u.bekleyen.is_some() {
        if u.kuru {
            gunluk::yaz(&format!("[kuru] efekt oynatılmadı (bekleyen var): {}", efekt.ad()));
        }
        return;
    }
    if u.kuru {
        gunluk::yaz(&format!("[kuru] efekt başladı: {} ({} ms, #{jeton})", efekt.ad(), ayar.sure_ms));
    }
    let hedef = u.hwnd.0 as isize;
    let bitince: Box<dyn FnOnce() + Send> = Box::new(move || {
        // SAFETY: PostMessageW her iş parçacığından çağrılabilir; pencere yoksa hata döner.
        let _ = unsafe {
            PostMessageW(Some(HWND(hedef as *mut _)), WM_EFEKT_BITTI, WPARAM(jeton as usize), LPARAM(0))
        };
    });
    u.efekt.oynat(Istek { efekt, ayar, metin, hedef: Hedef::Ekran(ayar.ekran) }, Some(bitince));
}

/// Komut dışı olay ipucu. Ayarda kapalıysa hiçbir şey olmaz. Efekt-sonrası bekleyen bir eylem
/// varken oynatılmaz: yeni efekt onun efektini keser ve ekranı erken kapattırırdı.
fn olay_oynat(u: &mut Uygulama, olay: Olay) {
    if u.bekleyen.is_some() || u.kapaniyor {
        return;
    }
    if let Some((e, a)) = u.ayarlar.olay_efekti(olay) {
        efekt_oynat(u, e, a, None, 0);
    }
}

fn efekt_bitti(hwnd: HWND, jeton: u64) {
    let Some(u) = uyg(hwnd) else { return };
    efekt_hatasini_denetle(u);
    if u.kuru {
        gunluk::yaz(&format!("[kuru] efekt bitti (#{jeton})"));
    }
    if jeton == 0 || u.bekleyen.as_ref().map(|b| b.jeton) != Some(jeton) {
        return;
    }
    let Some(b) = bekleyeni_al(u) else { return };
    // Bayat komut denetimi burada da: efekt sürerken duraklatıldıysa ya da çıkılıyorsa çalışmaz.
    // (Duraklatma bekleyeni zaten siler; bu denetim duraklatılmışken gelmiş komutlar içindir.)
    if (u.duraklatildi && !b.kaynak.duraklatmayi_asar()) || u.kapaniyor {
        gunluk::yaz(&format!("\"{}\" çalıştırılmadı (efekt sürerken duraklatıldı)", b.komut.gosterim));
        return;
    }
    if Instant::now() > b.son_tarih {
        gunluk::yaz(&format!("bayat komut atıldı: {}", b.komut.gosterim));
        return;
    }
    eylemi_uygula(u, &b.komut.eylem, &b.komut.gosterim, b.kaynak);
}

// --- Komutlar ---

fn hata_bildir(u: &mut Uygulama, gosterim: &str, hata: &str) {
    gunluk::yaz(&format!("HATA: \"{gosterim}\" çalıştırılamadı: {hata}"));
    balon(u, "Echora: komut başarısız", &format!("{gosterim}: {hata}"), true);
    olay_oynat(u, Olay::Hata);
}

/// Komutu başlatır: efekt + eylem (+ geri al kaydı). `false`: çıkış başladı, kuyruğun kalanı atılmalı.
fn komutu_baslat(u: &mut Uygulama, komut: Komut, kaynak: KomutKaynagi) -> bool {
    gunluk::yaz(&format!("komut{}: {}", kaynak.etiket(), komut.gosterim));
    let t = tohum(u);
    let (efekt, ayar) = komut.efekt_coz(&u.ayarlar, t);
    let metin = Some(komut.gosterim.clone());
    let karar = bekleyen_karari(&komut.eylem, u.bekleyen.is_some());
    match karar {
        BekleyenKarari::YalnizIptal | BekleyenKarari::IptalEtVeCalistir | BekleyenKarari::YerineGec => {
            if let Some(b) = bekleyeni_al(u) {
                gunluk::yaz(&format!("bekleyen eylem iptal edildi: {} ({})", b.komut.gosterim, komut.gosterim));
            }
        }
        BekleyenKarari::Normal | BekleyenKarari::EfektsizCalistir => {}
    }
    match (karar, &komut.eylem) {
        // "Geri al" bekleyen kapatmayı iptal eder; o eylem hiç çalışmadığı için yığına dokunulmaz.
        (BekleyenKarari::YalnizIptal, _) => return true,
        (_, komut::Eylem::GeriAl) => {
            geri_al(u, Some((efekt, ayar, metin)));
            return true;
        }
        // Duraklatma efektinin kendisi görünsün diye önce eylem, sonra efekt.
        (_, komut::Eylem::DinlemeyiDuraklat) => {
            let devam = eylemi_uygula(u, &komut.eylem, &komut.gosterim, kaynak);
            efekt_oynat(u, efekt, ayar, metin, 0);
            return devam;
        }
        _ => {}
    }
    // Efektler kapalıyken bekleyen/jeton oluşmaz: ekranı kapatan / kilitleyen / uyutan eylemler
    // de efekt beklemeden hemen çalışır (`efekt_oynat` efekti zaten oynatmaz).
    if crate::dagitim::efekt_beklenir_mi(&komut.eylem, u.ayarlar.efekt_oynasin_mi()) {
        u.sayac = u.sayac.wrapping_add(1);
        let jeton = u.sayac.max(1);
        let sure = Duration::from_millis(u64::from(ayar.sure_ms)) + BEKLEYEN_PAYI;
        u.bekleyen = Some(Bekleyen { jeton, komut, son_tarih: Instant::now() + sure, kaynak });
        // SAFETY: pencere bu iş parçacığına ait; aynı kimlikli zamanlayıcı yenilenir.
        let _ = unsafe { SetTimer(Some(u.hwnd), BEKLEYEN_ZAMANLAYICI, sure.as_millis() as u32, None) };
        efekt_oynat(u, efekt, ayar, metin, jeton);
        return true;
    }
    // EfektsizCalistir'da efekt_oynat bekleyen varken zaten oynatmaz.
    efekt_oynat(u, efekt, ayar, metin, 0);
    eylemi_uygula(u, &komut.eylem, &komut.gosterim, kaynak)
}

/// Ekran durumu bayrağı: DDC/CI ile kapatma `true`, ekranları açma `false`.
fn ddc_bayragi(bayrak: &AtomicBool, eylem: &komut::Eylem, sonuc: &Sonuc) {
    if sonuc.ddc_kapatildi {
        bayrak.store(true, Ordering::Relaxed);
    } else if matches!(eylem, komut::Eylem::EkranlariAc) {
        bayrak.store(false, Ordering::Relaxed);
    }
}

/// Eylemi `Sistem` üzerinde çalıştırır ve etkisini uygular. `false`: çıkış başladı.
fn eylemi_uygula(u: &mut Uygulama, eylem: &komut::Eylem, gosterim: &str, kaynak: KomutKaynagi) -> bool {
    let Some(s) = u.sistem.as_mut() else { return true };
    let sonuc = match dagit(s.sistem(), &*u.imzaci, eylem) {
        Ok(r) => r,
        Err(h) => {
            hata_bildir(u, gosterim, &h);
            return true;
        }
    };
    ddc_bayragi(&u.ddc_kapali, eylem, &sonuc);
    if let Some(k) = sonuc.kayit {
        u.geri_al.ekle(gosterim, k);
    }
    match sonuc.etki {
        Etki::Tamam => {}
        Etki::Duraklat => duraklat(u),
        Etki::Cik => {
            cikis_hazirla(u);
            // SAFETY: pencere bu is parcacigina ait.
            let _ = unsafe { DestroyWindow(u.hwnd) };
            return false;
        }
        Etki::GeriAl => geri_al(u, None),
        Etki::StudyoyuAc => studyo_ac(u, true),
        Etki::YenidenYukle => ayarlari_yeniden_yukle(u, Kaynak::Kullanici),
        Etki::Sahne(ad) => sahne_baslat(u, &ad, kaynak.duraklatmayi_asar()),
        Etki::Zamanla { dakika, eylem, gosterim: asil } => zamanlayici_kur(u, dakika, eylem, &asil, gosterim),
        Etki::ZamanlayicilariIptal => {
            if zamanlayicilari_iptal(u, gosterim) == 0 {
                balon(u, "Echora", "Kurulu zamanlayıcı yok.", false);
            }
        }
    }
    true
}

/// Son kaydı geri alır. Geri alınamıyorsa (yığın boş / son kayıt geri alınamaz) komutun efekti
/// yerine "tetik gerekli" görünümü (kehribar kenar nabzı: "olmaz" ipucu, hata değil) ve bilgi
/// balonu gösterilir.
fn geri_al(u: &mut Uygulama, efekt: Option<(Efekt, EfektAyari, Option<String>)>) {
    // Çalışan sahne önce kesilir; kesilene dek yapılan adımlar tek kayıt olarak geri alınır.
    if u.sahne.is_some() {
        sahneyi_bitir(u, true);
    }
    u.geri_al.eskileri_at(Instant::now());
    let yapilabilir = matches!(u.geri_al.son(), Some(k) if k.kayit != Kayit::Alinamaz);
    let ekran_acar = matches!(u.geri_al.son(), Some(GeriKaydi { kayit: Kayit::Ters(t), .. }) if t.ekran_acar());
    // Zamanlı komutun geri alınması o zamanlayıcıyı iptal eder (Sistem'e dokunulmaz).
    let zamanlayici = match u.geri_al.son() {
        Some(GeriKaydi { kayit: Kayit::Ters(Ters::Zamanlayici(k)), .. }) => Some(*k),
        _ => None,
    };
    if yapilabilir {
        if let Some((e, a, m)) = efekt {
            efekt_oynat(u, e, a, m, 0);
        }
    }
    let Some(s) = u.sistem.as_mut() else { return };
    let sonuc = u.geri_al.geri_al(s.sistem(), &*u.imzaci);
    match &sonuc {
        GeriAlSonucu::Yapildi(g) => {
            if ekran_acar {
                u.ddc_kapali.store(false, Ordering::Relaxed);
            }
            gunluk::yaz(&format!("geri alındı: {g}"));
            if let Some(k) = zamanlayici {
                zamanlayici_iptal(u, k, "geri al");
            }
        }
        GeriAlSonucu::Basarisiz { gosterim, hata } => hata_bildir(u, &format!("geri al ({gosterim})"), hata),
        GeriAlSonucu::Bos | GeriAlSonucu::Alinamaz(_) | GeriAlSonucu::PencereYok(_) => {
            let metin = sonuc.ipucu().unwrap_or_default();
            gunluk::yaz(&format!("geri al: {metin}"));
            olay_oynat(u, Olay::TetikGerekli);
            balon(u, "Echora", &metin, false);
        }
    }
}

// --- Sahneler ---

fn sahne_baslat(u: &mut Uygulama, ad: &str, studyodan: bool) {
    let Some(t) = u.ayarlar.sahneler.iter().find(|s| s.ad == ad).cloned() else {
        hata_bildir(u, ad, "sahne bulunamadı");
        return;
    };
    if u.sahne.is_some() {
        // Çalışan sahne varken yeni sahne gelirse eskisi kesilir.
        sahneyi_bitir(u, true);
    }
    u.sahne_nesil = u.sahne_nesil.wrapping_add(1);
    u.sahne = Some(SahneYurutucu::new(&t));
    u.sahne_studyodan = studyodan;
    ileti_at(u.hwnd, WM_SAHNE_ADIMI, u.sahne_nesil as usize);
}

/// Sıradaki tek adımı çalıştırır; sonraki adım ileti/zamanlayıcıyla gelir (UI bloklanmaz).
fn sahne_ilerle(u: &mut Uygulama) {
    if u.kapaniyor || (u.duraklatildi && !u.sahne_studyodan) {
        sahneyi_bitir(u, true);
        return;
    }
    let Some(y) = u.sahne.as_mut() else { return };
    match y.siradaki() {
        SiradakiAdim::Eylem(e) => {
            let Some(s) = u.sistem.as_mut() else { return };
            let r = dagit(s.sistem(), &*u.imzaci, &e);
            if let Ok(sonuc) = &r {
                ddc_bayragi(&u.ddc_kapali, &e, sonuc);
            }
            let etki = r.as_ref().ok().map(|s| s.etki.clone());
            if let Err(h) = &r {
                gunluk::yaz(&format!("HATA: \"{}\" sahnesi, \"{}\" adımı: {h}", y.ad(), eylem_ozeti(&e)));
            }
            y.sonuc(r.map(|s| s.kayit));
            // Sahnede kullanılabilen uygulama düzeyi eylemler.
            match etki {
                Some(Etki::StudyoyuAc) => studyo_ac(u, true),
                Some(Etki::YenidenYukle) => ayarlari_yeniden_yukle(u, Kaynak::Kullanici),
                Some(Etki::Zamanla { dakika, eylem, gosterim }) => {
                    let ozet = eylem_ozeti(&e);
                    zamanlayici_kur(u, dakika, eylem, &gosterim, &ozet);
                }
                Some(Etki::ZamanlayicilariIptal) => {
                    zamanlayicilari_iptal(u, "sahne");
                }
                _ => {}
            }
            ileti_at(u.hwnd, WM_SAHNE_ADIMI, u.sahne_nesil as usize);
        }
        SiradakiAdim::Bekle(0) => ileti_at(u.hwnd, WM_SAHNE_ADIMI, u.sahne_nesil as usize),
        SiradakiAdim::Bekle(ms) => {
            u.sahne_bekliyor = true;
            // SAFETY: pencere bu iş parçacığına ait; aynı kimlikli zamanlayıcı yenilenir.
            let _ = unsafe { SetTimer(Some(u.hwnd), SAHNE_ZAMANLAYICI, ms, None) };
        }
        SiradakiAdim::Bitti => sahneyi_bitir(u, false),
    }
}

/// Sahneyi bitirir ya da keser: kaydı yığına ekler, hata varsa tek balon gösterir.
fn sahneyi_bitir(u: &mut Uygulama, kesildi: bool) {
    // SAFETY: pencere bu iş parçacığına ait.
    let _ = unsafe { KillTimer(Some(u.hwnd), SAHNE_ZAMANLAYICI) };
    u.sahne_bekliyor = false;
    let Some(y) = u.sahne.take() else { return };
    let ozet = y.bitir();
    if let Some(k) = ozet.kayit {
        u.geri_al.ekle(&format!("{} sahnesi", ozet.ad), k);
    }
    if kesildi {
        gunluk::yaz(&format!("sahne kesildi: {}", ozet.ad));
    }
    if let Some(h) = ozet.hata {
        gunluk::yaz(&format!("HATA: \"{}\" sahnesi: {h}", ozet.ad));
        if !u.kapaniyor {
            balon(u, "Echora: sahne kısmen başarısız", &format!("{}: {h}", ozet.ad), true);
            olay_oynat(u, Olay::Hata);
        }
    }
}

// --- Stüdyo ---

/// `echora.exe --studyo [--ayar <aynı dosya>]`'yı ayrı süreç olarak başlatır (beklemez). Kuru
/// örnekten açılan Stüdyo'nun ortamına `ECHORA_KURU=1` konur: Stüdyo "Dene"yi `Echora.Tepsi.Kuru`
/// penceresine gönderir, gerçek örneğe değil. Gerçek kipte değişken (miras kalmışsa) kaldırılır.
pub fn studyo_sureci_baslat(ayar: Option<&Path>, kuru: bool) -> Result<(), String> {
    alt_surec_baslat("--studyo", ayar, kuru).map_err(|h| format!("Stüdyo başlatılamadı: {h}"))
}

/// `echora.exe --palet [--ayar …]`: komut paleti ayrı süreçte (tek örnek; açıksa öne gelir).
/// Kuru örnekten açılan palet komutları kuru örneğe gönderir (`ECHORA_KURU=1`).
pub fn palet_sureci_baslat(ayar: Option<&Path>, kuru: bool) -> Result<(), String> {
    alt_surec_baslat("--palet", ayar, kuru).map_err(|h| format!("Komut paleti başlatılamadı: {h}"))
}

fn alt_surec_baslat(kip: &str, ayar: Option<&Path>, kuru: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|h| format!("exe yolu alınamadı: {h}"))?;
    let mut komut = std::process::Command::new(&exe);
    komut.arg(kip);
    if let Some(a) = ayar {
        komut.arg("--ayar").arg(std::path::absolute(a).unwrap_or_else(|_| a.to_path_buf()));
    }
    if kuru {
        komut.env("ECHORA_KURU", "1");
    } else {
        komut.env_remove("ECHORA_KURU");
    }
    // `Child` düşünce süreç sürer; beklenmez.
    komut.spawn().map(drop).map_err(|h| h.to_string())
}

/// Palet tuşu ya da tepsi menüsü: kullanıcının açık isteği (kuru kipte de açılır; komutları kuru
/// örneğe gider).
fn palet_ac(u: &mut Uygulama) {
    match palet_sureci_baslat(u.ayar_arg.as_deref(), u.kuru) {
        Ok(()) => gunluk::yaz("komut paleti açılıyor"),
        Err(h) => {
            gunluk::yaz(&format!("HATA: {h}"));
            balon(u, "Echora: komut paleti açılamadı", &h, true);
        }
    }
}

/// `eylemden`: sesli/IPC komutla mı istendi. Kuru kipte komut yolu hiçbir şey başlatmaz; tepsi
/// menüsünden açıkça seçilmişse başlatır.
fn studyo_ac(u: &mut Uygulama, eylemden: bool) {
    if eylemden && u.kuru {
        gunluk::yaz("[kuru] stüdyo açılırdı");
        return;
    }
    if let Err(h) = studyo_sureci_baslat(u.ayar_arg.as_deref(), u.kuru) {
        gunluk::yaz(&format!("HATA: {h}"));
        balon(u, "Echora: Stüdyo açılamadı", &h, true);
    }
}

// --- IPC (Stüdyo → bu pencere, WM_COPYDATA) ---

fn ipc_al(hwnd: HWND, lp: LPARAM) -> LRESULT {
    let Some(u) = uyg(hwnd) else { return LRESULT(0) };
    if u.kapaniyor {
        return LRESULT(0);
    }
    let cds = lp.0 as *const COPYDATASTRUCT;
    if cds.is_null() {
        return LRESULT(0);
    }
    // SAFETY: WM_COPYDATA işlenirken sistem geçerli bir COPYDATASTRUCT ve verinin bu sürecteki
    // kopyasını sağlar; yalnız bu işlev boyunca okunur.
    let cds = unsafe { &*cds };
    let boyut = cds.cbData as usize;
    if boyut > ipc::EN_COK_BAYT {
        gunluk::yaz(&format!("UYARI: Stüdyo iletisi reddedildi: yük çok büyük ({boyut} bayt)"));
        return LRESULT(0);
    }
    let veri: &[u8] = if boyut == 0 || cds.lpData.is_null() {
        &[]
    } else {
        // SAFETY: yukarıdaki gibi; boyut sınır içinde.
        unsafe { std::slice::from_raw_parts(cds.lpData as *const u8, boyut) }
    };
    match ipc::coz(cds.dwData, veri) {
        // Eşzamanlı: hemen ardından gelen komut yeni ayarla eşlensin (okuma + doğrulama kısa;
        // dinleyiciye iletim yine eşzamansız).
        Ok(IpcIstegi::YenidenYukle) => {
            ayarlari_yeniden_yukle(u, Kaynak::Studyo);
            LRESULT(1)
        }
        Ok(IpcIstegi::GuncellemeDenetle) => {
            guncelleme_baslat(u, true);
            LRESULT(if u.kuru { 0 } else { 1 })
        }
        // Stüdyo isteği ayrı bir güven kapısı DEĞİLDİR: kurulum menüdeki aynı evet/hayır kutusundan
        // geçer. Ön koşullar burada eşzamanlı denetlenir (Stüdyo reddi görsün); onay, kurulum ve
        // çıkış ileti döngüsünde (WM_COPYDATA içinde kutu açılmaz, pencere yok edilmez).
        Ok(IpcIstegi::GuncellemeKur) => match kurulum_hazirligi(u) {
            Ok(_) => {
                ileti_at(hwnd, WM_GUNCELLEME_KUR, 0);
                LRESULT(1)
            }
            Err(h) => {
                gunluk::yaz(&format!("Stüdyo'nun güncelleme kurulum isteği reddedildi: {h}"));
                LRESULT(0)
            }
        },
        Ok(IpcIstegi::Komut(ifade)) => {
            // Stüdyo kaydedip hemen "dene" dediyse dosya yüklenenden yenidir: önce yükle.
            if crate::izleyici::dosya_ozeti(&u.ayar_yolu) != u.son_ozet {
                ayarlari_yeniden_yukle(u, Kaynak::Dosya);
            }
            match ipc::komut_bul(&u.ayarlar, &ifade) {
            // Gönderen (SendMessageTimeout) bekletilmesin: komut kuyruğa alınır, ileti döngüsünde çalışır.
            Some(k) => {
                u.ipc_kuyrugu.push_back(k.clone());
                ileti_at(hwnd, WM_IPC_KOMUT, 0);
                LRESULT(1)
            }
            None => {
                gunluk::yaz("UYARI: Stüdyo'dan gelen ifade hiçbir komutla eşleşmedi");
                LRESULT(0)
            }
            }
        }
        Err(h) => {
            gunluk::yaz(&format!("UYARI: Stüdyo iletisi reddedildi: {h}"));
            LRESULT(0)
        }
    }
}

/// Stüdyo'dan gelen komutlar: tetik kuralı ve dinleme duraklatması uygulanmaz (onayı Stüdyo aldı,
/// kullanıcı açıkça "dene" dedi). Normal yoldan çalışır: efekt + eylem + geri al. `--kuru`'ya uyar
/// (aynı `Sistem` kullanılır).
fn ipc_komutlari(hwnd: HWND) {
    let Some(u) = uyg(hwnd) else { return };
    while let Some(k) = u.ipc_kuyrugu.pop_front() {
        if u.kapaniyor {
            u.ipc_kuyrugu.clear();
            return;
        }
        if !komutu_baslat(u, k, KomutKaynagi::Studyo) {
            return;
        }
    }
}

// --- Kısayollar ve bas-konuş ---

/// Ayarların istediği kısayolları kaydeder (değişmeyenlere dokunulmaz). Başka uygulamanın tuttuğu
/// kısayollar için tek balon.
fn kisayollari_kaydet(u: &mut Uygulama) {
    let istenen = kisayollar::istenenler(&u.ayarlar);
    let r = u.kisayollar.uygula(u.hwnd, &istenen);
    if !r.eklenen.is_empty() {
        gunluk::yaz(&format!("kısayollar kaydedildi: {}", r.eklenen.join(", ")));
    }
    let mut mesgul = r.mesgul;
    mesgul.sort();
    if !mesgul.is_empty() {
        gunluk::yaz(&format!("UYARI: {}", kisayollar::mesgul_metni(&mesgul)));
    }
    let dinleme_kayitli = u.kisayollar.kayitli_mi(&KisayolHedefi::Dinleme);
    let (_, dusuk) = kisayollar::etkin_kip(u.ayarlar.dinleme_kipi, dinleme_kayitli);
    let tus = u.ayarlar.dinleme_tusu.as_ref().map(|k| kisayollar::gorunum(&k.metin));
    match (u.dinleme_tusu_yok, dusuk) {
        (false, true) => gunluk::yaz("UYARI: dinleme tuşu kaydedilemedi; geçici olarak sürekli kipte (tetik kuralıyla) dinleniyor"),
        (true, false) => gunluk::yaz("dinleme tuşu kaydedildi; tuşla dinlemeye dönüldü"),
        _ => {}
    }
    let bildirim = kisayollar::kayit_bildirimi(&u.mesgul_kisayollar, &mesgul, u.dinleme_tusu_yok, dusuk, tus.as_deref());
    u.mesgul_kisayollar = mesgul;
    u.dinleme_tusu_yok = dusuk;
    if let Some((baslik, metin)) = bildirim {
        gunluk::yaz(&format!("balon: {baslik}: {metin}"));
        balon(u, &baslik, &metin, true);
    }
}

/// `WM_HOTKEY`. Komut kısayolu tetiksiz ve dinleme duraklatılmış olsa da çalışır (kullanıcı tuşa
/// bastı); sessiz saat / tam ekran engeli uygulanmaz.
fn kisayol_basildi(hwnd: HWND, kimlik: i32) {
    let Some(u) = uyg(hwnd) else { return };
    if u.kapaniyor {
        return;
    }
    let Some(hedef) = u.kisayollar.hedef(kimlik).cloned() else { return };
    match hedef {
        KisayolHedefi::Komut(ifade) => {
            let Some(k) = u.ayarlar.komut_bul(&ifade).cloned() else {
                gunluk::yaz("UYARI: kısayolun ifadesi hiçbir komutla eşleşmedi");
                return;
            };
            if u.sistem.is_some() {
                komutu_baslat(u, k, KomutKaynagi::Kisayol);
            }
        }
        KisayolHedefi::Dinleme => tus_penceresi_bas(u),
        KisayolHedefi::Palet => palet_ac(u),
    }
}

/// Dinleme tuşu: tuş penceresini açar ya da (açıksa) kapatır.
fn tus_penceresi_bas(u: &mut Uygulama) {
    match u.tus_penceresi.basildi() {
        BasisKarari::Ac(nesil) => {
            if u.duraklatildi || !u.durum.dinliyor_mu() {
                u.tus_penceresi.hepsini_kapat();
                gunluk::yaz("tuş penceresi açılmadı: dinleme kapalı");
                balon(u, "Echora", "Dinleme kapalı; tuşla dinlemek için önce dinlemeyi sürdürün.", false);
                return;
            }
            let sn = u.ayarlar.dinleme_suresi_sn.clamp(2, 30);
            // SAFETY: pencere bu iş parçacığına ait; aynı kimlikli zamanlayıcı yenilenir.
            let _ = unsafe { SetTimer(Some(u.hwnd), TUS_PENCERESI_ZAMANLAYICI, sn * 1000, None) };
            if let Some(kol) = &u.kol {
                kol.tus_penceresi(Some(nesil));
            }
            gunluk::yaz(&format!("tuş penceresi açıldı ({sn} sn)"));
            simge_guncelle(u);
            // Kısık Kenar Nabzı: "dinliyorum" (efektler kapalıysa ya da bekleyen varsa oynamaz).
            if u.ayarlar.efekt_oynasin_mi() {
                let mut a = u.ayarlar.efekt_ayari(Efekt::KenarNabzi);
                a.siddet *= 0.4;
                efekt_oynat(u, Efekt::KenarNabzi, a, None, 0);
            }
        }
        BasisKarari::Kapat => tus_penceresi_kapandi(u, "ikinci basış"),
    }
}

/// `Some(n)`: yalnız `n` nesilli pencere açıksa kapatır; `None`: açık pencereyi kapatır.
fn tus_penceresi_kapat(u: &mut Uygulama, nesil: Option<u64>, neden: &str) {
    let kapandi = match nesil {
        Some(n) => u.tus_penceresi.kapat(n),
        None => u.tus_penceresi.hepsini_kapat(),
    };
    if kapandi {
        tus_penceresi_kapandi(u, neden);
    }
}

fn tus_penceresi_kapandi(u: &mut Uygulama, neden: &str) {
    // SAFETY: pencere bu iş parçacığına ait.
    let _ = unsafe { KillTimer(Some(u.hwnd), TUS_PENCERESI_ZAMANLAYICI) };
    if let Some(kol) = &u.kol {
        kol.tus_penceresi(None);
    }
    gunluk::yaz(&format!("tuş penceresi kapandı ({neden})"));
    simge_guncelle(u);
}

/// Çift alkış: ayardaki ifadenin komutu tetiksiz çalışır (riskli olsa da).
fn alkis_komutu(u: &mut Uygulama) -> bool {
    if !u.ayarlar.alkis.acik {
        return true;
    }
    match u.ayarlar.komut_bul(&u.ayarlar.alkis.ifade).cloned() {
        Some(k) => {
            gunluk::yaz("çift alkış algılandı");
            komutu_baslat(u, k, KomutKaynagi::Alkis)
        }
        None => {
            gunluk::yaz("UYARI: alkış ifadesi hiçbir komutla eşleşmedi");
            true
        }
    }
}

// --- Zamanlı komutlar ---

fn dolum_kimligi(kimlik: u64) -> usize {
    DOLUM_TABAN + (kimlik as usize % DOLUM_ARALIK)
}

fn ms(d: Duration) -> u32 {
    u32::try_from(d.as_millis()).unwrap_or(u32::MAX).max(1)
}

/// `Eylem::Zamanla`: zamanlayıcıyı kurar ve geri al kaydını ekler (geri al → iptal).
fn zamanlayici_kur(u: &mut Uygulama, dakika: u32, eylem: komut::Eylem, asil: &str, komut_gosterimi: &str) {
    let k = match u.zamanlayicilar.ekle(dakika, eylem, asil, Instant::now(), SystemTime::now()) {
        Ok(k) => k,
        Err(EklemeHatasi::ZatenKurulu(k)) => {
            gunluk::yaz(&format!("zamanlayıcı zaten kurulu: {asil} — {dakika} dk (#{k}); yenisi kurulmadı"));
            balon(u, "Echora", &format!("\"{asil}\" için {dakika} dakikalık zamanlayıcı zaten kurulu."), false);
            return;
        }
        Err(EklemeHatasi::Dolu) => {
            let n = zamanlayici::EN_COK_ZAMANLAYICI;
            gunluk::yaz(&format!("zamanlayıcı kurulmadı: en çok {n} zamanlayıcı ({asil})"));
            balon(u, "Echora: zamanlayıcı kurulmadı", &format!("En çok {n} zamanlayıcı kurulabilir; önce birini iptal edin."), true);
            return;
        }
    };
    let sure = Duration::from_secs(u64::from(dakika) * 60);
    // SAFETY: pencere bu iş parçacığına ait.
    let _ = unsafe { SetTimer(Some(u.hwnd), dolum_kimligi(k), ms(sure), None) };
    u.geri_al.ekle(komut_gosterimi, Kayit::Ters(Ters::Zamanlayici(k)));
    gunluk::yaz(&format!("zamanlayıcı kuruldu: {asil} — {dakika} dk sonra (#{k})"));
    zamanlayici_tik_kur(u);
    simge_guncelle(u);
}

/// İpucu/uyarı tıkı: yalnız zamanlayıcı varken, sıradaki değişime kadar.
fn zamanlayici_tik_kur(u: &mut Uygulama) {
    // SAFETY: pencere bu iş parçacığına ait; aynı kimlikli zamanlayıcı yenilenir.
    unsafe {
        match u.zamanlayicilar.sonraki_tik(Instant::now()) {
            Some(d) => {
                let _ = SetTimer(Some(u.hwnd), ZAMANLAYICI_TIK, ms(d), None);
            }
            None => {
                let _ = KillTimer(Some(u.hwnd), ZAMANLAYICI_TIK);
            }
        }
    }
}

/// Listeden çıkmış zamanlayıcının Win32 zamanlayıcısını ve geri al kaydını temizler.
fn zamanlayici_temizle(u: &mut Uygulama, z: &Zamanlayici) {
    // SAFETY: pencere bu iş parçacığına ait.
    let _ = unsafe { KillTimer(Some(u.hwnd), dolum_kimligi(z.kimlik)) };
    let kimlik = z.kimlik;
    u.geri_al.sil(|k| *k == Kayit::Ters(Ters::Zamanlayici(kimlik)));
}

fn zamanlayici_iptal(u: &mut Uygulama, kimlik: u64, neden: &str) -> bool {
    let Some(z) = u.zamanlayicilar.iptal(kimlik) else { return false };
    zamanlayici_temizle(u, &z);
    gunluk::yaz(&format!("zamanlayıcı iptal edildi: {} (#{}, {neden})", z.gosterim, z.kimlik));
    zamanlayici_tik_kur(u);
    simge_guncelle(u);
    true
}

/// Tümünü iptal eder; iptal edilen sayısını döner.
fn zamanlayicilari_iptal(u: &mut Uygulama, neden: &str) -> usize {
    let liste = u.zamanlayicilar.hepsini_iptal();
    for z in &liste {
        zamanlayici_temizle(u, z);
    }
    if liste.is_empty() {
        gunluk::yaz(&format!("iptal edilecek zamanlayıcı yok ({neden})"));
    } else {
        let adlar: Vec<&str> = liste.iter().map(|z| z.gosterim.as_str()).collect();
        gunluk::yaz(&format!("zamanlayıcılar iptal edildi ({}): {} ({neden})", liste.len(), adlar.join(", ")));
    }
    zamanlayici_tik_kur(u);
    simge_guncelle(u);
    liste.len()
}

/// Dakika değişimi (ipucu) ve son dakika uyarısı.
fn zamanlayici_tik(hwnd: HWND) {
    // SAFETY: pencere bu iş parçacığına ait.
    let _ = unsafe { KillTimer(Some(hwnd), ZAMANLAYICI_TIK) };
    let Some(u) = uyg(hwnd) else { return };
    if u.kapaniyor {
        return;
    }
    // Dolum zamanlayıcısı gelmemiş olsa da (kaçırılan WM_TIMER) süresi dolanlar burada da çalışır.
    if !dolanlari_calistir(u) {
        return;
    }
    for (kimlik, gosterim) in u.zamanlayicilar.uyarilacaklar(Instant::now()) {
        gunluk::yaz(&format!("zamanlayıcı 1 dakika içinde dolacak: {gosterim} (#{kimlik})"));
        balon(u, "Echora: zamanlayıcı", &zamanlayici::uyari_metni(&gosterim), false);
    }
    simge_guncelle(u);
    zamanlayici_tik_kur(u);
}

/// Bir dolum zamanlayıcısı uyandı: süresi dolanların (birikmiş olanlar dahil) asıl eylemi normal
/// yoldan (efektiyle) çalışır. Uykudan dolayı çok geç kalan çalıştırılmaz.
fn zamanlayici_doldu(hwnd: HWND, kimlik_win: usize) {
    // SAFETY: pencere bu iş parçacığına ait.
    let _ = unsafe { KillTimer(Some(hwnd), kimlik_win) };
    let Some(u) = uyg(hwnd) else { return };
    if u.kapaniyor {
        return;
    }
    let simdi = Instant::now();
    let dolanlar = u.zamanlayicilar.dolanlar(simdi);
    // Bu Win32 zamanlayıcısının sahibi henüz dolmadıysa (erken uyanma) kalan süreyle yeniden kur.
    let erken: Vec<Duration> = u
        .zamanlayicilar
        .sirali()
        .iter()
        .filter(|z| dolum_kimligi(z.kimlik) == kimlik_win)
        .map(|z| z.bitis.saturating_duration_since(simdi))
        .collect();
    for kalan in erken {
        // SAFETY: yukarıdakiyle aynı.
        let _ = unsafe { SetTimer(Some(hwnd), kimlik_win, ms(kalan), None) };
    }
    if !dolanlari_isle(u, dolanlar) {
        return;
    }
    zamanlayici_tik_kur(u);
    simge_guncelle(u);
}

/// Süresi dolanları listeden alıp çalıştırır. `false`: çıkış başladı (pencere yok edildi).
fn dolanlari_calistir(u: &mut Uygulama) -> bool {
    let dolanlar = u.zamanlayicilar.dolanlar(Instant::now());
    dolanlari_isle(u, dolanlar)
}

fn dolanlari_isle(u: &mut Uygulama, dolanlar: Vec<Zamanlayici>) -> bool {
    for z in dolanlar {
        zamanlayici_temizle(u, &z);
        match zamanlayici::dolum_karari(z.bitis_duvar, SystemTime::now()) {
            DolumKarari::Gecikti => {
                gunluk::yaz(&format!("zamanlayıcı çok gecikti (uyku?); çalıştırılmadı: {}", z.gosterim));
                balon(
                    u,
                    "Echora: zamanlayıcı çalıştırılmadı",
                    &format!("\"{}\" zamanı bilgisayar uykudayken geçti; çalıştırılmadı.", z.gosterim),
                    false,
                );
            }
            DolumKarari::Calistir => {
                gunluk::yaz(&format!("zamanlayıcı doldu: {} (#{})", z.gosterim, z.kimlik));
                let k = zamanlayici::asil_komut(&u.ayarlar, &z.eylem, &z.gosterim);
                if u.sistem.is_some() && !komutu_baslat(u, k, KomutKaynagi::Zamanlayici) {
                    return false;
                }
            }
        }
    }
    true
}

// --- Dinleyici iletileri ---

/// `false` donerse kuyrugun kalani atilmalidir (cikis basladi).
fn ileti_isle(hwnd: HWND, ileti: UiIleti) -> bool {
    let Some(u) = uyg(hwnd) else { return true };
    match ileti {
        UiIleti::Durum(d) => durum_ayarla(u, d),
        UiIleti::Balon { baslik, metin, hata } => balon(u, &baslik, &metin, hata),
        UiIleti::Olay(o) => {
            if !u.duraklatildi {
                olay_oynat(u, o);
            }
        }
        UiIleti::Komut { komut, olusma, pencere } => {
            // Tuş penceresinde ilk tanınan komut pencereyi kapatır (eski pencerenin komutu yenisini
            // değil) — komut bayat ya da duraklatılmış diye atılsa da.
            if let Some(n) = pencere {
                tus_penceresi_kapat(u, Some(n), "komut tanındı");
            }
            match komut_karari(olusma, Instant::now(), u.duraklatildi || u.kapaniyor) {
                KomutKarari::Bayat => {
                    gunluk::yaz(&format!("bayat komut atıldı: {}", komut.gosterim));
                    return !u.kapaniyor;
                }
                KomutKarari::Duraklatilmis => return !u.kapaniyor,
                KomutKarari::Calistir => {}
            }
            if u.sistem.is_none() {
                return true;
            }
            return komutu_baslat(u, komut, KomutKaynagi::Ses);
        }
        UiIleti::PencereKapandi(n) => tus_penceresi_kapat(u, Some(n), "komut yinelendi"),
        UiIleti::Alkis { olusma } => {
            match komut_karari(olusma, Instant::now(), u.duraklatildi || u.kapaniyor) {
                KomutKarari::Bayat => {
                    gunluk::yaz("bayat çift alkış atıldı");
                    return !u.kapaniyor;
                }
                KomutKarari::Duraklatilmis => return !u.kapaniyor,
                KomutKarari::Calistir => {}
            }
            if u.sistem.is_none() {
                return true;
            }
            return alkis_komutu(u);
        }
    }
    true
}

unsafe extern "system" fn pencere_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TEPSI => {
            match (lp.0 as u32) & 0xFFFF {
                WM_LBUTTONUP => {
                    if let Some(u) = uyg(hwnd) {
                        durdur_surdur(u);
                    }
                }
                WM_RBUTTONUP => menu_goster(hwnd),
                // Balona tıklama: yalnız "indirildi, kurmak için tıklayın" balonuysa kurulum onayı.
                NIN_BALLOONUSERCLICK => {
                    let guncelleme = uyg(hwnd).is_some_and(|u| std::mem::take(&mut u.guncelleme_balonu));
                    if guncelleme {
                        guncellemeyi_kur(hwnd);
                    }
                }
                NIN_BALLOONTIMEOUT | NIN_BALLOONHIDE => {
                    if let Some(u) = uyg(hwnd) {
                        u.guncelleme_balonu = false;
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_GUNCELLEME => {
            guncelleme_sonucu(hwnd);
            LRESULT(0)
        }
        WM_GUNCELLEME_KUR => {
            guncellemeyi_kur(hwnd);
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            if wp.0 as u32 == PBT_APMRESUMEAUTOMATIC {
                uyku_donusu(hwnd);
            }
            LRESULT(1)
        }
        WM_TIMER if wp.0 == GUNCELLEME_ZAMANLAYICI => {
            guncelleme_zamani(hwnd);
            LRESULT(0)
        }
        WM_HOTKEY => {
            kisayol_basildi(hwnd, wp.0 as i32);
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TUS_PENCERESI_ZAMANLAYICI => {
            let _ = KillTimer(Some(hwnd), TUS_PENCERESI_ZAMANLAYICI);
            if let Some(u) = uyg(hwnd) {
                if let Some(n) = u.tus_penceresi.acik() {
                    tus_penceresi_kapat(u, Some(n), "süre doldu");
                }
            }
            LRESULT(0)
        }
        WM_TIMER if wp.0 == ZAMANLAYICI_TIK => {
            zamanlayici_tik(hwnd);
            LRESULT(0)
        }
        WM_TIMER if (DOLUM_TABAN..DOLUM_TABAN + DOLUM_ARALIK).contains(&wp.0) => {
            zamanlayici_doldu(hwnd, wp.0);
            LRESULT(0)
        }
        WM_DINLEYICI => {
            // Kuyruk her seferinde yeniden kilitlenir; eylem çalışırken kilit tutulmaz.
            let kuyruk = uyg(hwnd).and_then(|u| u.ui.clone());
            if let Some(k) = kuyruk {
                while let Some(i) = k.al() {
                    if !ileti_isle(hwnd, i) {
                        // Cikis basladi: ayni turdaki kalan iletiler calistirilmaz.
                        while k.al().is_some() {}
                        break;
                    }
                }
            }
            LRESULT(0)
        }
        WM_EFEKT_BITTI => {
            efekt_bitti(hwnd, wp.0 as u64);
            LRESULT(0)
        }
        WM_SAHNE_ADIMI => {
            if let Some(u) = uyg(hwnd) {
                if u.sahne.is_some() && !u.sahne_bekliyor && wp.0 == u.sahne_nesil as usize {
                    sahne_ilerle(u);
                }
            }
            LRESULT(0)
        }
        WM_AYAR_DEGISTI => {
            if let Some(u) = uyg(hwnd) {
                if !u.kapaniyor {
                    ayarlari_yeniden_yukle(u, if wp.0 == 1 { Kaynak::Studyo } else { Kaynak::Dosya });
                }
            }
            LRESULT(0)
        }
        WM_COPYDATA => ipc_al(hwnd, lp),
        WM_IPC_KOMUT => {
            ipc_komutlari(hwnd);
            LRESULT(0)
        }
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION => {
            if wp.0 != 0 {
                // Oturum kapaniyor: Windows kisa sure tanir. ONCE perdeler kalksin / monitorler
                // acilsin (sistem dusurulur), sonra efekt ve dinleyici durdurulsun, sonra pencere yok edilsin.
                // Efekt motoru ve dinleyici BEKLENMEZ (süreç zaten sonlanacak; bloklanmasın).
                if let Some(u) = uyg(hwnd) {
                    cikis_hazirla(u);
                    u.oturum_bitiyor = true;
                    u.sistem = None;
                }
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_TIMER if wp.0 == SAHNE_ZAMANLAYICI => {
            let _ = KillTimer(Some(hwnd), SAHNE_ZAMANLAYICI);
            if let Some(u) = uyg(hwnd) {
                if u.sahne_bekliyor {
                    u.sahne_bekliyor = false;
                    sahne_ilerle(u);
                }
            }
            LRESULT(0)
        }
        WM_TIMER if wp.0 == BEKLEYEN_ZAMANLAYICI => {
            let _ = KillTimer(Some(hwnd), BEKLEYEN_ZAMANLAYICI);
            if let Some(u) = uyg(hwnd) {
                bekleyen_suresi_doldu(u);
            }
            LRESULT(0)
        }
        WM_TIMER if wp.0 == SIMGE_ZAMANLAYICI => {
            simge_yeniden_dene(hwnd);
            LRESULT(0)
        }
        WM_TIMER if wp.0 == ZAMANLAYICI_SURE => {
            let _ = KillTimer(Some(hwnd), ZAMANLAYICI_SURE);
            if let Some(u) = uyg(hwnd) {
                cikis_hazirla(u);
            }
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Some(u) = uyg(hwnd) {
                cikis_hazirla(u);
                u.kisayollar.hepsini_birak(hwnd);
                simge_sil(u);
            }
            PostQuitMessage(0);
            LRESULT(0)
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            if let Some(u) = uyg(hwnd) {
                simge_ekle(u);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Gizli pencereyi oluşturur, durum işaretçisini bağlar, simgeyi ekler.
///
/// `u` işaretçisi pencere yaşadığı sürece (mesaj döngüsü bitene dek) geçerli kalmalıdır.
pub fn pencere_olustur(u: *mut Uygulama) -> Result<HWND, String> {
    // SAFETY: sınıf adı ve metinler 'static; işaretçi çağıranın sözleşmesiyle geçerli.
    unsafe {
        let ornek = GetModuleHandleW(None).map_err(|h| h.to_string())?;
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);
        let sinif_adi = if (*u).kuru { SINIF_KURU } else { SINIF_GERCEK };
        let sinif = WNDCLASSW {
            lpfnWndProc: Some(pencere_proc),
            hInstance: ornek.into(),
            lpszClassName: sinif_adi,
            ..Default::default()
        };
        if RegisterClassW(&sinif) == 0 {
            return Err("pencere sınıfı kaydedilemedi".into());
        }
        // Üst düzey, görünmeyen pencere: ileti-yalnız pencereler "TaskbarCreated" yayınını almaz.
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            sinif_adi,
            w!("Echora"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(ornek.into()),
            None,
        )
        .map_err(|h| h.to_string())?;
        (*u).hwnd = hwnd;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, u as isize);
        simge_ekle(&mut *u);
        // Simgeden sonra: "kısayol kullanılıyor" balonu gösterilebilsin.
        kisayollari_kaydet(&mut *u);
        Ok(hwnd)
    }
}

/// Ayar dosyası izleyicisinin geri çağrısı: UI'ya "dosya değişti" iletisi atar.
pub fn ayar_degisti_bildirici(hwnd: HWND) -> impl Fn() + Send + 'static {
    let hedef = hwnd.0 as isize;
    move || {
        // SAFETY: PostMessageW her iş parçacığından çağrılabilir; pencere yoksa hata döner.
        let _ = unsafe { PostMessageW(Some(HWND(hedef as *mut _)), WM_AYAR_DEGISTI, WPARAM(0), LPARAM(0)) };
    }
}

/// `--sure` için tek seferlik zamanlayıcı; dolunca pencere yok edilir (temiz çıkış).
pub fn sure_ayarla(hwnd: HWND, saniye: u32) {
    // SAFETY: pencere bu iş parçacığına ait; zamanlayıcı WM_TIMER olarak ona düşer.
    let _ = unsafe { SetTimer(Some(hwnd), ZAMANLAYICI_SURE, saniye.saturating_mul(1000), None) };
}

/// Pencere yok edilene kadar mesajları işler. Bloklayıcıdır: boşta uyanma yoktur.
pub fn mesaj_dongusu() {
    let mut m = MSG::default();
    // SAFETY: standart Win32 mesaj döngüsü.
    unsafe {
        while GetMessageW(&mut m, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }
}

/// Başlangıçta (mesaj döngüsünden önce) balon göstermek için.
pub fn bildirim_balonu(hwnd: HWND, baslik: &str, metin: &str, hata: bool) {
    if let Some(u) = uyg(hwnd) {
        balon(u, baslik, metin, hata);
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    fn metin(d: &[u16]) -> String {
        let n = d.iter().position(|&u| u == 0).unwrap();
        String::from_utf16(&d[..n]).unwrap()
    }

    #[test]
    fn dizi_doldur_sigdigi_kadar_yazar() {
        let mut d = [0xFFFFu16; 6];
        dizi_doldur(&mut d, "abc");
        assert_eq!(metin(&d), "abc");
        let mut d = [0xFFFFu16; 3];
        dizi_doldur(&mut d, "abcdef");
        assert_eq!(metin(&d), "ab");
    }

    #[test]
    fn dizi_doldur_vekil_ciftini_ortadan_kesmez() {
        // "a" + U+1F600 (iki birim): 3 yuvada a, yuksek vekil, NUL olurdu; yarim cift atilir.
        let mut d = [0xFFFFu16; 3];
        dizi_doldur(&mut d, "a\u{1F600}");
        assert_eq!(metin(&d), "a");
        // Yeterli yer varsa cift tam yazilir.
        let mut d = [0xFFFFu16; 4];
        dizi_doldur(&mut d, "a\u{1F600}");
        assert_eq!(metin(&d), "a\u{1F600}");
    }

    #[test]
    fn ipuclari_tepsi_sinirina_sigar() {
        // szTip 128 UTF-16 birimi (NUL dahil).
        for d in [
            Durum::Dinliyor,
            Durum::Duraklatildi,
            Durum::MikrofonYok,
            Durum::ModelYok,
            Durum::DinleyiciYok,
            Durum::SessizSaat,
            Durum::TamEkran,
        ] {
            assert!(ipucu(d).encode_utf16().count() < 128);
        }
        assert_eq!(ipucu(Durum::SessizSaat), "Echora — sessiz saat");
    }

    #[test]
    fn ipucu_tusla_pencere_ve_zamanlayici() {
        assert_eq!(ipucu_olustur(Durum::Dinliyor, None, false, false, None), "Echora — dinliyor");
        assert_eq!(
            ipucu_olustur(Durum::Dinliyor, Some("Ctrl+Alt+Boşluk"), false, false, None),
            "Echora — tuşla dinleme (Ctrl+Alt+Boşluk)"
        );
        assert_eq!(ipucu_olustur(Durum::Dinliyor, Some("Ctrl+Alt+Boşluk"), false, true, None), "Echora — dinliyor…");
        assert_eq!(ipucu_olustur(Durum::Dinliyor, None, false, true, None), "Echora — dinliyor…");
        // Tuşla kipinde tuş kaydedilemedi: sürekli dinlendiği söylenir.
        assert_eq!(
            ipucu_olustur(Durum::Dinliyor, Some("Ctrl+Alt+Boşluk"), true, false, None),
            "Echora — dinleme tuşu kullanılamıyor; sürekli dinleniyor"
        );
        // Duraklatılmışken tuşla kip metni gösterilmez.
        assert_eq!(ipucu_olustur(Durum::Duraklatildi, Some("Ctrl+Alt+Boşluk"), false, false, None), "Echora — duraklatıldı");
        let z = ipucu_olustur(Durum::Dinliyor, None, false, false, Some("⏲ Bilgisayarı uyut — 29 dk".into()));
        assert_eq!(z, "Echora — dinliyor\n⏲ Bilgisayarı uyut — 29 dk");
        assert!(z.encode_utf16().count() < 128);
    }

    #[test]
    fn dolum_kimlikleri_aralikta() {
        for k in [1u64, 2, 0xFFF, 0x1000, 0x1001, u64::MAX] {
            let id = dolum_kimligi(k);
            assert!((DOLUM_TABAN..DOLUM_TABAN + DOLUM_ARALIK).contains(&id), "{k}");
            assert!(![ZAMANLAYICI_SURE, SIMGE_ZAMANLAYICI, SAHNE_ZAMANLAYICI, BEKLEYEN_ZAMANLAYICI, GUNCELLEME_ZAMANLAYICI, ZAMANLAYICI_TIK, TUS_PENCERESI_ZAMANLAYICI].contains(&id));
        }
        assert_eq!(ms(Duration::ZERO), 1);
        assert_eq!(ms(Duration::from_secs(300)), 300_000);
    }
}
