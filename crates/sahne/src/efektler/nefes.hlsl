// Nefes — ekran kenarları bir kez hafifçe kararıp açılır (renk1 tonlu vinyet).
// Atanmamış komutların ve tüm düşürmelerin (animasyon kapalı, yazılmamış efekt) efekti.

float4 ps_ana(Girdi g) : SV_Target
{
    float2 px = g.konum.xy;
    float t = saturate(zaman);

    // Tek nefes: sin² — uçlarda hem değer hem eğim sıfır, giriş/çıkış kendiliğinden yumuşak.
    float nefes = sin(PI * t);
    nefes *= nefes;

    // Süperelips uzaklık: kenar ortalarında 1, köşelerde ~1.19; vinyet ekranın biçimine uyar.
    float2 d = abs(px / cozunurluk * 2.0 - 1.0);
    float2 d4 = d * d * d * d;
    float r = sqrt(sqrt(d4.x + d4.y));
    float vinyet = smoothstep(0.40, 1.18, r);
    vinyet *= vinyet * (3.0 - 2.0 * vinyet);

    // Kenarın dibinde ince ek koyulaşma (çerçeve hissi).
    float kisa = min(cozunurluk.x, cozunurluk.y);
    float kenar = exp(-kenar_mesafesi(px) / (kisa * 0.035));

    float guc = lerp(0.22, 0.55, saturate(siddet)) * seviye;
    float alfa = saturate(vinyet * 0.9 + kenar * 0.25) * nefes * guc;

    // Çok koyu, renk1'e çalan ton; kenara yaklaştıkça ton biraz belirginleşir.
    float3 ton = renk1.rgb * lerp(0.10, 0.30, kenar);
    return cikis(ton, alfa, px);
}
