# Efekt ekleme notu

## Adımlar
1. `src/efektler/<anahtar>.hlsl` yaz (`Efekt::anahtar()` adı: `portal`, `sok_dalgasi`, `kinetik_yazi`…).
   Giriş noktası: `float4 ps_ana(Girdi g) : SV_Target`. `#include` YAZMA: `ortak.hlsli` motor tarafından
   başa eklenir (`#line 1 "<anahtar>.hlsl"` ile; hata satırları dosyanın kendi satırıdır).
2. `mod.rs` → `kaynak()` içine: `Efekt::Portal => Some(include_str!("portal.hlsl")),`
3. Efekt sonunda ekran opak kalacaksa `opak_biter()`'e ekle (bugün: Tüp Kapanışı, Perde). Böyle bir
   efektin `zaman >= 1` karesi `float4(0, 0, 0, 1)` döndürmeli; motor onu 1 sn tutar.
4. `cargo test -p sahne` — 16 efektin hepsi için otomatik: derleme, boş olmayan kare
   (t = 0.1/0.3/0.5/0.7/0.9), premultiplied geçerliliği, t = 0'da tam saydamlık, t = 1'de tam saydamlık
   ya da (opak bitende) tam siyah. Metin isteyenler metinli ve metinsiz, masaüstü görüntüsü isteyenler
   görüntülü (sentetik desen) ve görüntüsüz sınanır. `hepsi_kipinde_ekranlar_arasi_sureklilik` Tarama ve
   Yıldız Kayması'nın iki pencereye bölünmüş çiziminin tek geniş çizimle aynı olduğunu doğrular.
   `SAHNE_KARE_DIZINI=<dizin>` kareleri BMP yazar (ek olarak 1280x720 `buyuk_*` kareleri);
   `SAHNE_KARE_KOYU=1` koyu masaüstü zemini kullanır.
5. Gerçek ekranda: `cargo run -p sahne --example efekt_dene -- <anahtar> [--hepsi]`.

## Derleme maliyeti
Bayt kodu süreç genelinde önbellekte (`gpu::bayt_kodu`): her efekt süreç başına bir kez derlenir,
aygıt yeniden oluşturulunca yalnız `CreatePixelShader` çağrılır. Yine de ilk oynatmayı kısa tut:
çok çağrılan döngülerde `[loop]` kullan (açılmış döngü bayt kodunu ve D3DCompile süresini katlar;
`fbm` bu yüzden `[loop]`), uzun parçacık döngüsünde `[loop] [fastopt]`. Ölçüm:
`cargo test -p sahne ortak_ve_tum -- --nocapture` efekt başına derleme süresi ve bayt kodu boyutunu yazar.

## Çıktı kuralı
B8G8R8A8_UNORM, **premultiplied**: `return cikis(renk, alfa, px);` (rgb·alfa, sabit titreşimli alfa).
Katman birleştirmek için `ust_uste(ust, alt)`. Renkler sRGB-kodlu 0-1 (doğrusallaştırma yok).
t = 0 ve t = 1'de tamamen saydam olmalı (giriş/çıkış: `zarf(t, giris, cikis)`); sert kenar yok
(`smoothstep`, üstel hale), zamana bağlı rastgelelik yok (titreme). Piksel ölçülerini 1080 px'e göre
ölçekle (`kenar_nabzi.hlsl` → `olcek_al()`): Stüdyo önizlemesi küçük `Hedef::Dikdortgen`'de oynar.

## cbuffer `Sabitler` (b0) — `ortak.hlsli`, Rust: `sabitler.rs`
| alan | tür | anlam |
|---|---|---|
| zaman | float | 0-1 ilerleme |
| sure_sn | float | süre (sn) |
| cozunurluk | float2 | pencere px |
| renk1, renk2 | float4 | ayar renkleri (rgb, a=1) |
| siddet | float | 0-1 |
| seviye | float | sese tepki çarpanı: kapalı 1.0, açık 0.4 + 0.6·yumuşak seviye |
| imlec | float2 | imleç, pencereye göre px (bu ekranda değilse merkez) |
| ekran_ofseti | float2 | pencerenin sanal masaüstündeki yeri (px, sol-üst 0) — çok ekranda süreklilik |
| sanal_boyut | float2 | sanal masaüstü px |
| tohum | float | efekt başına 0-1 |
| goruntu_var | float | 1: t0 geçerli; 0: yedek görünüm çiz |
| metin_var | float | 1: t1 geçerli |
| gecen_sn | float | baştan beri sn |
| metin_kutusu | float4 | metnin kutusu px (sol, üst, genişlik, yükseklik) |
| dalga[16] | float4 | 64 seviye örneği, eskiden yeniye ~1 sn (sese tepki kapalıysa 0.5); `dalga_ornegi(x)` |

Alan eklemek: HLSL ve Rust'ta AYNI sıra; 16 bayt sınırını aşan vektör olmasın; `BEKLENEN_OFSETLER`
güncelle (yansıma testi D3DReflect ile doğrular).

## Dokular
- `t0 masaustu`: pencere boyutunda masaüstü görüntüsü (`Efekt::masaustu_goruntusu_ister()` olanlar:
  Tüp Kapanışı, Şok Dalgası). uv = px / cozunurluk. Alınamazsa `goruntu_var = 0`.
- `t1 metin_dokusu`: pencere boyutunda, beyaz premultiplied metin (`metin_ister()`: Kinetik Yazı,
  Harf Yağmuru; `Istek::metin` boş değilse). Ortalı, Segoe UI Variable Display (yoksa Segoe UI),
  yarı kalın, boyut = yükseklik·0.085. Harf harf etki için `metin_kutusu` içinde x'e göre ilerle.
- `s0 orneklec`: doğrusal, kenara kenetli. `SampleLevel(orneklec, uv, 0)` kullan.

## Yardımcılar (`ortak.hlsli`)
`zarf`, `kolay_giris/kolay_cikis/kolay_gc`, `kenar_mesafesi(px)`, `kutu_sdf(p, merkez, yari, r)`,
`px_olcek()` (1080 px'e göre ölçek), `dondur(p, a)`, `parca_mesafesi(p, a, b)` (x: uzaklık, y: 0-1 konum),
`karma(p)` / `karma2(p)` (hash), `gurultu(p)`, `fbm(p)`, `dalga_ornegi(x)`, `cikis`, `ust_uste`,
`yedek_nefes(px, t)` (metin/görüntü yoksa Nefes görünümü), `PI`, `TAU`.

Çok ekran (`Hepsi`): ekranlar arası süreklilik isteyen efekt x'i `px.x + ekran_ofseti.x` ile sanal
masaüstünde ölçer, genişlik için `max(sanal_boyut.x, cozunurluk.x)` kullanır (Tarama, Yıldız Kayması, Aurora).
