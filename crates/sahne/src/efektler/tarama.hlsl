// Tarama — soldan sağa geçen parlak ışık hüzmesi: ortada ince beyaza çalan çizgi, çevresinde
// yumuşak dikey bant, arkasında (solunda) yavaş sönen soluk iz. Bant dikeyde ortada biraz daha
// parlaktır. x sanal masaüstünde ölçülür: çok ekranda (Hepsi) hüzme ekranlar arasında kesintisiz geçer.

float4 ps_ana(Girdi g) : SV_Target
{
    float2 px = g.konum.xy;
    float t = saturate(zaman);
    float s = px_olcek();
    float H = cozunurluk.y;
    float Vx = max(sanal_boyut.x, cozunurluk.x);
    float X = px.x + ekran_ofseti.x;

    float bant_eni = lerp(50.0, 90.0, saturate(siddet)) * s;
    float hale_eni = 240.0 * s;
    // Hüzme ekran dışından girer, ekran dışına çıkar; ortada hafif hızlı (akıcı).
    float u = lerp(t, kolay_gc(t), 0.45);
    float bx = lerp(-3.0 * hale_eni, Vx + 3.0 * hale_eni, u);
    float d = X - bx;                                     // - : hüzmenin arkası

    float c = max(1.5 * s, 0.9);
    float cizgi = exp(-(d * d) / (2.0 * c * c));
    float bant = exp(-(d * d) / (2.0 * bant_eni * bant_eni));
    float hale = exp(-(d * d) / (2.0 * hale_eni * hale_eni)) * 0.35;
    // İz: hüzmenin arkasında, geçtiği yol boyunca soluk ışık (yaklaşık 0.35 ekran boyu).
    float iz_boyu = 0.35 * min(Vx, 2.0 * cozunurluk.x);
    // Hüzmenin hemen arkasında sıfırdan yumuşakça başlar (d = 0'da süreksizlik olmasın).
    float iz = (d < 0.0) ? exp(d / iz_boyu) * smoothstep(-iz_boyu * 3.0, 0.0, d) * 0.12 : 0.0;
    iz *= smoothstep(0.0, 1.5 * bant_eni, -d);

    // Dikey: ortada en parlak, üst/alt kenara doğru %35 söner.
    float yy = px.y / H * 2.0 - 1.0;
    float dikey = 1.0 - 0.35 * yy * yy;

    float3 r_cizgi = lerp(renk1.rgb, float3(1, 1, 1), 0.65);
    float3 r_bant = renk1.rgb;
    float3 r_hale = lerp(renk1.rgb, renk2.rgb, 0.5);
    float3 r_iz = lerp(renk2.rgb, float3(1, 1, 1), 0.30);

    float a_cizgi = cizgi * 0.9;
    float a_bant = bant * 0.42;
    float a_hale = hale * 0.5;
    float3 isik = r_cizgi * a_cizgi + r_bant * a_bant + r_hale * a_hale + r_iz * iz;
    float yogunluk = a_cizgi + a_bant + a_hale + iz;

    float guc = lerp(0.6, 1.0, saturate(siddet)) * seviye;
    float zarf_ = smoothstep(0.0, 0.05, t) * (1.0 - smoothstep(0.85, 1.0, t));
    return cikis(isik / max(yogunluk, 1e-4), saturate(yogunluk) * dikey * guc * zarf_, px);
}
