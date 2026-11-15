// Mürekkep — suya düşen mürekkep gibi ortadan yayılan organik leke. Sınırı alan bükmeli fbm ile
// yumuşak loblu; kenarda mürekkep birikir (daha koyu, yoğun şerit), içi daha saydam ve renk1/renk2
// arasında girdaplı bulutlar halinde. Yayılma yavaşlayınca lekenin içinde düzensiz bir boşluk
// açılır, yumuşak kenarla içten dışa siler.
//
// Zaman çizelgesi: 0-0.62 yayılma (yavaşlayarak) | 0.42-1.00 içten dışa silinme.

// Organik sınır: yarıçapa eklenen alan bükmeli gürültü (px). yon: birim yön vektörü.
float sinir_bozumu(float2 p, float2 yon, float R, float s, float akis, float tohum_)
{
    // Büyük loblar yöne bağlı (sürekli: açı değil birim vektör), saçaklar konuma bağlı.
    float lob = fbm(yon * 2.0 + float2(tohum_ * 9.0, tohum_ * 4.0) + akis * 0.12) - 0.5;
    float2 q = p / (85.0 * s);
    float2 bukme = float2(fbm(q + float2(1.7, 9.2) + akis * 0.25), fbm(q + float2(8.3, 2.8) - akis * 0.25)) - 0.5;
    float sacak = fbm(q * 1.3 + bukme * 1.6) - 0.5;
    return lob * R * 0.52 + sacak * R * 0.14;
}

// Boşluğun sınırı: aynı fikir, daha ucuz (alan bükmesiz) — yumuşak kenarlı olduğu için ince
// saçak gerekmez; derleme süresi ve bayt kodu da küçük kalır.
float bosluk_bozumu(float2 p, float2 yon, float R, float s, float akis)
{
    float lob = fbm(yon * 1.7 + float2(tohum * 5.0 + 3.0, tohum * 7.0) + akis * 0.12) - 0.5;
    float sacak = fbm(p / (70.0 * s) + akis * 0.2 + 11.0) - 0.5;
    return lob * R * 0.42 + sacak * R * 0.14;
}

float4 ps_ana(Girdi g) : SV_Target
{
    float2 px = g.konum.xy;
    float t = saturate(zaman);
    float s = px_olcek();
    float2 p = px - cozunurluk * 0.5;
    float r = length(p);
    float2 yon = p / max(r, 1e-3);

    float Rmax = lerp(220.0, 340.0, saturate(siddet)) * s;
    if (r > Rmax * 1.9)
        return float4(0, 0, 0, 0);

    float akis = gecen_sn * 0.6;
    float yayilma = kolay_cikis(saturate(t / 0.62));
    float R = Rmax * (0.04 + 0.96 * yayilma);
    float dis = r - (R + sinir_bozumu(p, yon, R, s, akis, tohum));   // eksi: lekenin içi

    // Silinme: içten açılan, kendi organik sınırı olan boşluk. Bozulma ölçüsü baştan büyük:
    // boşluk yuvarlak bir delik gibi değil, düzensiz adacıklar halinde açılır.
    // Boşluk ~t = 0.95'te lekenin tipik sınırına yetişir; son adımda (pow 8) en uç loblar da silinir.
    float sil = saturate((t - 0.42) / 0.58);
    float Rb = Rmax * (1.15 * pow(sil, 1.3) + 0.60 * pow(sil, 8.0));
    float bozum_b = bosluk_bozumu(p, -yon, max(Rb, Rmax * 0.45) * 0.8, s, akis + 4.0);
    float ic = r - (Rb + bozum_b);                         // + : boşluğun dışı (mürekkep kalır)

    float leke = saturate(0.5 - dis / (3.5 * s));
    float bosluk_disi = (sil > 0.0) ? smoothstep(-12.0 * s, 26.0 * s, ic) : 1.0;
    float maske = leke * bosluk_disi;
    if (maske <= 0.0)
        return float4(0, 0, 0, 0);

    // Kenar birikimi: dış sınıra yakın yoğun şerit; boşluğun kenarında da hafif birikim.
    float kenar = exp(dis / (12.0 * s));
    float ic_kenar = (sil > 0.0) ? exp(-max(ic, 0.0) / (16.0 * s)) * smoothstep(-12.0 * s, 8.0 * s, ic) : 0.0;

    // İç girdap: mürekkebin suda açılan bulutları (yavaş döner).
    float2 q = dondur(p, r / (240.0 * s) - akis * 0.25) / (95.0 * s);
    float bulut = fbm(q + fbm(q * 1.5 + akis * 0.2) * 1.5);
    float yogun = 0.22 + 0.42 * smoothstep(0.30, 0.80, bulut) + 0.42 * kenar + 0.20 * ic_kenar;
    yogun = saturate(yogun);

    float3 rk = lerp(renk1.rgb, renk2.rgb, smoothstep(0.30, 0.75, bulut));
    // Doygun mürekkep: kenar şeridi biraz daha koyu ve yoğun.
    float3 renk = rk * lerp(0.95, 0.60, kenar);

    float guc = lerp(0.65, 0.95, saturate(siddet)) * seviye;
    float zarf_ = smoothstep(0.0, 0.05, t) * (1.0 - smoothstep(0.95, 1.0, t));
    return cikis(renk, yogun * maske * guc * zarf_, px);
}
