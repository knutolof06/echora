// Kenar Nabzı — çerçevede bir tur dolaşan ince parlak ışık. Baş renk1, kuyruk renk2'ye geçer;
// kenarın içine doğru yumuşak parıltı. Üst ortadan başlar, saat yönünde tam tur atar.
// Ölçüler 1080 px yüksekliğe göre verilir ve pencereyle ölçeklenir (Stüdyo önizlemesi aynı görünür).

float olcek_al() { return max(min(cozunurluk.x, cozunurluk.y) / 1080.0, 0.2); }

// Başa göre çevre üzerindeki fark (px): eksi = başın arkası.
float bas_farki(float s, float bas, float cevre)
{
    float f = s - bas;
    return f - cevre * floor(f / cevre + 0.5);
}

// Bir kenarın katkısı. p kenar doğrusuna dik izdüşürülür (kenar boyunca kenetlenir); böylece köşelerde
// iki kenarın katkısı sürekli kalır, köşegen dikiş oluşmaz.
// s0: kenarın çevredeki başlangıcı, yon: kenar boyunca birim yön, uzunluk: kenar uzunluğu.
float3 kenar_katkisi(float2 p, float2 baslangic, float2 yon, float uzunluk, float s0,
                     float bas, float gidilen, float cevre)
{
    float olcek = olcek_al();
    float boy = clamp(dot(p - baslangic, yon), 0.0, uzunluk);
    float2 en_yakin = baslangic + yon * boy;
    float d = length(p - en_yakin);
    float fark = bas_farki(s0 + boy, bas, cevre);

    float kuyruk_boyu = cevre * 0.26;
    float burun = 12.0 * olcek;
    float kuyruk = fark <= 0.0 ? exp(fark / (kuyruk_boyu * 0.33)) : exp(-(fark * fark) / (2.0 * burun * burun));
    // Başlangıç noktasının gerisinde kuyruk olmasın (ışık yoktan var olmuş gibi görünmesin).
    kuyruk *= smoothstep(-gidilen - 90.0 * olcek, -gidilen + 30.0 * olcek, fark);

    float dd = max(d - 1.0 * olcek, 0.0);
    float c = max(1.4 * olcek, 0.8);
    float cekirdek = exp(-(dd * dd) / (2.0 * c * c));
    float hale = exp(-d / (20.0 * olcek)) * 0.42 + exp(-d / (75.0 * olcek)) * 0.12;
    float bp = 45.0 * olcek;
    float bas_parlama = exp(-(fark * fark) / (2.0 * bp * bp)) * exp(-d / (28.0 * olcek)) * 0.55;
    float kuyruk_konumu = saturate(-fark / kuyruk_boyu);
    // x: ışık yoğunluğu, y: beyazlık, z: kuyruk konumu (renk geçişi)
    return float3(kuyruk * (cekirdek + hale) + bas_parlama,
                  saturate(cekirdek * kuyruk * 0.55 + bas_parlama * 0.7),
                  kuyruk_konumu);
}

float4 ps_ana(Girdi g) : SV_Target
{
    float2 px = g.konum.xy;
    float t = saturate(zaman);
    float W = cozunurluk.x, H = cozunurluk.y;
    float cevre = 2.0 * (W + H);

    // Başın konumu: yavaş başlar, hızlanır, yavaşlayarak turu tamamlar.
    float gidilen = kolay_gc(t) * cevre;
    float bas = 0.5 * W + gidilen;

    // Dört kenar, saat yönünde: üst, sağ, alt, sol.
    float3 k0 = kenar_katkisi(px, float2(0, 0), float2(1, 0), W, 0.0, bas, gidilen, cevre);
    float3 k1 = kenar_katkisi(px, float2(W, 0), float2(0, 1), H, W, bas, gidilen, cevre);
    float3 k2 = kenar_katkisi(px, float2(W, H), float2(-1, 0), W, W + H, bas, gidilen, cevre);
    float3 k3 = kenar_katkisi(px, float2(0, H), float2(0, -1), H, 2.0 * W + H, bas, gidilen, cevre);
    float3 k = k0;
    if (k1.x > k.x) k = k1;
    if (k2.x > k.x) k = k2;
    if (k3.x > k.x) k = k3;

    // Tüm çerçevede çok hafif sabit hale (ışığın yolunu sezdirir).
    float cerceve = exp(-kenar_mesafesi(px) / (9.0 * olcek_al())) * 0.09;

    float z = zarf(t, 0.10, 0.22);
    float guc = lerp(0.55, 1.0, saturate(siddet)) * seviye;
    float yogunluk = (k.x + cerceve) * z * guc;

    float3 renk = lerp(renk1.rgb, renk2.rgb, smoothstep(0.0, 0.8, k.z));
    renk = lerp(renk, float3(1.0, 1.0, 1.0), k.y);
    return cikis(renk, yogunluk, px);
}
