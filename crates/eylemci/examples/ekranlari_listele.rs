//! Salt-okuma: ekranları ve her biri için DDC/CI güç kipi (VCP 0xD6) okumasını yazdırır.
//! Hiçbir şey yazmaz, hiçbir monitörü kapatmaz.

fn main() {
    let ekranlar = eylemci::ekranlari_listele();
    println!("{} ekran bulundu", ekranlar.len());
    for e in &ekranlar {
        println!(
            "{}  ana={}  konum=({}, {})  boyut={}x{}",
            e.aygit_adi, e.ana, e.sol, e.ust, e.genislik, e.yukseklik
        );
        match eylemci::ddc_guc_durumu(&e.aygit_adi) {
            Ok(deger) => println!("  DDC/CI güç kipi (0xD6) okundu: {deger} (1 = açık)"),
            Err(hata) => println!("  DDC/CI güç kipi okunamadı: {hata}"),
        }
    }
}
