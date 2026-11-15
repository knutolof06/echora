// Aurora — üst kenardan süzülen kuzey ışıkları. İki ışık perdesi (renk1 önde, renk2 arkada):
// her biri yavaşça kıvrılan parlak bir alt hat, ondan yukarı uzanıp sönen dikey ışık sütunları
// (fbm akışı); alt hattın altında hızlı, üstünde uzun sönüm. Ekranın üst yarısında kalır,
// perdeler girişte yukarıdan hafifçe aşağı süzülür ve yatayda yavaşça akar.
// x sanal masaüstünde ölçülür: çok ekranda perdeler ekranlar arasında kesintisiz uzanır.

// Bir perde. x: yoğunluk, y: alt hatta yakınlık (0-1, renk geçişi için).
float2 perde(float2 p, float H, float s, float k, float akis, float suzulme)
{
    // Alt hat: geniş ölçekli kıvrım + daha küçük dalgalanma.
    float hx = p.x / (1100.0 * s);
    float taban = H * (0.27 + 0.07 * k)
                + (fbm(float2(hx + k * 3.7 + tohum * 5.0, akis * 0.30 + k)) - 0.5) * H * 0.20
                + sin(p.x / (240.0 * s) + akis * 1.4 + k * 2.0) * 10.0 * s
                - suzulme;
    float d = p.y - taban;                                 // + : alt hattın altı

    // Dikey ışık sütunları: x'e göre sık, y'ye göre çok yavaş değişen gürültü.
    float sutun = fbm(float2(p.x / (24.0 * s) + akis * (0.9 + 0.4 * k) + k * 11.0, p.y / (650.0 * s) - akis * 0.2));
    sutun = smoothstep(0.22, 0.82, sutun);
    // Perde boyunca geniş parlaklık değişimi (perde yer yer incelir).
    float genis = fbm(float2(p.x / (380.0 * s) - akis * 0.45 + k * 5.0, k * 2.3 + 0.5));
    genis = smoothstep(0.25, 0.70, genis);

    float yukari = exp(min(d, 0.0) / (H * lerp(0.13, 0.18, k)));   // alt hattın üstünde uzun sönüm
    float asagi = exp(-max(d, 0.0) / (16.0 * s));                   // alt hattın altında hızlı sönüm
    float hat = exp(-abs(d) / (18.0 * s));                          // alt hattaki parlak şerit
    // Gövde dolu bir blok değil, sütunlardan oluşur (yalnız hafif bir taban ışığı).
    float govde = yukari * (0.10 + 0.90 * sutun * sutun) * 0.85;
    float yogun = (govde + hat * (0.35 + 0.65 * sutun) * 0.60) * asagi * genis;
    return float2(yogun, saturate(exp(min(d, 0.0) / (H * 0.05))));
}

float4 ps_ana(Girdi g) : SV_Target
{
    float2 px = g.konum.xy;
    float t = saturate(zaman);
    float s = px_olcek();
    float H = cozunurluk.y;

    // Ekranın üst yarısının dışı hiç çizilmez.
    float alt_sinir = 1.0 - smoothstep(H * 0.36, H * 0.56, px.y);
    if (alt_sinir <= 0.0)
        return float4(0, 0, 0, 0);

    float2 p = float2(px.x + ekran_ofseti.x, px.y);
    float akis = gecen_sn * 0.55 + tohum * 20.0;
    float suzulme = H * 0.10 * (1.0 - kolay_cikis(t));    // yukarıdan aşağı süzülerek gelir

    float2 on = perde(p, H, s, 0.0, akis, suzulme);
    float2 arka = perde(p, H, s, 1.0, akis * 0.8 + 3.0, suzulme * 1.3);

    // Renkler ışık gibi açık tutulur (açık zeminde kirli görünmesin): ön perde altta renk1,
    // yukarıda renk2'ye; alt hat beyaza çalar. Arka perde renk2 ağırlıklı.
    float3 r1 = lerp(renk1.rgb, float3(1, 1, 1), 0.15);
    float3 r2 = lerp(renk2.rgb, float3(1, 1, 1), 0.10);
    float3 on_rengi = lerp(lerp(r2, r1, 0.35), r1, on.y);
    on_rengi = lerp(on_rengi, float3(1, 1, 1), on.y * 0.20);
    float3 arka_rengi = lerp(r2, r1, arka.y * 0.35);

    // Üst kenarda çok hafif ışık örtüsü (gökyüzü parıltısı).
    float ust_isik = exp(-px.y / (H * 0.12)) * 0.08;

    float guc = lerp(0.65, 1.0, saturate(siddet)) * seviye;
    float zarf_ = zarf(t, 0.28, 0.36);
    float on_a = saturate(on.x);
    float arka_a = saturate(arka.x) * 0.6;
    float3 isik = on_rengi * on_a + arka_rengi * arka_a + lerp(r1, r2, 0.5) * ust_isik;
    float yogunluk = on_a + arka_a + ust_isik;
    float3 renk = isik / max(yogunluk, 1e-4);
    return cikis(renk, saturate(yogunluk) * guc * zarf_ * alt_sinir, px);
}
