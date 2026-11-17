//! Komut tablosu: ayar dosyasını okur, ifadeleri düz listeye açar, tanınan metni komuta eşler.
//! Ses ya da Win32 bağımlılığı yoktur; saf mantıktır.

mod ayar;
#[cfg(test)]
mod ayar_testler;
pub mod efekt;
mod esleme;
mod ham;
mod kisayol;
mod metin;
mod ortak;
#[cfg(test)]
mod ortak_testler;
mod saat;
#[cfg(test)]
mod sema_testler;
#[cfg(test)]
mod v02_testler;
#[cfg(test)]
mod yakinlik_testler;
mod sayi;
mod turler;

pub use turler::{DinlemeKipi, EkranSecimi, MedyaTusu, YontemTercihi};

pub use ayar::{
    adim_alanlari, efekt_varsayilani, eylem_adlari, eylem_alanlari, Adim, Ayarlar, EfektKipi, Eylem,
    Komut, OlayAyari, OlayEfektleri, SahneTanimi, YakinlikAyari,
};
pub use esleme::Eslesme;
pub use ham::{
    HamAdim, HamAlkis, HamAyarlar, HamEkran, HamKisayol, HamKomut, HamOlaylar, HamSahne, HamUygulama,
    HamYakinlik,
};
pub use kisayol::{
    kisayol_coz, AlkisAyari, Kisayol, KisayolTanimi, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN,
};
pub use metin::turkce_kucuk;
pub use ortak::{EfektOrtak, EfektOzel};
pub use saat::{saat_araligi_coz, saat_araligi_yaz};
pub use sayi::sayi_sozcukleri;
