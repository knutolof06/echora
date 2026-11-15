//! Sahne iş parçacığının durumu: aygıt, katmanlar, oynayan efekt. Yalnız sahne iş parçacığında yaşar.
//!
//! Yaşam döngüsü: `baslat` → her döngüde `kare` (Oynuyor) → süre dolunca `bitir`:
//! saydam son kare + gizle + `bitince` (opak biten efektte: siyah son kare + `bitince` +
//! kısa tutma, sonra gizle). Boşta kalınca `birak` her şeyi serbest bırakır.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::Win32::Foundation::{LPARAM, POINT, RECT, WAIT_OBJECT_0};
use windows::Win32::Graphics::Direct3D11::{ID3D11PixelShader, ID3D11ShaderResourceView, ID3D11Texture2D};
use windows::Win32::Graphics::DirectComposition::{DCompositionCreateDevice, IDCompositionDevice};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows::Win32::System::Threading::{WaitForMultipleObjects, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, SystemParametersInfoW, MONITORINFOF_PRIMARY, SPI_GETCLIENTAREAANIMATION,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

use crate::efektler;
use crate::gpu::Gpu;
use crate::goruntu;
use crate::hata;
use crate::metin::{MetinCizici, MetinDokusu};
use crate::pencere::Katman;
use crate::sabitler::Sabitler;
use crate::secim::{
    efekt_cozumle, hedef_alanlari, imlec_yerel, sanal_alan, seviye_carpani, seviye_normallestir, yumusat, Dikdortgen,
    Ekran,
    SeviyeGecmisi, GECMIS_UZUNLUGU,
};
use crate::{Efekt, EfektAyari, Istek};

pub(crate) type Bitince = Box<dyn FnOnce() + Send>;

/// Opak biten efektin son karesinin tutulma süresi: `bitince` (ör. monitörü kapat) işini yaparken
/// masaüstü bir an geri görünmesin.
const OPAK_TUTMA: Duration = Duration::from_millis(1000);
/// Kare bekleme nesnesi için üst sınır (ms); örtülü/gizli ekranda sonsuza beklemeyelim.
const KARE_BEKLEME_MS: u32 = 50;
/// Sunum hiç beklemediğinde (ör. ekran kapalı) döngü boşa dönmesin diye en kısa kare süresi.
const EN_KISA_KARE: Duration = Duration::from_millis(4);
/// Bir katman bu süre boyunca hiç kare sunamadıysa askıda sayılır (beklenmez); hepsi askıdaysa
/// efekt bitmiş sayılır (`bitince` + kaynak bırakma).
const ASKI_SURESI: Duration = Duration::from_millis(500);
/// Opak son karenin ekrana ulaşması için her adımda en çok bekleme.
const SON_KARE_BEKLEME: Duration = Duration::from_millis(50);
/// Masaüstü yakalamadan önce, overlay'in saydam karesinin birleştirilmesi için bekleme (~2 tarama).
const GIZLEME_PAYI: Duration = Duration::from_millis(34);

struct KatmanVerisi {
    goruntu: Option<(ID3D11Texture2D, ID3D11ShaderResourceView)>,
    metin: Option<MetinDokusu>,
}

struct Oynayan {
    efekt: Efekt,
    ayar: EfektAyari,
    sure: Duration,
    baslangic: Instant,
    son_kare: Instant,
    bitince: Option<Bitince>,
    ps: ID3D11PixelShader,
    /// `Motor::katmanlar` ile aynı sırada.
    veriler: Vec<KatmanVerisi>,
    sanal: Dikdortgen,
    tohum: f32,
    gecmis: SeviyeGecmisi,
    yumusak: f32,
    gosterildi: bool,
}

enum Durum {
    Bos,
    Oynuyor(Box<Oynayan>),
    /// Opak biten efektin son karesi ekranda; bu ana kadar tutulur.
    Tutuyor(Instant),
}

pub(crate) struct Motor {
    seviye: Arc<AtomicU32>,
    gpu: Option<Gpu>,
    dcomp: Option<IDCompositionDevice>,
    metin: Option<MetinCizici>,
    katmanlar: Vec<Katman>,
    durum: Durum,
}

fn renk(r: komut::efekt::Renk) -> [f32; 4] {
    let k = r.kesir();
    [k[0], k[1], k[2], 1.0]
}

/// Windows "animasyon efektlerini göster" ayarı (okunamazsa açık sayılır).
pub(crate) fn animasyon_acik() -> bool {
    let mut b = windows::core::BOOL(1);
    // SAFETY: çıktı yerel BOOL'a.
    let r = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut b as *mut _ as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    r.map(|_| b.as_bool()).unwrap_or(true)
}

unsafe extern "system" fn ekran_topla(hm: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> windows::core::BOOL {
    // SAFETY: lp, `ekranlari_listele`'deki Vec'in adresi; geri çağırma onun ömrü içinde çalışır.
    let liste = unsafe { &mut *(lp.0 as *mut Vec<Ekran>) };
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    // SAFETY: hm sistemin verdiği tutamaç; mi geçerli.
    if unsafe { GetMonitorInfoW(hm, &mut mi) }.as_bool() {
        let r = mi.rcMonitor;
        liste.push(Ekran {
            alan: Dikdortgen::yeni(r.left, r.top, r.right - r.left, r.bottom - r.top),
            ana: mi.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
    }
    true.into()
}

/// Monitörler (fiziksel piksel; çağıran iş parçacığı DPI farkında olmalı).
pub(crate) fn ekranlari_listele() -> Vec<Ekran> {
    let mut liste: Vec<Ekran> = Vec::new();
    // SAFETY: geri çağırma yalnız bu çağrı süresince liste'ye yazar.
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(ekran_topla), LPARAM(&mut liste as *mut _ as isize));
    }
    liste
}

fn imlec_konumu() -> (i32, i32) {
    let mut p = POINT::default();
    // SAFETY: çıktı yerel.
    match unsafe { GetCursorPos(&mut p) } {
        Ok(()) => (p.x, p.y),
        Err(_) => (i32::MIN / 2, i32::MIN / 2),
    }
}

fn tohum_uret() -> f32 {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E37_79B9);
    // splitmix64
    let mut z = n.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

impl Oynayan {
    fn sabitler(&self, alan: Dikdortgen, t: f32, gecen_sn: f32, imlec: (i32, i32), veri: &KatmanVerisi) -> Sabitler {
        let mut s = Sabitler::sifir();
        s.zaman = t.clamp(0.0, 1.0);
        s.sure_sn = self.sure.as_secs_f32();
        s.cozunurluk = [alan.genislik as f32, alan.yukseklik as f32];
        s.renk1 = renk(self.ayar.renk1);
        s.renk2 = renk(self.ayar.renk2);
        s.siddet = if self.ayar.siddet.is_finite() { self.ayar.siddet.clamp(0.0, 1.0) } else { 0.7 };
        s.seviye = seviye_carpani(self.yumusak, self.ayar.sese_tepki);
        s.imlec = imlec_yerel(alan, imlec);
        s.ekran_ofseti = [(alan.sol - self.sanal.sol) as f32, (alan.ust - self.sanal.ust) as f32];
        s.sanal_boyut = [self.sanal.genislik as f32, self.sanal.yukseklik as f32];
        s.tohum = self.tohum;
        s.goruntu_var = if veri.goruntu.is_some() { 1.0 } else { 0.0 };
        s.metin_var = if veri.metin.is_some() { 1.0 } else { 0.0 };
        s.gecen_sn = gecen_sn;
        s.metin_kutusu = veri.metin.as_ref().map(|m| m.kutu).unwrap_or([0.0; 4]);
        s.dalga = if self.ayar.sese_tepki { self.gecmis.sirali() } else { [0.5; GECMIS_UZUNLUGU] };
        s
    }

    fn bitince_cagir(&mut self) {
        if let Some(b) = self.bitince.take() {
            b();
        }
    }
}

impl Motor {
    pub fn yeni(seviye: Arc<AtomicU32>) -> Self {
        Motor { seviye, gpu: None, dcomp: None, metin: None, katmanlar: Vec::new(), durum: Durum::Bos }
    }

    pub fn oynuyor_mu(&self) -> bool {
        matches!(self.durum, Durum::Oynuyor(_))
    }

    pub fn tutma_bitisi(&self) -> Option<Instant> {
        match self.durum {
            Durum::Tutuyor(t) => Some(t),
            _ => None,
        }
    }

    /// Yeni efekt başlatır; sürmekte olan efekt kesilir (`bitince`'i hemen çağrılır).
    pub fn baslat(&mut self, istek: Istek, bitince: Option<Bitince>) {
        if let Durum::Oynuyor(mut o) = std::mem::replace(&mut self.durum, Durum::Bos) {
            o.bitince_cagir();
        }
        let (efekt, sure_ms) =
            efekt_cozumle(istek.efekt, istek.ayar.sure_ms, animasyon_acik(), |e| efektler::kaynak(e).is_some());
        match self.hazirla(efekt, sure_ms, &istek) {
            Ok(mut o) => {
                o.bitince = bitince;
                self.durum = Durum::Oynuyor(Box::new(o));
            }
            Err(e) => {
                hata::kaydet(e);
                self.hepsini_gizle();
                if let Some(b) = bitince {
                    b();
                }
            }
        }
    }

    fn aygit_hazirla(&mut self) -> Result<(), String> {
        if self.gpu.is_none() {
            self.gpu = Some(Gpu::olustur(false)?);
        }
        if self.dcomp.is_none() {
            let gpu = self.gpu.as_ref().ok_or("aygıt yok")?;
            let dxgi: IDXGIDevice = gpu.aygit.cast().map_err(|e| format!("DXGI aygıtı: {e}"))?;
            // SAFETY: geçerli DXGI aygıtı.
            let d: IDCompositionDevice =
                unsafe { DCompositionCreateDevice(&dxgi) }.map_err(|e| format!("DirectComposition aygıtı: {e}"))?;
            self.dcomp = Some(d);
        }
        Ok(())
    }

    fn hazirla(&mut self, efekt: Efekt, sure_ms: u32, istek: &Istek) -> Result<Oynayan, String> {
        let ekranlar = ekranlari_listele();
        let imlec = imlec_konumu();
        let alanlar = hedef_alanlari(istek.hedef, &ekranlar, imlec);
        if alanlar.is_empty() {
            return Err("efekt için hedef ekran bulunamadı".into());
        }
        let ilk = alanlar[0];
        let sanal = sanal_alan(&ekranlar, ilk);

        self.aygit_hazirla()?;
        let ps = self.gpu.as_mut().ok_or("aygıt yok")?.piksel_golgelendirici(efekt)?;

        // Masaüstü görüntüsü, kendi overlay'imiz görünmezken alınmalı. `hepsini_gizle` önce tam
        // saydam bir kare sunar, sonra pencereyi gizler; DWM'in bunu bir kez birleştirmesi için
        // SINIRLI bir bekleme yeter (~2 dikey tarama). `DwmFlush` kullanılmaz: DWM bir sonraki
        // kompozisyonu yapana kadar bloklar ve ekran kapalı/oturum kilitliyken bu süresiz olabilir.
        let goruntu_ister = efekt.masaustu_goruntusu_ister();
        if goruntu_ister && self.katmanlar.iter().any(|k| k.gorunur_mu()) {
            self.hepsini_gizle();
            std::thread::sleep(GIZLEME_PAYI);
        }

        // Katmanları eşle: aynı dikdörtgendekiler yeniden kullanılır, artakalanlar yok edilir.
        let mut eski = std::mem::take(&mut self.katmanlar);
        let mut yeni = Vec::with_capacity(alanlar.len());
        {
            let gpu = self.gpu.as_ref().ok_or("aygıt yok")?;
            let dcomp = self.dcomp.as_ref().ok_or("DirectComposition yok")?;
            for alan in &alanlar {
                if let Some(i) = eski.iter().position(|k| k.alan == *alan) {
                    yeni.push(eski.swap_remove(i));
                    continue;
                }
                match Katman::olustur(gpu, dcomp, *alan) {
                    Ok(k) => yeni.push(k),
                    Err(e) => hata::kaydet(e),
                }
            }
        }
        drop(eski);
        self.katmanlar = yeni;
        if self.katmanlar.is_empty() {
            return Err(hata::son_hata().unwrap_or_else(|| "overlay penceresi açılamadı".into()));
        }

        let metin = istek.metin.as_deref().map(str::trim).filter(|m| !m.is_empty());
        if efekt.metin_ister() && metin.is_some() && self.metin.is_none() {
            let gpu = self.gpu.as_ref().ok_or("aygıt yok")?;
            match MetinCizici::olustur(gpu) {
                Ok(m) => self.metin = Some(m),
                Err(e) => hata::kaydet(e),
            }
        }

        let gpu = self.gpu.as_ref().ok_or("aygıt yok")?;
        let mut veriler = Vec::with_capacity(self.katmanlar.len());
        // Tüm ekranların yakalanması için tek, toplam bütçe.
        let yakalama_sonu = Instant::now() + goruntu::YAKALAMA_BUTCESI;
        for k in &self.katmanlar {
            let goruntu = if goruntu_ister {
                match goruntu::yakala(gpu, k.alan, yakalama_sonu) {
                    Ok(g) => Some(g),
                    Err(e) => {
                        hata::kaydet(e);
                        None
                    }
                }
            } else {
                None
            };
            let metin_dokusu = match (efekt.metin_ister(), metin, &self.metin) {
                (true, Some(m), Some(c)) => {
                    let (w, h) = k.boyut();
                    match c.ciz(gpu, m, w, h) {
                        Ok(d) => Some(d),
                        Err(e) => {
                            hata::kaydet(e);
                            None
                        }
                    }
                }
                _ => None,
            };
            veriler.push(KatmanVerisi { goruntu, metin: metin_dokusu });
        }

        let simdi = Instant::now();
        for k in &mut self.katmanlar {
            k.son_sunum = simdi;
        }
        Ok(Oynayan {
            efekt,
            ayar: istek.ayar,
            sure: Duration::from_millis(sure_ms as u64),
            baslangic: simdi,
            son_kare: simdi,
            bitince: None,
            ps,
            veriler,
            sanal,
            tohum: tohum_uret(),
            gecmis: SeviyeGecmisi::yeni(),
            yumusak: 0.0,
            gosterildi: false,
        })
    }

    /// Kare temposu: askıda olmayan katmanların takas zincirleri yeni kare kabul edene kadar en çok
    /// `KARE_BEKLEME_MS` beklenir (dikey tarama eşlemesi). Sonuç YALNIZ tempo içindir; bir katmanın
    /// kareyi gerçekten kabul edip etmediğini beklemesiz `Present` söyler (`Katman::sun`). Bekleme
    /// sonucuna güvenilmez: bekleme nesnesi her sunuma bir sinyal veren bir sayaçtır, tek bir
    /// fazladan bekleme (ör. opak son karenin ekrana ulaşmasını beklerken) sonraki tüm beklemeleri
    /// zaman aşımına düşürürdü. Askıdaki (uzun süredir kare kabul etmeyen) katman beklenmez:
    /// kapalı bir ekran diğerlerini 20 kare/sn'ye düşürmesin.
    fn kare_temposu(&self, simdi: Instant) {
        let tutamaclar: Vec<_> = self
            .katmanlar
            .iter()
            .filter(|k| simdi.saturating_duration_since(k.son_sunum) < ASKI_SURESI)
            .map(|k| k.bekleme)
            .collect();
        if !tutamaclar.is_empty() {
            // SAFETY: tutamaçlar katmanlara ait ve geçerli; bekleme sınırlı.
            let _ = unsafe { WaitForMultipleObjects(&tutamaclar, true, KARE_BEKLEME_MS) };
        }
    }

    /// Bir kare çizer ve sunar; süre dolduysa efekti bitirir. Hiçbir adımı süresiz bloklamaz:
    /// sunum beklemesizdir, kare kabul etmeyen katmanın karesi atlanır; hiçbir katman
    /// `ASKI_SURESI` boyunca kare sunamadıysa (ekran kapalı / oturum kilitli) efekt bitmiş sayılır.
    pub fn kare(&mut self) {
        let kare_basi = Instant::now();
        self.kare_temposu(kare_basi);

        let simdi = Instant::now();
        let Durum::Oynuyor(o) = &mut self.durum else { return };
        let gecen = simdi.saturating_duration_since(o.baslangic);
        let t = gecen.as_secs_f32() / o.sure.as_secs_f32().max(0.001);
        if t >= 1.0 {
            self.bitir();
            return;
        }
        if self.katmanlar.iter().all(|k| simdi.saturating_duration_since(k.son_sunum) >= ASKI_SURESI) {
            hata::kaydet(format!(
                "efekt kesildi: {} ms boyunca hiçbir ekran kare kabul etmedi (ekran kapalı ya da oturum kilitli olabilir)",
                ASKI_SURESI.as_millis()
            ));
            // `birak` bekleyen `bitince`'i çağırır ve tüm kaynakları bırakır (sunum yapmadan).
            self.birak();
            return;
        }
        let Durum::Oynuyor(o) = &mut self.durum else { return };
        let dt = simdi.saturating_duration_since(o.son_kare).as_secs_f32();
        o.son_kare = simdi;
        let ham = seviye_normallestir(f32::from_bits(self.seviye.load(Ordering::Relaxed)));
        o.yumusak = yumusat(o.yumusak, ham, dt);
        o.gecmis.ilerle(dt, o.yumusak);
        let imlec = imlec_konumu();

        let Some(gpu) = self.gpu.as_mut() else { return };
        for (k, v) in self.katmanlar.iter().zip(&o.veriler) {
            let sabit = o.sabitler(k.alan, t, gecen.as_secs_f32(), imlec, v);
            let t0 = v.goruntu.as_ref().map(|g| &g.1);
            let t1 = v.metin.as_ref().map(|m| &m.gorunum);
            if let Err(e) = gpu.ciz(&k.hedef, k.boyut(), &sabit, &o.ps, t0, t1) {
                hata::kaydet(e);
            }
        }
        let mut sunulan = false;
        for k in self.katmanlar.iter_mut() {
            match k.sun() {
                Ok(s) => sunulan |= s,
                Err(e) => hata::kaydet(e),
            }
        }
        if sunulan && !o.gosterildi {
            for k in &mut self.katmanlar {
                k.goster();
            }
            o.gosterildi = true;
        }
        let gecen_kare = kare_basi.elapsed();
        if gecen_kare < EN_KISA_KARE {
            std::thread::sleep(Duration::from_millis(12));
        }
    }

    fn bitir(&mut self) {
        let Durum::Oynuyor(mut o) = std::mem::replace(&mut self.durum, Durum::Bos) else { return };
        if efektler::opak_biter(o.efekt) {
            // Son kare (zaman = 1): opak siyah. Ekranda kalır, `bitince` overlay kalkmadan çalışır
            // ve ancak kare ekrana ULAŞTIKTAN sonra (ör. monitör kapatılmadan önce siyah görünsün).
            let imlec = imlec_konumu();
            if let Some(gpu) = self.gpu.as_mut() {
                for (k, v) in self.katmanlar.iter().zip(&o.veriler) {
                    let sabit = o.sabitler(k.alan, 1.0, o.sure.as_secs_f32(), imlec, v);
                    if gpu.ciz(&k.hedef, k.boyut(), &sabit, &o.ps, None, None).is_err() {
                        gpu.temizle(&k.hedef, [0.0, 0.0, 0.0, 1.0]);
                    }
                }
            }
            for k in &mut self.katmanlar {
                k.goster();
            }
            self.son_kareyi_ulastir();
            // Masaüstü görüntüsü artık gerekmiyor: dokular burada bırakılır.
            o.veriler.clear();
            o.bitince_cagir();
            // `bitince` yeni bir efekt başlatmış olabilir (sahne.oynat kanaldan gelir, burada değil).
            self.durum = Durum::Tutuyor(Instant::now() + OPAK_TUTMA);
        } else {
            self.hepsini_gizle();
            o.veriler.clear();
            o.bitince_cagir();
        }
    }

    /// Opak son kareyi sunar ve ekrana ulaşmasını SINIRLI süre bekler. Sıra: (1) takas zinciri
    /// kare kabul etmiyorsa en çok `SON_KARE_BEKLEME` beklenip yeniden denenir; (2) sunulduktan sonra
    /// bekleme nesnesi yeniden işaretlenene kadar (kare DWM'ce alındı) en çok `SON_KARE_BEKLEME`.
    /// Toplam üst sınır ~2 × 50 ms; ekran kare tüketmiyorsa (kapalı/kilitli) yine döner.
    fn son_kareyi_ulastir(&mut self) {
        let ms = SON_KARE_BEKLEME.as_millis() as u32;
        let mut sunulan = Vec::with_capacity(self.katmanlar.len());
        for k in &mut self.katmanlar {
            let mut s = k.sun().unwrap_or(false);
            if !s {
                // SAFETY: tutamaç katmana ait; bekleme sınırlı.
                if unsafe { WaitForSingleObject(k.bekleme, ms) } == WAIT_OBJECT_0 {
                    s = k.sun().unwrap_or(false);
                }
            }
            if s {
                sunulan.push(k.bekleme);
            }
        }
        if !sunulan.is_empty() {
            // SAFETY: tutamaçlar katmanlara ait; bekleme sınırlı.
            let _ = unsafe { WaitForMultipleObjects(&sunulan, true, ms) };
        }
    }

    /// Tutma süresi doldu: siyah katmanları kaldır.
    pub fn tutmayi_bitir(&mut self) {
        if matches!(self.durum, Durum::Tutuyor(_)) {
            self.durum = Durum::Bos;
            self.hepsini_gizle();
        }
    }

    /// Görünür katmanlara saydam kare sunar ve gizler (yeniden gösterildiğinde eski kare parlamasın).
    /// Sunum beklemesizdir; kuyruk doluysa saydam kare atlanır, pencere yine gizlenir.
    fn hepsini_gizle(&mut self) {
        let Some(gpu) = self.gpu.as_ref() else { return };
        for k in &mut self.katmanlar {
            if k.gorunur_mu() {
                gpu.temizle(&k.hedef, [0.0; 4]);
                let _ = k.sun();
                k.gizle();
            }
        }
    }

    /// Her şeyi bırakır: bekleyen `bitince` çağrılır, pencereler, takas zincirleri, DComp,
    /// Direct2D, dokular ve aygıt serbest kalır.
    pub fn birak(&mut self) {
        match std::mem::replace(&mut self.durum, Durum::Bos) {
            Durum::Oynuyor(mut o) => {
                o.veriler.clear();
                o.bitince_cagir();
            }
            Durum::Tutuyor(_) | Durum::Bos => {}
        }
        self.katmanlar.clear();
        self.metin = None;
        if let Some(d) = self.dcomp.take() {
            // SAFETY: geçerli DComp aygıtı; hedefler zaten bırakıldı.
            unsafe {
                let _ = d.Commit();
            }
        }
        if let Some(g) = self.gpu.take() {
            g.bosalt();
        }
    }
}

impl Drop for Motor {
    fn drop(&mut self) {
        self.birak();
    }
}
