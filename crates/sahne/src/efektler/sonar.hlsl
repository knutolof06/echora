// Sonar — imleçten arka arkaya yayılan üç radar halkası. Her halka ince parlak bir çizgi,
// arkasında içe doğru sönen yumuşak bir dolgu; yayıldıkça incelir ve söner. İmlecin altında
// halkalarla birlikte atan küçük bir ışık noktası. Halkalar renk1'den renk2'ye geçer.

// Tek halka. x: çizgi yoğunluğu, y: dolgu yoğunluğu.
float2 halka(float r, float u, float Rmax, float s)
{
    if (u <= 0.0 || u >= 1.0)
        return float2(0, 0);
    float R = kolay_cikis(u) * Rmax;
    float d = r - R;
    float sonum = pow(1.0 - u, 1.6) * smoothstep(0.0, 0.08, u);
    float c = lerp(2.0, 1.2, u) * s;
    float cekirdek = exp(-(d * d) / (2.0 * c * c));
    float hale = exp(-abs(d) / (13.0 * s)) * 0.40 + exp(-abs(d) / (45.0 * s)) * 0.10;
    // Arkadaki dolgu: halkanın içinde, halkaya yaklaştıkça artan çok hafif "tarama" izi.
    float dolgu = d < 0.0 ? exp(d / (55.0 * s)) * 0.16 * smoothstep(0.0, 30.0 * s, R) : 0.0;
    return float2((cekirdek + hale) * sonum, dolgu * sonum);
}

float4 ps_ana(Girdi g) : SV_Target
{
    float2 px = g.konum.xy;
    float t = saturate(zaman);
    float s = px_olcek();
    float r = length(px - imlec);

    float Rmax = lerp(200.0, 320.0, saturate(siddet)) * s;
    if (r > Rmax + 160.0 * s)
        return float4(0, 0, 0, 0);

    const float omur = 0.64;
    float3 isik = 0;
    float cizgi_top = 0, dolgu_top = 0;
    float3 dolgu_isik = 0;
    [unroll]
    for (int i = 0; i < 3; i++)
    {
        float fi = (float)i;
        float u = (t - fi * 0.18) / omur;
        float2 h = halka(r, u, Rmax * (1.0 - 0.08 * fi), s);
        float3 rk = lerp(renk1.rgb, renk2.rgb, fi * 0.5);
        isik += lerp(rk, float3(1, 1, 1), 0.25) * h.x;
        cizgi_top += h.x;
        dolgu_isik += rk * h.y;
        dolgu_top += h.y;
    }

    // İmleçteki ışık noktası: her halka çıkışında hafifçe atar.
    float atis = 0;
    [unroll]
    for (int j = 0; j < 3; j++)
    {
        float u = (t - (float)j * 0.18) / 0.22;
        atis += (u > 0.0 && u < 1.0) ? sin(PI * u) : 0.0;
    }
    float nokta = (exp(-r / (4.0 * s)) * 0.8 + exp(-r / (16.0 * s)) * 0.25) * saturate(atis);
    isik += lerp(renk1.rgb, float3(1, 1, 1), 0.5) * nokta;
    cizgi_top += nokta;

    float guc = lerp(0.6, 1.0, saturate(siddet)) * seviye;
    float zarf_ = smoothstep(0.0, 0.03, t) * (1.0 - smoothstep(0.97, 1.0, t));
    float4 ust = cikis(isik / max(cizgi_top, 1e-4), saturate(cizgi_top) * guc * zarf_, px);
    float4 alt = cikis(dolgu_isik / max(dolgu_top, 1e-4), saturate(dolgu_top) * guc * zarf_, px);
    return ust_uste(ust, alt);
}
