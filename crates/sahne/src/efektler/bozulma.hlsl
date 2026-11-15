// Bozulma — kısa sinyal bozulması: renk1 tonlu tarama çizgileri, RGB'si ayrışmış kayık bantlar,
// ekranda kayan bir tarama ışığı. Anlaşılmadı (gri) ve hata (kırmızı) olaylarının efekti.
// Bant düzeni 6 adımda değişir; her adım kendi içinde yumuşakça belirip söner (sert sıçrama yok).

// Bir adımın bant katkısı: x'te yumuşak kenarlı, satırı seçilmişse görünür.
float3 bantlar(float2 px, float adim, float ayrisma)
{
    float satir_yuk = lerp(14.0, 70.0, karma(float2(adim * 3.1, tohum * 91.0)));
    float satir = floor(px.y / satir_yuk);
    float r = karma(float2(satir * 1.7 + adim * 13.0, tohum * 53.0));
    float secili = smoothstep(0.70, 0.76, r);
    if (secili <= 0.0)
        return 0;

    float yerel = frac(px.y / satir_yuk);
    float dikey = smoothstep(0.0, 0.12, yerel) * (1.0 - smoothstep(0.88, 1.0, yerel));

    float kayma = (karma(float2(satir, adim + 7.0)) - 0.5) * 160.0;
    float x0 = karma(float2(satir * 1.3, adim + 11.0)) * cozunurluk.x * 0.7 - cozunurluk.x * 0.1;
    float x1 = x0 + lerp(0.15, 0.8, karma(float2(satir, adim + 5.0))) * cozunurluk.x;

    float3 x = px.x + kayma + float3(ayrisma, 0.0, -ayrisma);
    float3 yatay = smoothstep(x0 - 3.0, x0 + 3.0, x) * (1.0 - smoothstep(x1 - 3.0, x1 + 3.0, x));
    return yatay * dikey * secili;
}

float4 ps_ana(Girdi g) : SV_Target
{
    float2 px = g.konum.xy;
    float t = saturate(zaman);
    float z = smoothstep(0.0, 0.10, t) * (1.0 - smoothstep(0.50, 1.0, t));
    float guc = lerp(0.55, 1.0, saturate(siddet)) * seviye;

    // Adım geçişi: her adım sin ile belirip söner, komşu adım üst üste biner.
    float k = t * 6.0;
    float adim = floor(k);
    float f = frac(k);
    float ayrisma = lerp(3.0, 12.0, saturate(siddet)) * z;
    float3 bant = bantlar(px, adim, ayrisma) * sin(PI * f)
                + bantlar(px, adim + 1.0, ayrisma) * smoothstep(0.6, 1.0, f) * 0.5;
    bant = saturate(bant);

    // Bantlar: RGB kanalları ayrı ayrı kaydığı için kenarları renkli; renk1 ile tonlanır.
    float3 ton = lerp(float3(1, 1, 1), renk1.rgb, 0.55);
    float bant_alfa = max(max(bant.r, bant.g), bant.b) * 0.30;
    float4 bant_katmani = float4(bant * ton * 0.30, bant_alfa);

    // Tarama çizgileri (3 px aralık) + renk1'e çalan çok hafif örtü.
    float tarama = 0.5 + 0.5 * sin(px.y * (TAU / 3.0));
    float kenar = 1.0 - smoothstep(0.0, min(cozunurluk.x, cozunurluk.y) * 0.25, kenar_mesafesi(px));
    float ortu_alfa = (0.05 + tarama * 0.07 + kenar * 0.06);
    float4 ortu = float4(renk1.rgb * 0.18 * ortu_alfa, ortu_alfa);

    // Aşağı kayan tarama ışığı.
    float isik_y = frac(t * 1.4 + tohum) * (cozunurluk.y + 200.0) - 100.0;
    float isik = exp(-abs(px.y - isik_y) / 30.0) * 0.12;
    float4 isik_katmani = float4(ton * isik, isik);

    float4 sonuc = ust_uste(bant_katmani, ust_uste(isik_katmani, ortu));
    sonuc *= z * guc;

    // Görünür alfa'ya sabit titreşim (bantlaşmayı önler).
    float titres = (karma(px) - 0.5) / 255.0 * step(0.002, sonuc.a);
    sonuc.a = saturate(sonuc.a + titres);
    sonuc.rgb = min(sonuc.rgb, sonuc.a);
    return sonuc;
}
