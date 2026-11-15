//! Salt-okuma: monitörler ve `EkranSecimi` eşlemesi, ses durumu, ön pencere, öğrenilmiş ekran
//! yöntemleri, mikrofon aygıtları. Hiçbir şeyi değiştirmez, hiçbir eylem çalıştırmaz.

use eylemci::EkranSecimi;

fn main() {
    let ekranlar = eylemci::ekranlari_listele();
    println!("== Monitörler ({}) ==", ekranlar.len());
    for e in &ekranlar {
        println!(
            "{}  ana={}  konum=({}, {})  boyut={}x{}",
            e.aygit_adi, e.ana, e.sol, e.ust, e.genislik, e.yukseklik
        );
    }

    println!("== EkranSecimi eşlemesi ==");
    for (ad, s) in [
        ("Ana", EkranSecimi::Ana),
        ("Ikinci", EkranSecimi::Ikinci),
        ("Sol", EkranSecimi::Sol),
        ("Sag", EkranSecimi::Sag),
    ] {
        match eylemci::ekran_sec(&ekranlar, s) {
            Ok(i) => println!("{ad:<7} -> {}", ekranlar[i].aygit_adi),
            Err(e) => println!("{ad:<7} -> hata: {e}"),
        }
    }

    println!("== Ses ==");
    match eylemci::ses_durumu_oku() {
        Ok(d) => println!("seviye={:.0}%  sessiz={}", d.seviye * 100.0, d.sessiz),
        Err(e) => println!("okunamadı: {e}"),
    }

    println!("== Ön pencere ==");
    match eylemci::on_pencere_oku() {
        None => println!("uygun ön pencere yok (masaüstü/görev çubuğu/kendi pencerem ya da görünmez)"),
        Some(p) => {
            println!("başlık: {:?}", eylemci::pencere_basligi(p));
            println!("süreç: {:?}", eylemci::pencere_surec_adi(p));
            match eylemci::pencere_durumu_oku(p) {
                Ok((d, r)) => println!(
                    "durum: {d:?}  dikdörtgen: sol={} üst={} sağ={} alt={}",
                    r.sol, r.ust, r.sag, r.alt
                ),
                Err(e) => println!("durum okunamadı: {e}"),
            }
        }
    }

    println!("== Öğrenilmiş ekran yöntemleri ==");
    let klasor = eylemci::yanki_klasoru();
    let yontemler = eylemci::ogrenilen_yontemleri_oku(klasor.as_deref());
    if yontemler.is_empty() {
        println!("(kayıt yok)");
    }
    for (ad, y) in yontemler {
        println!("{ad} -> {y:?}");
    }

    println!("== Mikrofon aygıtları ==");
    match kulak::yakalama::aygitlari_listele() {
        Ok(liste) => {
            for a in liste {
                println!("{} varsayilan={}  kimlik={}", a.ad, a.varsayilan, a.kimlik);
            }
        }
        Err(e) => println!("listelenemedi: {e}"),
    }
}
