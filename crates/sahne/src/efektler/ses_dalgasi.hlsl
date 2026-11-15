// Ses Dalgası — alt kenarda sesin dalga formu. Ortada en yeni ses, iki yana doğru ~1 sn öncesi;
// üç katmanlı ince ışık çizgisi (renk1, renk2, ikisinin karışımı) ve altında yumuşak dolgu.
// Genlik `dalga[]` geçmişinden gelir (sese tepki kapalıysa sabit 0.5).

float4 ps_ana(Girdi g) : SV_Target
{
    float2 px = g.konum.xy;
    float t = saturate(zaman);
    float W = cozunurluk.x, H = cozunurluk.y;

    float z = zarf(t, 0.16, 0.30);
    float taban = H - H * 0.075;                         // çizgilerin dinlenme hattı
    float olcek = H * 0.055 * lerp(0.6, 1.25, saturate(siddet));

    // Bant dışında hiçbir şey yok (erken çık).
    if (px.y < taban - olcek * 1.6 - 60.0)
        return float4(0, 0, 0, 0);

    float x = px.x / W;
    float u = abs(x - 0.5) * 2.0;                        // 0 orta, 1 kenar
    float gecmis = dalga_ornegi(1.0 - u);                // ortada en yeni
    float incelme = sin(PI * saturate(x));
    incelme *= incelme;                                  // kenarlarda genlik sıfıra iner
    float genlik = (0.22 + 0.78 * gecmis) * olcek * incelme * lerp(0.35, 1.0, z);

    float3 renk_toplam = 0;
    float yogunluk = 0;
    [unroll]
    for (int k = 0; k < 3; k++)
    {
        float fk = (float)k;
        float frekans = (6.5 + fk * 3.2) * PI / W;        // rad/px
        float yon = (k == 1) ? -1.0 : 1.0;
        float faz = px.x * frekans + yon * gecen_sn * (2.4 + fk * 0.9) + tohum * TAU + fk * 1.7;
        float a = genlik * (1.0 - 0.28 * fk);
        float y = taban - a * sin(faz);
        // Dikey fark eğimle bölünür: dik yerlerde çizgi incelmesin.
        float egim = a * cos(faz) * frekans;
        float uzak = abs(px.y - y) / sqrt(1.0 + egim * egim);
        float cekirdek = exp(-(uzak * uzak) / (2.0 * 1.15 * 1.15));
        float hale = exp(-uzak / 11.0) * 0.32;
        float agirlik = (k == 0) ? 1.0 : 0.62;
        float3 rk = (k == 0) ? renk1.rgb : ((k == 1) ? renk2.rgb : lerp(renk1.rgb, renk2.rgb, 0.5));
        float ik = (cekirdek + hale) * agirlik;
        renk_toplam += lerp(rk, float3(1, 1, 1), cekirdek * 0.35) * ik;
        yogunluk += ik;
    }

    // Alt kenardan yükselen yumuşak ışık (dalga formunun zemini).
    float zemin = exp(-(H - px.y) / (H * 0.045)) * 0.20 * sqrt(incelme) * (0.6 + 0.4 * gecmis);
    renk_toplam += lerp(renk1.rgb, renk2.rgb, x) * zemin;
    yogunluk += zemin;

    float3 renk = renk_toplam / max(yogunluk, 1e-4);
    float guc = lerp(0.6, 1.0, saturate(siddet)) * seviye;
    return cikis(renk, saturate(yogunluk) * z * guc, px);
}
