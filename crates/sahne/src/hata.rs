//! Crate içi son hata kaydı. Motor hiçbir yerde `eprintln!` kullanmaz; üst katman
//! `son_hata()` ile alıp kendi günlüğüne yazar.

use std::sync::Mutex;

static SON_HATA: Mutex<Option<String>> = Mutex::new(None);

/// Hatayı kaydeder (öncekinin üstüne yazar).
pub(crate) fn kaydet(ileti: impl Into<String>) {
    let mut k = SON_HATA.lock().unwrap_or_else(|e| e.into_inner());
    *k = Some(ileti.into());
}

/// Son kaydedilen hatayı verir ve kaydı temizler (her hata bir kez alınır).
/// Örnek: shader derleme hatası, aygıt/pencere oluşturulamaması, masaüstü görüntüsü alınamaması.
pub fn son_hata() -> Option<String> {
    SON_HATA.lock().unwrap_or_else(|e| e.into_inner()).take()
}
