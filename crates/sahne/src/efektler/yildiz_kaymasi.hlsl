// Yıldız Kayması — üst köşelerden birinden (tohum seçer) karşı alt tarafa süzülen kuyruklu ışık.
// Baş: beyaz çekirdek, renk1 hale ve ince dört kollu parıltı; kuyruk renk1 → renk2 incelerek
// söner; geçtiği yerde küçük ışıltılar belirip yavaşça kaybolur.
// Yol sanal masaüstünde tanımlıdır: çok ekranda (Hepsi) ışık ekranlar arasında kesintisiz geçer.

// Küçük dört kollu ışıltı (yerel koordinat, px).
float isilti(float2 d, float boy, float s)
{
    float r2 = dot(d, d);
    float cekirdek = exp(-r2 / (2.0 * 1.1 * 1.1 * s * s));
    float kol = exp(-abs(d.x) / (0.7 * s)) * exp(-abs(d.y) / (boy * s))
              + exp(-abs(d.y) / (0.7 * s)) * exp(-abs(d.x) / (boy * s));
    float hale = exp(-sqrt(r2) / (4.0 * s)) * 0.25;
    return cekirdek + kol * 0.45 + hale;
}

float4 ps_ana(Girdi g) : SV_Target
{
    float2 px = g.konum.xy;
    float t = saturate(zaman);
    float s = px_olcek();
    float2 V = max(sanal_boyut, cozunurluk);
    float2 P = px + ekran_ofseti;                          // sanal masaüstü koordinatı

    // Yol: tohum < 0.5 sol üstten sağ alta, değilse sağ üstten sol alta; yükseklikler tohumla oynar.
    float sag_sol = step(0.5, tohum);
    float h1 = frac(tohum * 7.31);
    float h2 = frac(tohum * 3.77 + 0.4);
    float2 A = float2(-0.06 * V.x, (0.02 + 0.16 * h1) * V.y);
    float2 B = float2(1.06 * V.x, (0.62 + 0.25 * h2) * V.y);
    if (sag_sol > 0.5)
    {
        A.x = V.x - A.x;
        B.x = V.x - B.x;
    }
    float2 yol = B - A;
    float yol_boyu = length(yol);
    float2 yon = yol / yol_boyu;
    float2 dik = float2(-yon.y, yon.x);

    // Baş hızlanmadan, akıcı biçimde ilerler; sonda hafif yavaşlar.
    float u = lerp(t, kolay_cikis(t), 0.35);
    float2 bas = A + yol * u;
    float kuyruk_boyu = min(yol_boyu * 0.30, 620.0 * s) * lerp(0.7, 1.1, saturate(siddet));

    float2 d = P - bas;
    float boyuna = dot(d, yon);                           // + : başın önü
    float enine = dot(d, dik);

    // Kuyruk: başın arkasında üstel sönen, incelen ışık.
    float k = saturate(-boyuna / kuyruk_boyu);           // 0 baş, 1 kuyruk sonu
    float kuyruk_varligi = (boyuna < 0.0) ? pow(1.0 - k, 2.2) : exp(-(boyuna * boyuna) / (2.0 * 3.0 * 3.0 * s * s));
    float w = lerp(2.3, 0.5, k) * max(s, 0.5);
    float kuyruk = exp(-(enine * enine) / (2.0 * w * w)) * kuyruk_varligi;
    float kuyruk_hale = exp(-abs(enine) / (lerp(9.0, 3.0, k) * s)) * kuyruk_varligi * 0.30;

    // Baş: parlak çekirdek + hale + yola hizalı ince parıltı kolları.
    float2 yerel = float2(boyuna, enine);
    float r = length(yerel);
    float bas_isik = exp(-(r * r) / (2.0 * 2.2 * 2.2 * s * s)) * 1.2 + exp(-r / (16.0 * s)) * 0.45 + exp(-r / (60.0 * s)) * 0.12;
    float2 cap = dondur(yerel, PI * 0.25);
    float kollar = (exp(-abs(cap.x) / (0.8 * s)) * exp(-abs(cap.y) / (26.0 * s))
                  + exp(-abs(cap.y) / (0.8 * s)) * exp(-abs(cap.x) / (26.0 * s))) * 0.35;

    // Işıltılar: yol boyunca hücrelere bölünmüş, baş geçtikten sonra belirip sönen noktalar.
    float hucre = 46.0 * s;
    float yol_konumu = dot(P - A, yon);
    float yol_enine = dot(P - A, dik);
    float i0 = floor(yol_konumu / hucre);
    float isiltilar = 0;
    [unroll]
    for (int j = -1; j <= 1; j++)
    {
        float i = i0 + (float)j;
        float2 h = karma2(float2(i * 1.7 + 2.0, tohum * 31.0));
        if (h.x < 0.35)
            continue;
        float konum_ = (i + h.x) * hucre;
        float2 merkez = float2(konum_, (h.y - 0.5) * 34.0 * s);
        // Baş bu noktayı ne zaman geçti (u cinsinden): o andan sonra 0.30 boyunca yaşar.
        float gecis = konum_ / yol_boyu;
        float yas = (u - gecis) / 0.30;
        if (yas <= 0.0 || yas >= 1.0)
            continue;
        float parlak = smoothstep(0.0, 0.12, yas) * pow(1.0 - yas, 1.5) * lerp(0.45, 1.0, frac(h.y * 7.0));
        isiltilar += isilti(float2(yol_konumu, yol_enine) - merkez, lerp(4.0, 8.0, h.y), s) * parlak;
    }

    float3 kuyruk_rengi = lerp(renk1.rgb, renk2.rgb, smoothstep(0.1, 0.8, k));
    kuyruk_rengi = lerp(kuyruk_rengi, float3(1, 1, 1), (1.0 - k) * 0.45 * kuyruk / max(kuyruk + kuyruk_hale, 1e-4));
    float3 bas_rengi = lerp(renk1.rgb, float3(1, 1, 1), 0.7);
    float3 isilti_rengi = lerp(renk1.rgb, float3(1, 1, 1), 0.5);

    float ky = kuyruk + kuyruk_hale;
    float by = bas_isik + kollar;
    float iy = isiltilar * 1.3;
    float3 isik = kuyruk_rengi * ky + bas_rengi * by + isilti_rengi * iy;
    float yogunluk = ky + by + iy;

    float guc = lerp(0.65, 1.0, saturate(siddet)) * seviye;
    float zarf_ = smoothstep(0.0, 0.04, t) * (1.0 - smoothstep(0.82, 1.0, t));
    return cikis(isik / max(yogunluk, 1e-4), saturate(yogunluk) * guc * zarf_, px);
}
