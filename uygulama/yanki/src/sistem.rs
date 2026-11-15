//! `Sistem` uygulamaları: gerçek (`eylemci::GercekSistem`) ve `--kuru` kipindeki `KuruSistem`.

use eylemci::{
    Dikdortgen, EkranSecimi, GercekSistem, MedyaTusu, PencereDurumu, PencereKimligi, SesDurumu, Sistem, Yontem,
    YontemTercihi,
};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowThreadProcessId, IsWindow};

use crate::dagitim::{PencereImzaci, PencereImzasi};
use crate::gunluk;

/// `--kuru` kipi: hiçbir şey çalıştırılmaz, yalnız günlüğe yazılır. Okuma yöntemleri sabit sahte
/// değer döner (ses %50, ön pencere 1, normal 800x600 pencere).
pub struct KuruSistem;

fn kuru(metin: &str) {
    gunluk::yaz(&format!("[kuru] {metin}"));
}

impl Sistem for KuruSistem {
    fn tum_ekranlari_kapat(&mut self) -> Result<(), String> {
        kuru("tum_ekranlari_kapat");
        Ok(())
    }
    fn ana_ekrani_kapat(&mut self) -> Result<Yontem, String> {
        kuru("ana_ekrani_kapat");
        Ok(Yontem::SiyahPerde)
    }
    fn ekrani_kapat(&mut self, secim: EkranSecimi) -> Result<Yontem, String> {
        kuru(&format!("ekrani_kapat {secim:?}"));
        Ok(Yontem::SiyahPerde)
    }
    fn ekranlari_ac(&mut self) -> Result<(), String> {
        kuru("ekranlari_ac");
        Ok(())
    }
    fn uygulama_ac(&mut self, hedef: &str, argumanlar: &[String]) -> Result<(), String> {
        kuru(format!("uygulama_ac {hedef} {}", argumanlar.join(" ")).trim_end());
        Ok(())
    }
    fn uygulama_kapat(&mut self, surec_adi: &str) -> Result<(), String> {
        kuru(&format!("uygulama_kapat {surec_adi}"));
        Ok(())
    }
    fn uygulama_one_getir(&mut self, surec_adi: &str) -> Result<(), String> {
        kuru(&format!("uygulama_one_getir {surec_adi}"));
        Ok(())
    }
    fn ses_oku(&mut self) -> Result<SesDurumu, String> {
        Ok(SesDurumu { seviye: 0.5, sessiz: false })
    }
    fn ses_ayarla(&mut self, seviye: f32) -> Result<(), String> {
        kuru(&format!("ses_ayarla {:.0}%", seviye * 100.0));
        Ok(())
    }
    fn sessiz_ayarla(&mut self, sessiz: bool) -> Result<(), String> {
        kuru(&format!("sessiz_ayarla {sessiz}"));
        Ok(())
    }
    fn on_pencere(&mut self) -> Option<PencereKimligi> {
        Some(PencereKimligi(1))
    }
    fn pencere_durumu(&mut self, _p: PencereKimligi) -> Result<(PencereDurumu, Dikdortgen), String> {
        Ok((PencereDurumu::Normal, Dikdortgen { sol: 100, ust: 100, sag: 900, alt: 700 }))
    }
    fn pencere_durumu_ayarla(&mut self, p: PencereKimligi, d: PencereDurumu) -> Result<(), String> {
        kuru(&format!("pencere_durumu_ayarla {} {d:?}", p.0));
        Ok(())
    }
    fn pencereyi_diger_ekrana_tasi(&mut self, p: PencereKimligi) -> Result<(), String> {
        kuru(&format!("pencereyi_diger_ekrana_tasi {}", p.0));
        Ok(())
    }
    fn pencereyi_konuma_tasi(&mut self, p: PencereKimligi, d: Dikdortgen) -> Result<(), String> {
        kuru(&format!("pencereyi_konuma_tasi {} ({},{})-({},{})", p.0, d.sol, d.ust, d.sag, d.alt));
        Ok(())
    }
    fn masaustunu_goster(&mut self) -> Result<(), String> {
        kuru("masaustunu_goster");
        Ok(())
    }
    fn oturumu_kilitle(&mut self) -> Result<(), String> {
        kuru("oturumu_kilitle");
        Ok(())
    }
    fn uyut(&mut self) -> Result<(), String> {
        kuru("uyut");
        Ok(())
    }
    fn medya_tusu(&mut self, t: MedyaTusu) -> Result<(), String> {
        kuru(&format!("medya_tusu {t:?}"));
        Ok(())
    }
    fn tus_gonder(&mut self, kombinasyon: &str) -> Result<(), String> {
        kuru(&format!("tus_gonder {kombinasyon}"));
        Ok(())
    }
    fn komut_calistir(&mut self, komut_satiri: &str) -> Result<(), String> {
        // Komut satırı kullanıcının yazdığı metindir; yalnız uzunluğu yazılır (gizli bilgi taşıyabilir).
        kuru(&format!("komut_calistir ({} karakter)", komut_satiri.chars().count()));
        Ok(())
    }
}

/// UI iş parçacığının sahiplendiği sistem. Somut tür tutulur: ekran yöntem tercihleri yalnız
/// `GercekSistem`'e özgüdür.
pub enum SistemKutusu {
    Kuru(KuruSistem),
    Gercek(GercekSistem),
}

impl SistemKutusu {
    /// Bu sisteme uygun pencere imzacısı.
    pub fn imzaci(&self) -> Box<dyn PencereImzaci> {
        match self {
            SistemKutusu::Kuru(_) => Box::new(KuruImzaci),
            SistemKutusu::Gercek(_) => Box::new(Win32Imzaci),
        }
    }

    pub fn sistem(&mut self) -> &mut dyn Sistem {
        match self {
            SistemKutusu::Kuru(s) => s,
            SistemKutusu::Gercek(s) => s,
        }
    }

    /// Ayar dosyasındaki monitör başına yöntemleri uygular. `onceki` listede olup yenide olmayan
    /// aygıtlar `Otomatik`'e döner. Her yüklemede çağrılır.
    pub fn yontemleri_uygula(&mut self, onceki: &[(String, YontemTercihi)], yeni: &[(String, YontemTercihi)]) {
        match self {
            SistemKutusu::Gercek(g) => {
                for (aygit, _) in onceki {
                    if !yeni.iter().any(|(a, _)| a == aygit) {
                        g.yontem_tercihi(aygit, YontemTercihi::Otomatik);
                    }
                }
                for (aygit, t) in yeni {
                    g.yontem_tercihi(aygit, *t);
                }
            }
            SistemKutusu::Kuru(_) => {
                if !yeni.is_empty() {
                    let liste: Vec<String> = yeni.iter().map(|(a, t)| format!("{a}={t:?}")).collect();
                    kuru(&format!("ekran yöntemleri: {}", liste.join(", ")));
                }
            }
        }
    }
}

/// Gerçek pencere imzası: süreç kimliği + sınıf adı (salt-okuma Win32).
pub struct Win32Imzaci;

impl PencereImzaci for Win32Imzaci {
    fn imza(&self, p: PencereKimligi) -> Option<PencereImzasi> {
        let h = HWND(p.0 as *mut _);
        // SAFETY: salt-okuma sorguları; geçersiz HWND'de hata/0 döner.
        unsafe {
            if !IsWindow(Some(h)).as_bool() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid));
            if pid == 0 {
                return None;
            }
            let mut tampon = [0u16; 256];
            let n = GetClassNameW(h, &mut tampon);
            if n <= 0 {
                return None;
            }
            Some(PencereImzasi { pid, sinif: String::from_utf16_lossy(&tampon[..n as usize]) })
        }
    }
}

/// Kuru kip: `KuruSistem`'in sahte ön penceresi için sabit imza.
pub struct KuruImzaci;

impl PencereImzaci for KuruImzaci {
    fn imza(&self, _p: PencereKimligi) -> Option<PencereImzasi> {
        Some(PencereImzasi { pid: 0, sinif: "kuru".into() })
    }
}
