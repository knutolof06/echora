//! Ekransız GPU testleri: gerçek D3D11 aygıtı, pencere YOK. Donanım aygıtı yoksa WARP;
//! o da yoksa test mesaj yazıp geçer.
//!
//! `SAHNE_KARE_DIZINI` ortam değişkeni verilirse çizilen kareler (sahte bir masaüstü zeminine
//! bindirilmiş olarak) o dizine BMP yazılır — yalnız görsel denetim içindir. Masaüstü görüntüsü
//! isteyen efektler sentetik bir desen dokusuyla sınanır (gerçek ekran görüntüsü alınmaz).

use windows::core::{Interface, PCWSTR, GUID};
use windows::Win32::Graphics::Direct3D::Fxc::D3DReflect;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11PixelShader, ID3D11ShaderReflection, ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
    D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SHADER_BUFFER_DESC,
    D3D11_SHADER_VARIABLE_DESC, D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
    D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::DirectWrite::{DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};

use crate::efektler;
use crate::gpu::{blob_baytlari, derle, Gpu};
use crate::metin::{genis, MetinCizici};
use crate::sabitler::{Sabitler, BEKLENEN_OFSETLER};
use crate::secim::GECMIS_UZUNLUGU;
use crate::{Efekt, EfektAyari};

const G: u32 = 480;
const Y: u32 = 270;

fn aygit() -> Option<Gpu> {
    match Gpu::olustur(true) {
        Ok(g) => Some(g),
        Err(e) => {
            println!("GPU testi atlandı: {e}");
            None
        }
    }
}

fn yazilan_efektler() -> Vec<Efekt> {
    Efekt::HEPSI.iter().copied().filter(|e| efektler::kaynak(*e).is_some()).collect()
}

/// Dokuyu CPU'ya okur (BGRA, satır satır sıkışık).
fn oku(gpu: &Gpu, doku: &ID3D11Texture2D, g: u32, y: u32) -> Vec<u8> {
    let tanim = D3D11_TEXTURE2D_DESC {
        Width: g,
        Height: y,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut ara = None;
    let mut cikti = vec![0u8; (g * y * 4) as usize];
    // SAFETY: test; tüm nesneler aynı aygıtta, eşlenen bellek RowPitch*y bayt.
    unsafe {
        gpu.aygit.CreateTexture2D(&tanim, None, Some(&mut ara)).expect("ara doku");
        let ara = ara.expect("ara doku");
        gpu.baglam.CopyResource(&ara, doku);
        let mut m = D3D11_MAPPED_SUBRESOURCE::default();
        gpu.baglam.Map(&ara, 0, D3D11_MAP_READ, 0, Some(&mut m)).expect("eşleme");
        for satir in 0..y as usize {
            let kaynak = (m.pData as *const u8).add(satir * m.RowPitch as usize);
            std::ptr::copy_nonoverlapping(kaynak, cikti.as_mut_ptr().add(satir * g as usize * 4), g as usize * 4);
        }
        gpu.baglam.Unmap(&ara, 0);
    }
    cikti
}

/// Görsel denetim için BMP (premultiplied pikseller sahte bir masaüstü zeminine bindirilir).
fn bmp_yaz(ad: &str, piksel: &[u8], g: u32, y: u32) {
    bmp_yaz_zeminli(ad, piksel, g, y, None);
}

/// `zemin` verilirse (BGRA, aynı boyut) kare onun üstüne bindirilir: masaüstünü büken efektler
/// gerçekte kendi aldıkları görüntünün üstünde görünür.
fn bmp_yaz_zeminli(ad: &str, piksel: &[u8], g: u32, y: u32, zemin_pikselleri: Option<&[u8]>) {
    let Ok(dizin) = std::env::var("SAHNE_KARE_DIZINI") else { return };
    let koyu = std::env::var("SAHNE_KARE_KOYU").is_ok();
    let mut veri = Vec::with_capacity(piksel.len() + 54);
    let boyut = 54 + piksel.len() as u32;
    veri.extend_from_slice(b"BM");
    veri.extend_from_slice(&boyut.to_le_bytes());
    veri.extend_from_slice(&[0; 4]);
    veri.extend_from_slice(&54u32.to_le_bytes());
    veri.extend_from_slice(&40u32.to_le_bytes());
    veri.extend_from_slice(&(g as i32).to_le_bytes());
    veri.extend_from_slice(&(-(y as i32)).to_le_bytes());
    veri.extend_from_slice(&1u16.to_le_bytes());
    veri.extend_from_slice(&32u16.to_le_bytes());
    veri.extend_from_slice(&[0; 24]);
    for (i, p) in piksel.chunks_exact(4).enumerate() {
        let x = (i as u32 % g) as f32 / g as f32;
        let yy = (i as u32 / g) as f32 / y as f32;
        // Zemin: soldan sağa açıktan koyuya, üstte mavimsi pencere şeridi.
        let z = 215.0 - 170.0 * x;
        let zemin = match zemin_pikselleri {
            Some(zp) => [zp[i * 4] as f32, zp[i * 4 + 1] as f32, zp[i * 4 + 2] as f32],
            // `SAHNE_KARE_KOYU`: koyu masaüstü zemini (koyu temada nasıl göründüğünü denetlemek için).
            None if koyu => [28.0 + 10.0 * x, 22.0, 18.0],
            None if yy < 0.3 => [z * 1.0, z * 0.8, z * 0.6],
            None => [z, z, z],
        };
        let a = p[3] as f32 / 255.0;
        for k in 0..3 {
            veri.push((p[k] as f32 + zemin[k] * (1.0 - a)).clamp(0.0, 255.0) as u8);
        }
        veri.push(255);
    }
    let _ = std::fs::create_dir_all(&dizin);
    let _ = std::fs::write(std::path::Path::new(&dizin).join(format!("{ad}.bmp")), veri);
}

fn sabitler(t: f32, goruntu: bool) -> Sabitler {
    let ayar = EfektAyari::default();
    let mut s = Sabitler::sifir();
    s.zaman = t;
    s.sure_sn = 1.0;
    s.cozunurluk = [G as f32, Y as f32];
    let r1 = ayar.renk1.kesir();
    let r2 = ayar.renk2.kesir();
    s.renk1 = [r1[0], r1[1], r1[2], 1.0];
    s.renk2 = [r2[0], r2[1], r2[2], 1.0];
    s.siddet = ayar.siddet;
    s.seviye = 1.0;
    s.imlec = [G as f32 * 0.5, Y as f32 * 0.5];
    s.sanal_boyut = [G as f32, Y as f32];
    s.tohum = 0.37;
    s.goruntu_var = if goruntu { 1.0 } else { 0.0 };
    s.gecen_sn = t;
    for (i, d) in s.dalga.iter_mut().enumerate() {
        *d = 0.5 + 0.4 * (i as f32 * 0.4).sin();
    }
    assert_eq!(s.dalga.len(), GECMIS_UZUNLUGU);
    s
}

/// Sentetik masaüstü deseni (BGRA): renkli degrade + dama tahtası (bükülme gözle seçilsin).
fn sahte_masaustu_pikselleri(g: u32, y: u32) -> Vec<u8> {
    let hucre = (y / 17).max(4);
    let mut p = vec![0u8; (g * y * 4) as usize];
    for (i, px) in p.chunks_exact_mut(4).enumerate() {
        let xx = i as u32 % g;
        let yy = i as u32 / g;
        px[0] = (xx * 255 / g) as u8;
        px[1] = (yy * 255 / y) as u8;
        px[2] = if (xx / hucre + yy / hucre).is_multiple_of(2) { 220 } else { 60 };
        px[3] = 255;
    }
    p
}

/// Sahte masaüstü görüntüsü (t0) ve CPU'daki pikselleri.
fn sahte_masaustu(gpu: &Gpu, g: u32, y: u32) -> (windows::Win32::Graphics::Direct3D11::ID3D11ShaderResourceView, Vec<u8>) {
    let p = sahte_masaustu_pikselleri(g, y);
    let tanim = D3D11_TEXTURE2D_DESC {
        Width: g,
        Height: y,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let veri = D3D11_SUBRESOURCE_DATA { pSysMem: p.as_ptr() as *const _, SysMemPitch: g * 4, SysMemSlicePitch: 0 };
    let mut d = None;
    // SAFETY: test; başlangıç verisi tanım boyutunda.
    unsafe { gpu.aygit.CreateTexture2D(&tanim, Some(&veri), Some(&mut d)) }.expect("sahte masaüstü");
    (gpu.kaynak_gorunumu(&d.expect("sahte masaüstü")).expect("görünüm"), p)
}

#[test]
fn ortak_ve_tum_hlsl_derlenir() {
    derle(efektler::ORTAK, "ortak.hlsli", windows::core::s!("vs_ana"), windows::core::s!("vs_5_0"))
        .unwrap_or_else(|e| panic!("{e}"));
    let yazilan = yazilan_efektler();
    assert_eq!(yazilan.len(), 16);
    let mut toplam = 0;
    for e in yazilan {
        let k = efektler::tam_kaynak(e).expect("kaynak");
        let bas = std::time::Instant::now();
        let blob = derle(&k, &format!("{}.hlsl", e.anahtar()), windows::core::s!("ps_ana"), windows::core::s!("ps_5_0"))
            .unwrap_or_else(|h| panic!("{h}"));
        let n = blob_baytlari(&blob).len();
        toplam += n;
        println!("{:>14}: derleme {:>6.1} ms, bayt kodu {n} B", e.anahtar(), bas.elapsed().as_secs_f64() * 1000.0);
    }
    println!("toplam bayt kodu: {toplam} B");
}

#[test]
fn derleme_hatasi_ileti_ile_doner() {
    let k = format!("{}\nfloat4 ps_ana(Girdi g) : SV_Target {{ return tanimsiz; }}", efektler::ORTAK);
    let h = derle(&k, "bozuk.hlsl", windows::core::s!("ps_ana"), windows::core::s!("ps_5_0")).err().expect("hata beklenirdi");
    assert!(h.contains("tanimsiz") || h.contains("undeclared"), "{h}");
}

#[test]
fn cbuffer_yansima_ile_eslesir() {
    let k = efektler::tam_kaynak(Efekt::SesDalgasi).expect("kaynak");
    let blob = derle(&k, "ses_dalgasi.hlsl", windows::core::s!("ps_ana"), windows::core::s!("ps_5_0")).expect("derleme");
    let b = blob_baytlari(&blob);
    let mut p = std::ptr::null_mut();
    // SAFETY: test; bayt kodu geçerli, arayüz kimliği ID3D11ShaderReflection.
    unsafe {
        D3DReflect(b.as_ptr() as *const _, b.len(), &ID3D11ShaderReflection::IID as *const GUID, &mut p).expect("yansıma");
        let yansima = ID3D11ShaderReflection::from_raw(p);
        let cb = yansima.GetConstantBufferByName(windows::core::s!("Sabitler")).expect("cbuffer");
        let mut btanim = D3D11_SHADER_BUFFER_DESC::default();
        cb.GetDesc(&mut btanim).expect("cbuffer tanımı");
        assert_eq!(btanim.Size as usize, std::mem::size_of::<Sabitler>(), "cbuffer boyutu");
        assert_eq!(btanim.Variables as usize, BEKLENEN_OFSETLER.len());
        for (ad, ofset) in BEKLENEN_OFSETLER {
            let adc = format!("{ad}\0");
            let v = cb.GetVariableByName(windows::core::PCSTR(adc.as_ptr())).expect(ad);
            let mut vt = D3D11_SHADER_VARIABLE_DESC::default();
            v.GetDesc(&mut vt).unwrap_or_else(|e| panic!("{ad}: {e}"));
            assert_eq!(vt.StartOffset as usize, ofset, "{ad} ofseti");
        }
    }
}

/// Bir efektin sınanacak girdi çeşitleri: (görüntü var, metin var). Masaüstü görüntüsü
/// isteyenler görüntülü ve görüntüsüz, metin isteyenler metinli ve metinsiz sınanır.
fn cesitler(e: Efekt, metin_var: bool) -> Vec<(bool, bool)> {
    let goruntu: &[bool] = if e.masaustu_goruntusu_ister() { &[false, true] } else { &[false] };
    let metin: &[bool] = if e.metin_ister() && metin_var { &[false, true] } else { &[false] };
    goruntu.iter().flat_map(|&g| metin.iter().map(move |&m| (g, m))).collect()
}

fn cesit_adi(goruntu: bool, metin: bool) -> &'static str {
    match (goruntu, metin) {
        (true, _) => "goruntu",
        (_, true) => "metin",
        _ => "yedek",
    }
}

#[test]
fn efektler_bos_olmayan_goruntu_uretir() {
    let Some(mut gpu) = aygit() else { return };
    let doku = gpu.doku(G, Y, &[D3D11_BIND_RENDER_TARGET]).expect("hedef doku");
    let rtv = gpu.hedef_gorunumu(&doku).expect("rtv");
    let (masaustu, masaustu_pikselleri) = sahte_masaustu(&gpu, G, Y);
    // Metin dokusu (t1): motorun kullandığı çizici ile, pencere boyutunda.
    let metin = match MetinCizici::olustur(&gpu).and_then(|c| c.ciz(&gpu, "Müziği aç, sesi yüzde elli yap", G, Y)) {
        Ok(m) => Some(m),
        Err(e) => {
            println!("metinli çeşitler atlandı: {e}");
            None
        }
    };

    let ciz = |gpu: &mut Gpu, e: Efekt, ps: &ID3D11PixelShader, t: f32, goruntu: bool, metinli: bool| -> Vec<u8> {
        let mut s = sabitler(t, goruntu);
        let t1 = match (&metin, metinli) {
            (Some(m), true) => {
                s.metin_var = 1.0;
                s.metin_kutusu = m.kutu;
                Some(&m.gorunum)
            }
            _ => None,
        };
        gpu.temizle(&rtv, [0.0; 4]);
        gpu.ciz(&rtv, (G, Y), &s, ps, goruntu.then_some(&masaustu), t1).unwrap_or_else(|h| panic!("{}: {h}", e.anahtar()));
        let p = oku(gpu, &doku, G, Y);
        // Premultiplied geçerlilik: hiçbir kanal alfadan büyük değil (1 birim yuvarlama payı).
        let gecersiz = p.chunks_exact(4).filter(|px| px[0].max(px[1]).max(px[2]) > px[3].saturating_add(1)).count();
        assert_eq!(gecersiz, 0, "{} t={t} {}: premultiplied değil", e.anahtar(), cesit_adi(goruntu, metinli));
        p
    };

    for e in yazilan_efektler() {
        let ps = gpu.piksel_golgelendirici(e).unwrap_or_else(|h| panic!("{h}"));
        for (goruntu, metinli) in cesitler(e, metin.is_some()) {
            let ad = cesit_adi(goruntu, metinli);
            for t in [0.1f32, 0.3, 0.5, 0.7, 0.9] {
                let p = ciz(&mut gpu, e, &ps, t, goruntu, metinli);
                let gorunur = p.chunks_exact(4).filter(|px| px[3] > 0).count();
                assert!(gorunur > 0, "{} t={t} {ad}: boş kare", e.anahtar());
                println!("{:>14} t={t:.1} {ad:<7} görünür piksel: {gorunur}/{}", e.anahtar(), G * Y);
                let zemin = goruntu.then_some(masaustu_pikselleri.as_slice());
                bmp_yaz_zeminli(&format!("{}_{ad}_{:02}", e.anahtar(), (t * 10.0).round() as u32), &p, G, Y, zemin);
            }

            // Uçlar: t = 0 her efektte tamamen saydam (görüntülü Tüp Kapanışı hariç: ilk karesi
            // masaüstünün opak kopyasıdır). t = 1: opak biten efektte tam siyah, diğerlerinde saydam.
            if !(e == Efekt::TupKapanisi && goruntu) {
                let p = ciz(&mut gpu, e, &ps, 0.0, goruntu, metinli);
                let kalan = p.chunks_exact(4).filter(|px| px[3] > 1).count();
                assert_eq!(kalan, 0, "{} t=0 {ad}: başta görünür piksel var", e.anahtar());
            }
            let p = ciz(&mut gpu, e, &ps, 1.0, goruntu, metinli);
            if efektler::opak_biter(e) {
                assert!(p.chunks_exact(4).all(|px| px == [0, 0, 0, 255]), "{} {ad}: son kare tam siyah değil", e.anahtar());
            } else {
                let kalan = p.chunks_exact(4).filter(|px| px[3] > 1).count();
                assert_eq!(kalan, 0, "{} t=1 {ad}: sonda görünür piksel kaldı", e.anahtar());
            }
        }
    }
    drop(metin);
    gpu.bosalt();
}

/// Premultiplied değişmezi (rgb ≤ a) tüm zaman çizelgesinde: t = 0..1 sık adımla, şiddet uçları
/// (0, 1) ve uç renklerle (beyaz/beyaz, doygun kırmızı/mavi). Ana testteki 5 zaman noktası
/// yalnız bazı evreleri görür; değişmez evre geçişlerinde (ör. zemin ile hale aynı anda kısmi)
/// bozulabilir, o anlar burada yakalanır.
#[test]
fn premultiplied_degismezi_tum_zaman_cizelgesi() {
    let Some(mut gpu) = aygit() else { return };
    let (kg, ky) = (160u32, 90u32);
    let doku = gpu.doku(kg, ky, &[D3D11_BIND_RENDER_TARGET]).expect("hedef doku");
    let rtv = gpu.hedef_gorunumu(&doku).expect("rtv");
    let (masaustu, _) = sahte_masaustu(&gpu, kg, ky);
    let metin = MetinCizici::olustur(&gpu).and_then(|c| c.ciz(&gpu, "Ekranı kapat", kg, ky)).ok();
    let renkler: [([f32; 3], [f32; 3]); 3] =
        [([0.31, 0.76, 0.97], [0.70, 0.53, 1.0]), ([1.0, 1.0, 1.0], [1.0, 1.0, 1.0]), ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0])];
    let mut kare_sayisi = 0;
    for e in yazilan_efektler() {
        let ps = gpu.piksel_golgelendirici(e).expect("ps");
        for (goruntu, metinli) in cesitler(e, metin.is_some()) {
            for (r1, r2) in renkler {
                for siddet in [0.0f32, 1.0] {
                    for adim in 0..=40 {
                        let t = adim as f32 / 40.0;
                        let mut s = sabitler(t, goruntu);
                        s.cozunurluk = [kg as f32, ky as f32];
                        s.sanal_boyut = s.cozunurluk;
                        s.imlec = [kg as f32 * 0.5, ky as f32 * 0.5];
                        s.renk1 = [r1[0], r1[1], r1[2], 1.0];
                        s.renk2 = [r2[0], r2[1], r2[2], 1.0];
                        s.siddet = siddet;
                        let t1 = match (&metin, metinli) {
                            (Some(m), true) => {
                                s.metin_var = 1.0;
                                s.metin_kutusu = m.kutu;
                                Some(&m.gorunum)
                            }
                            _ => None,
                        };
                        gpu.temizle(&rtv, [0.0; 4]);
                        gpu.ciz(&rtv, (kg, ky), &s, &ps, goruntu.then_some(&masaustu), t1).expect("çizim");
                        let p = oku(&gpu, &doku, kg, ky);
                        let gecersiz = p.chunks_exact(4).filter(|px| px[0].max(px[1]).max(px[2]) > px[3].saturating_add(1)).count();
                        assert_eq!(
                            gecersiz,
                            0,
                            "{} {} t={t} şiddet={siddet} renk1={r1:?}: premultiplied değil",
                            e.anahtar(),
                            cesit_adi(goruntu, metinli)
                        );
                        kare_sayisi += 1;
                    }
                }
            }
        }
    }
    println!("premultiplied taraması: {kare_sayisi} kare");
    gpu.bosalt();
}

/// Yalnız görsel denetim (`SAHNE_KARE_DIZINI` verilmişse): her efektin 1280x720 kareleri.
/// Küçük test penceresinde alt piksele düşen ince ayrıntılar (parçacık, kuyruk) burada seçilir.
#[test]
fn buyuk_kareler_gorsel_denetim() {
    if std::env::var("SAHNE_KARE_DIZINI").is_err() {
        return;
    }
    let Some(mut gpu) = aygit() else { return };
    let (bg, by) = (1280u32, 720u32);
    let doku = gpu.doku(bg, by, &[D3D11_BIND_RENDER_TARGET]).expect("hedef doku");
    let rtv = gpu.hedef_gorunumu(&doku).expect("rtv");
    let (masaustu, masaustu_pikselleri) = sahte_masaustu(&gpu, bg, by);
    let metin = MetinCizici::olustur(&gpu).and_then(|c| c.ciz(&gpu, "Müziği aç, sesi yüzde elli yap", bg, by)).ok();
    for e in yazilan_efektler() {
        let ps = gpu.piksel_golgelendirici(e).expect("ps");
        for (goruntu, metinli) in cesitler(e, metin.is_some()) {
            for t in [0.25f32, 0.5, 0.75] {
                let mut s = sabitler(t, goruntu);
                s.cozunurluk = [bg as f32, by as f32];
                s.sanal_boyut = s.cozunurluk;
                s.imlec = [bg as f32 * 0.42, by as f32 * 0.55];
                let t1 = match (&metin, metinli) {
                    (Some(m), true) => {
                        s.metin_var = 1.0;
                        s.metin_kutusu = m.kutu;
                        Some(&m.gorunum)
                    }
                    _ => None,
                };
                gpu.temizle(&rtv, [0.0; 4]);
                gpu.ciz(&rtv, (bg, by), &s, &ps, goruntu.then_some(&masaustu), t1).expect("çizim");
                let p = oku(&gpu, &doku, bg, by);
                let zemin = goruntu.then_some(masaustu_pikselleri.as_slice());
                let ad = format!("buyuk_{}_{}_{:03}", e.anahtar(), cesit_adi(goruntu, metinli), (t * 100.0).round() as u32);
                bmp_yaz_zeminli(&ad, &p, bg, by, zemin);
            }
        }
    }
    gpu.bosalt();
}

/// Tarama ve Yıldız Kayması çok ekranda kesintisiz: sanal masaüstünü iki pencereye bölüp
/// (`ekran_ofseti`) çizmek, tek geniş pencereye çizmekle aynı sonucu verir.
#[test]
fn hepsi_kipinde_ekranlar_arasi_sureklilik() {
    let Some(mut gpu) = aygit() else { return };
    let (gg, yy) = (2 * G, Y);
    let genis = gpu.doku(gg, yy, &[D3D11_BIND_RENDER_TARGET]).expect("geniş doku");
    let genis_rtv = gpu.hedef_gorunumu(&genis).expect("rtv");
    let yarim = gpu.doku(G, Y, &[D3D11_BIND_RENDER_TARGET]).expect("yarım doku");
    let yarim_rtv = gpu.hedef_gorunumu(&yarim).expect("rtv");
    for (e, tohum) in [(Efekt::Tarama, 0.37f32), (Efekt::YildizKaymasi, 0.37), (Efekt::YildizKaymasi, 0.81)] {
        let ps = gpu.piksel_golgelendirici(e).expect("ps");
        for t in [0.3f32, 0.5, 0.7] {
            let mut s = sabitler(t, false);
            s.tohum = tohum;
            s.sanal_boyut = [gg as f32, yy as f32];
            s.cozunurluk = [gg as f32, yy as f32];
            s.imlec = [gg as f32 * 0.5, yy as f32 * 0.5];
            gpu.temizle(&genis_rtv, [0.0; 4]);
            gpu.ciz(&genis_rtv, (gg, yy), &s, &ps, None, None).expect("çizim");
            let tam = oku(&gpu, &genis, gg, yy);
            let mut gorunur = [0usize; 2];
            for parca in 0..2u32 {
                s.cozunurluk = [G as f32, Y as f32];
                s.ekran_ofseti = [(parca * G) as f32, 0.0];
                gpu.temizle(&yarim_rtv, [0.0; 4]);
                gpu.ciz(&yarim_rtv, (G, Y), &s, &ps, None, None).expect("çizim");
                let p = oku(&gpu, &yarim, G, Y);
                let mut en_buyuk = 0u8;
                for y in 0..Y as usize {
                    for x in 0..G as usize {
                        let a = &p[(y * G as usize + x) * 4..][..4];
                        let b = &tam[(y * gg as usize + parca as usize * G as usize + x) * 4..][..4];
                        for k in 0..4 {
                            en_buyuk = en_buyuk.max(a[k].abs_diff(b[k]));
                        }
                        gorunur[parca as usize] += (a[3] > 0) as usize;
                    }
                }
                // Fark yalnız konuma bağlı ±0.5/255 titreşimden gelebilir.
                assert!(en_buyuk <= 2, "{} t={t} parça {parca}: geniş çizimden {en_buyuk} birim farklı", e.anahtar());
            }
            println!("{:>14} tohum={tohum} t={t:.1} sol/sağ görünür: {gorunur:?}", e.anahtar());
            bmp_yaz(&format!("{}_hepsi_{tohum}_{:02}", e.anahtar(), (t * 10.0).round() as u32), &tam, gg, yy);
        }
    }
    gpu.bosalt();
}

/// Bayt kodu süreç genelinde önbellekte: aygıt yeniden oluşturulunca derleme tekrarlanmaz.
#[test]
fn bayt_kodu_surec_geneli_onbellekte() {
    let e = Efekt::Portal;
    let bas = std::time::Instant::now();
    let k = efektler::tam_kaynak(e).expect("kaynak");
    let _ = derle(&k, "portal.hlsl", windows::core::s!("ps_ana"), windows::core::s!("ps_5_0")).expect("derleme");
    let derleme = bas.elapsed();

    let ilk = crate::gpu::bayt_kodu(Some(e)).expect("bayt kodu");
    let bas = std::time::Instant::now();
    let ikinci = crate::gpu::bayt_kodu(Some(e)).expect("bayt kodu");
    let onbellek = bas.elapsed();
    assert!(std::sync::Arc::ptr_eq(&ilk, &ikinci), "ikinci çağrı yeniden derledi");

    let Some(mut gpu) = aygit() else { return };
    let _ = gpu.piksel_golgelendirici(e).expect("ps");
    gpu.bosalt();
    drop(gpu);
    let Some(mut gpu) = aygit() else { return };
    let bas = std::time::Instant::now();
    let _ = gpu.piksel_golgelendirici(e).expect("ps");
    let yeni_aygitta = bas.elapsed();
    gpu.bosalt();
    println!(
        "portal: D3DCompile {:.1} ms | önbellekten bayt kodu {:.3} ms | yeni aygıtta CreatePixelShader {:.2} ms | bayt kodu {} B",
        derleme.as_secs_f64() * 1000.0,
        onbellek.as_secs_f64() * 1000.0,
        yeni_aygitta.as_secs_f64() * 1000.0,
        ilk.len()
    );
}

#[test]
fn metin_dokusu_turkce_karakterler() {
    let Some(gpu) = aygit() else { return };
    let cizici = match MetinCizici::olustur(&gpu) {
        Ok(c) => c,
        Err(e) => {
            println!("metin testi atlandı: {e}");
            return;
        }
    };
    println!("yazı tipi: {}", cizici.aile());

    // Seçilen yazı tipinde tüm Türkçe harflerin glifi var (kutucuğa düşmez).
    let harfler: Vec<u32> = "çğıİöşüÇĞÖŞÜ".chars().map(|c| c as u32).collect();
    // SAFETY: test; DirectWrite çağrıları, çıktılar yerel.
    unsafe {
        let ad = genis(cizici.aile());
        let mut dizin = 0u32;
        let mut var = windows::core::BOOL(0);
        cizici.koleksiyon().FindFamilyName(PCWSTR(ad.as_ptr()), &mut dizin, &mut var).expect("aile ara");
        assert!(var.as_bool(), "seçilen aile koleksiyonda yok");
        let aile = cizici.koleksiyon().GetFontFamily(dizin).expect("aile");
        let yazi = aile
            .GetFirstMatchingFont(DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL)
            .expect("yazı tipi");
        let yuz = yazi.CreateFontFace().expect("yazı yüzü");
        let mut glifler = vec![0u16; harfler.len()];
        yuz.GetGlyphIndices(harfler.as_ptr(), harfler.len() as u32, glifler.as_mut_ptr()).expect("glifler");
        assert!(glifler.iter().all(|&g| g != 0), "eksik glif: {glifler:?}");
    }

    let (g, y) = (640u32, 160u32);
    let d = cizici.ciz(&gpu, "Çağrı İşığı öğün ŞÜ", g, y).unwrap_or_else(|e| panic!("{e}"));
    let p = oku(&gpu, &d._doku, g, y);
    let murekkep = p.chunks_exact(4).filter(|px| px[3] > 128).count();
    assert!(murekkep > 500, "metin çok az piksel: {murekkep}");
    // Beyaz premultiplied: rgb == a.
    assert!(p.chunks_exact(4).all(|px| px[0].abs_diff(px[3]) <= 1 && px[2].abs_diff(px[3]) <= 1));
    // Kutu ortalı ve dokunun içinde.
    let [sol, ust, gen, yuk] = d.kutu;
    assert!(sol > 0.0 && ust > 0.0 && gen > 0.0 && yuk > 0.0 && sol + gen < g as f32 && ust + yuk < y as f32, "{:?}", d.kutu);
    assert!(((sol + gen * 0.5) - g as f32 * 0.5).abs() < 2.0, "yatay ortalı değil: {:?}", d.kutu);
    println!("metin kutusu: {:?}, mürekkep pikseli: {murekkep}", d.kutu);
    bmp_yaz("metin", &p, g, y);
    gpu.bosalt();
}
