// Şok Dalgası — ortadan yayılan halka masaüstünün görüntüsünü (t0) büker: halka üstünde
// kırılma (ışınsal ötelenme) + hafif kromatik sapma + öndeki ince renk1 ışık sırtı.
// Overlay yalnız halka bölgesini çizer: halkanın dışı ve içi tam saydamdır, gerçek masaüstü görünür.
// Görüntü yoksa (goruntu_var = 0): parlak halka (iç yüzü renk1, dış yüzü renk2'ye çalar) +
// arkasında kısa, yumuşak iz.

float4 ps_ana(Girdi g) : SV_Target
{
    float2 px = g.konum.xy;
    float t = saturate(zaman);
    float s = px_olcek();
    float2 merkez = cozunurluk * 0.5;
    float2 p = px - merkez;
    float r = length(p);
    float2 yon = p / max(r, 1e-3);

    // Halka ekranın köşelerine kadar yayılır, yavaşlayarak; genişliği büyür, gücü azalır.
    float yari_kosegen = length(cozunurluk) * 0.5;
    float w = lerp(26.0, 80.0, t) * s;
    float ilerleme = 1.0 - pow(1.0 - t, 2.2);
    float R = ilerleme * (yari_kosegen + w * 1.5);
    float d = r - R;                                     // + : halkanın önü

    float zarf_ = smoothstep(0.0, 0.07, t) * (1.0 - smoothstep(0.55, 1.0, t));
    float guc = lerp(0.55, 1.0, saturate(siddet)) * seviye;

    // Halkanın kompakt bandı (dışında hiçbir şey çizilmez). Bükülmenin belirgin olduğu yerde
    // tam opak; ötelenme ~0'a indikten sonra söner (bükük ve gerçek görüntü üst üste binmesin).
    float bant = 1.0 - smoothstep(1.4 * w, 2.4 * w, abs(d));
    // Merkezde (halka henüz küçükken) bükülme sıfıra insin: tekillik olmasın.
    bant *= smoothstep(0.0, 1.5 * w, R);
    // Görüntülü kipte bant hızla opaklaşır, bükülme zarfla büyür: yarı saydam bükük kopya
    // gerçek masaüstüyle üst üste binip çift görüntü vermesin.
    float alfa_bant = bant * smoothstep(0.0, 0.3, zarf_);

    // Kırılma profili: gaussiyanın türevi (önde dışa, arkada içe iter) — mercek gibi.
    float x = d / w;
    float profil = -x * exp(-x * x * 1.6) * 2.1;
    float otele = profil * lerp(10.0, 26.0, saturate(siddet)) * s * zarf_ * seviye;
    // Işık sırtı: halkanın ön yüzünde ince parlaklık.
    float u_sirt = (d - 0.25 * w) / (0.22 * w);          // negatif olabilir: pow değil kare
    float sirt = exp(-u_sirt * u_sirt);

    if (goruntu_var > 0.5)
    {
        if (alfa_bant <= 0.0)
            return float4(0, 0, 0, 0);
        // Kromatik sapma: kanallar biraz farklı miktarda kırılır.
        float sapma = 0.22 + 0.25 * saturate(siddet);
        float2 uvr = (px - yon * otele * (1.0 + sapma)) / cozunurluk;
        float2 uvg = (px - yon * otele) / cozunurluk;
        float2 uvb = (px - yon * otele * (1.0 - sapma)) / cozunurluk;
        float3 c;
        c.r = masaustu.SampleLevel(orneklec, uvr, 0).r;
        c.g = masaustu.SampleLevel(orneklec, uvg, 0).g;
        c.b = masaustu.SampleLevel(orneklec, uvb, 0).b;
        // Sırtta hafif aydınlanma, arkadaki sıkışmada çok hafif gölgelenme.
        c += lerp(renk1.rgb, float3(1, 1, 1), 0.4) * sirt * 0.22 * guc * zarf_;
        c *= 1.0 - 0.10 * saturate(-profil) * zarf_;
        // Bant kenarına doğru bükülme zaten sıfıra iner; alfa yumuşakça gerçek masaüstüne karışır.
        return cikis(c, alfa_bant, px);
    }

    // Yedek: parlak halka (kenarlarında çok hafif renk ayrışması); arkasında yumuşak, kısa iz.
    float c0 = max(1.8 * s, 0.9);
    float cekirdek = exp(-(d * d) / (2.0 * c0 * c0));
    float hale = exp(-abs(d) / (12.0 * s)) * 0.40 + exp(-abs(d) / (40.0 * s)) * 0.12;
    float halka = cekirdek + hale;
    // Ayrışma: halkanın dış yüzü renk2'ye, iç yüzü renk1'e çalar (gökkuşağı değil, iki ton).
    float yuz = smoothstep(-6.0 * s, 6.0 * s, d);

    float iz_boyu = lerp(40.0, 150.0, t) * s;
    float iz = (d < 0.0 ? exp(d / iz_boyu) : 0.0) * 0.13 * smoothstep(0.0, 1.0, R / (3.0 * w));

    float3 renk_halka = lerp(lerp(renk1.rgb, renk2.rgb, yuz * 0.6), float3(1, 1, 1), cekirdek * 0.6);
    float3 isik = renk_halka * halka + lerp(renk1.rgb, renk2.rgb, saturate(-d / iz_boyu)) * iz;
    float yogunluk = halka + iz;
    float3 renk = isik / max(yogunluk, 1e-4);
    // Yedekte bant sınırı genişletilir (iz halkanın arkasında uzanır).
    float yedek_bant = 1.0 - smoothstep(iz_boyu * 2.5, iz_boyu * 3.0 + 2.0 * w, abs(d));
    return cikis(renk, saturate(yogunluk) * zarf_ * guc * yedek_bant, px);
}
