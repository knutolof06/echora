//! Dinlemenin kendiliğinden beklemesi: sessiz saatler ve ön planda tam ekran uygulama.
//!
//! Yoklama YOK: dinleyici her konuşma BAŞLANGICINDA bir kez sorar (boşta sıfır maliyet).
//! Karar saf işlevdir; saat ve bildirim durumu `Ortam` ile soyutlanır.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use komut::Ayarlar;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Shell::SHQueryUserNotificationState;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

/// Konuşmanın neden yok sayıldığı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engel {
    SessizSaat,
    TamEkran,
}

/// Dış durum sorguları.
pub trait Ortam {
    /// Yerel saat, gece yarısından beri dakika.
    fn yerel_dakika(&self) -> u16;
    /// `SHQueryUserNotificationState` ham değeri; sorgu başarısızsa `None`.
    fn bildirim_durumu(&self) -> Option<i32>;
    /// Ön plan penceresi bu sürecin mi (ör. kendi siyah perdemiz).
    fn on_plan_bizim(&self) -> bool;
    /// Bu süreç bir ekranı DDC/CI ile kapattı ve henüz açmadı.
    fn ddc_kapali(&self) -> bool;
}

/// `QUNS_BUSY` (2), `QUNS_RUNNING_D3D_FULL_SCREEN` (3), `QUNS_PRESENTATION_MODE` (4): ön planda
/// tam ekran uygulama ya da sunum kipi.
pub fn tam_ekran_mi(durum: i32) -> bool {
    matches!(durum, 2..=4)
}

/// Saf karar. Önce sessiz saat (ucuz), sonra tam ekran sorgulanır; sessiz saatteyse bildirim
/// durumu hiç sorulmaz. Sorgu başarısızsa engel yoktur.
///
/// Tam ekran engeli UYGULANMAZ: ön plan penceresi bizimse (siyah perde tam ekran sayılabilir;
/// aksi halde perde açıkken "ekranları aç" yok sayılırdı) ya da bir ekranı DDC/CI ile biz kapattıysak.
pub fn engel(ayarlar: &Ayarlar, ortam: &dyn Ortam) -> Option<Engel> {
    if !ayarlar.sessiz_saatler.is_empty() && ayarlar.sessiz_mi(ortam.yerel_dakika()) {
        return Some(Engel::SessizSaat);
    }
    match ortam.bildirim_durumu() {
        Some(d) if tam_ekran_mi(d) && !ortam.on_plan_bizim() && !ortam.ddc_kapali() => Some(Engel::TamEkran),
        _ => None,
    }
}

/// Gerçek Windows sorguları (salt-okuma). `ddc_kapali` UI iş parçacığıyla paylaşılan bayraktır.
pub struct GercekOrtam {
    pub ddc_kapali: Arc<AtomicBool>,
}

impl Ortam for GercekOrtam {
    fn yerel_dakika(&self) -> u16 {
        // SAFETY: GetLocalTime yalnız dönen yapıyı doldurur.
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        t.wHour * 60 + t.wMinute
    }
    fn bildirim_durumu(&self) -> Option<i32> {
        // SAFETY: yalnız çıktı değeri yazılır.
        unsafe { SHQueryUserNotificationState() }.ok().map(|d| d.0)
    }
    fn on_plan_bizim(&self) -> bool {
        // SAFETY: salt-okuma; ön plan penceresi yoksa pid 0 döner.
        unsafe {
            let h = GetForegroundWindow();
            if h.is_invalid() {
                return false;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            pid != 0 && pid == GetCurrentProcessId()
        }
    }
    fn ddc_kapali(&self) -> bool {
        self.ddc_kapali.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod testler {
    use super::*;
    use std::cell::Cell;

    struct Sahte {
        dakika: u16,
        durum: Option<i32>,
        soruldu: Cell<bool>,
        bizim: bool,
        ddc: bool,
    }

    impl Ortam for Sahte {
        fn yerel_dakika(&self) -> u16 {
            self.dakika
        }
        fn bildirim_durumu(&self) -> Option<i32> {
            self.soruldu.set(true);
            self.durum
        }
        fn on_plan_bizim(&self) -> bool {
            self.bizim
        }
        fn ddc_kapali(&self) -> bool {
            self.ddc
        }
    }

    fn ortam(dakika: u16, durum: Option<i32>) -> Sahte {
        Sahte { dakika, durum, soruldu: Cell::new(false), bizim: false, ddc: false }
    }

    #[test]
    fn kendi_perdemiz_ve_ddc_kapali_ekran_tam_ekran_sayilmaz() {
        let a = Ayarlar::coz(Ayarlar::varsayilan_toml()).unwrap();
        let bizim = Sahte { bizim: true, ..ortam(0, Some(2)) };
        assert_eq!(engel(&a, &bizim), None);
        let ddc = Sahte { ddc: true, ..ortam(0, Some(3)) };
        assert_eq!(engel(&a, &ddc), None);
        // Sessiz saat ise yine engellenir (perdemiz olsa da).
        let sessiz = ayarlar("\"00:00-23:59\"");
        assert_eq!(engel(&sessiz, &Sahte { bizim: true, ..ortam(60, Some(2)) }), Some(Engel::SessizSaat));
    }

    fn ayarlar(sessiz: &str) -> Ayarlar {
        let metin = Ayarlar::varsayilan_toml().replacen(
            "tetik = \"atlas\"",
            &format!("tetik = \"atlas\"\nsessiz_saatler = [{sessiz}]"),
            1,
        );
        Ayarlar::coz(&metin).unwrap()
    }

    #[test]
    fn tam_ekran_durumlari() {
        // QUNS_NOT_PRESENT=1, BUSY=2, D3D=3, SUNUM=4, KABUL=5, SESSİZ=6, UYGULAMA=7.
        let beklenen = [(1, false), (2, true), (3, true), (4, true), (5, false), (6, false), (7, false)];
        for (d, b) in beklenen {
            assert_eq!(tam_ekran_mi(d), b, "{d}");
        }
    }

    #[test]
    fn sessiz_saat_gece_yarisini_asar_ve_tam_ekran_sorulmaz() {
        let a = ayarlar("\"23:00-07:30\"");
        let o = ortam(2 * 60, Some(3));
        assert_eq!(engel(&a, &o), Some(Engel::SessizSaat));
        assert!(!o.soruldu.get(), "sessiz saatte bildirim durumu sorulmamalı");
        assert_eq!(engel(&a, &ortam(7 * 60 + 30, Some(5))), None);
        assert_eq!(engel(&a, &ortam(23 * 60, Some(5))), Some(Engel::SessizSaat));
    }

    #[test]
    fn tam_ekran_sessiz_saat_disinda() {
        let a = ayarlar("\"13:00-14:00\"");
        assert_eq!(engel(&a, &ortam(12 * 60, Some(2))), Some(Engel::TamEkran));
        assert_eq!(engel(&a, &ortam(12 * 60, Some(4))), Some(Engel::TamEkran));
        assert_eq!(engel(&a, &ortam(12 * 60, Some(5))), None);
        // Sorgu başarısızsa dinleme sürer.
        assert_eq!(engel(&a, &ortam(12 * 60, None)), None);
    }

    #[test]
    fn sessiz_saat_yoksa_yalniz_tam_ekran() {
        let a = Ayarlar::coz(Ayarlar::varsayilan_toml()).unwrap();
        assert!(a.sessiz_saatler.is_empty());
        assert_eq!(engel(&a, &ortam(0, Some(5))), None);
        assert_eq!(engel(&a, &ortam(0, Some(3))), Some(Engel::TamEkran));
    }
}
