<#
.SYNOPSIS
    Echora sürüm paketini üretir: dist\Echora-<sürüm>-win64\ klasörü, zip'i ve SHA256SUMS.txt.

.DESCRIPTION
    Yerelde ve GitHub Actions'ta aynı betik çalışır (PowerShell 5.1 uyumlu).
    1. `cargo build --release --locked -p yanki` (-DerlemeYok ile atlanır).
    2. dist\Echora-<sürüm>-win64\ altına: echora.exe, 4 Vosk/MinGW DLL'i,
       modeller\vosk-model-small-tr-0.3\ (tam klasör), README.md, UCUNCU-TARAF.txt.
    3. dist\Echora-<sürüm>-win64.zip (girdiler "Echora-<sürüm>-win64/" altında, "/" ayırıcılı,
       sıralı) ve dist\SHA256SUMS.txt (sha256sum biçimi).

    Sürüm `cargo metadata`'dan (workspace sürümü) okunur. Gerekenler: modeller\ altında Vosk modeli
    ve kütüphanesi (modeller\OKU.md); kütüphane başka yerdeyse VOSK_LIB_DIR.

.PARAMETER DerlemeYok
    Derlemeyi atlar; mevcut target\release\echora.exe kullanılır.

.PARAMETER Cikti
    Çıktı klasörü (varsayılan: depo kökünde dist).

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\araclar\paketle.ps1
#>
[CmdletBinding()]
param(
    [switch]$DerlemeYok,
    [string]$Cikti
)

$ErrorActionPreference = 'Stop'
$kok = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not $Cikti) { $Cikti = Join-Path $kok 'dist' }

$meta = & cargo metadata --no-deps --format-version 1 --manifest-path (Join-Path $kok 'Cargo.toml') | ConvertFrom-Json
if ($LASTEXITCODE -ne 0) { throw "cargo metadata başarısız (çıkış kodu $LASTEXITCODE)" }
$surum = ($meta.packages | Where-Object { $_.name -eq 'yanki' }).version
if (-not $surum -or $surum -notmatch '^\d+\.\d+\.\d+$') { throw "Sürüm okunamadı ya da X.Y.Z değil: '$surum'" }
Write-Host "Echora $surum paketleniyor"

$model = Join-Path $kok 'modeller\vosk-model-small-tr-0.3'
$vosk = if ($env:VOSK_LIB_DIR) { $env:VOSK_LIB_DIR } else { Join-Path $kok 'modeller\vosk-win64-0.3.45' }
$dller = 'libvosk.dll', 'libstdc++-6.dll', 'libgcc_s_seh-1.dll', 'libwinpthread-1.dll'
if (-not (Test-Path (Join-Path $model 'final.mdl'))) { throw "Tanıma modeli yok: $model (modeller\OKU.md)" }
foreach ($d in $dller) {
    if (-not (Test-Path (Join-Path $vosk $d))) { throw "Vosk kütüphanesi eksik: $(Join-Path $vosk $d) (modeller\OKU.md)" }
}

if (-not $DerlemeYok) {
    Write-Host 'Derleniyor (cargo build --release --locked -p yanki)...'
    Push-Location $kok
    try {
        & cargo build --release --locked -p yanki
        if ($LASTEXITCODE -ne 0) { throw "Derleme başarısız (çıkış kodu $LASTEXITCODE)" }
    } finally { Pop-Location }
}
$exe = Join-Path $kok 'target\release\echora.exe'
if (-not (Test-Path $exe)) { throw "Derlenmiş echora.exe yok: $exe" }

$ad = "Echora-$surum-win64"
$klasor = Join-Path $Cikti $ad
$zip = Join-Path $Cikti "$ad.zip"
$ozetler = Join-Path $Cikti 'SHA256SUMS.txt'
New-Item -ItemType Directory -Force -Path $Cikti | Out-Null
if (Test-Path $klasor) { Remove-Item $klasor -Recurse -Force }
foreach ($f in $zip, $ozetler) { if (Test-Path $f) { Remove-Item $f -Force } }

New-Item -ItemType Directory -Force -Path $klasor | Out-Null
Copy-Item $exe -Destination $klasor
foreach ($d in $dller) { Copy-Item (Join-Path $vosk $d) -Destination $klasor }
New-Item -ItemType Directory -Force -Path (Join-Path $klasor 'modeller') | Out-Null
Copy-Item $model -Destination (Join-Path $klasor 'modeller') -Recurse
Copy-Item (Join-Path $kok 'README.md') -Destination $klasor
Copy-Item (Join-Path $kok 'araclar\UCUNCU-TARAF.txt') -Destination $klasor

# Zip elle yazılır: PowerShell 5.1'in Compress-Archive'ı girdi adlarında "\" kullanabiliyor.
# Girdiler sıralı, "/" ayırıcılı ve "<ad>/" önekli; yalnız deflate.
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$dosyalar = Get-ChildItem $klasor -Recurse -File | Sort-Object { $_.FullName.Substring($klasor.Length + 1).Replace('\', '/') } -CaseSensitive
$akis = [System.IO.File]::Open($zip, [System.IO.FileMode]::CreateNew)
try {
    $arsiv = New-Object System.IO.Compression.ZipArchive($akis, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($f in $dosyalar) {
            $goreli = $f.FullName.Substring($klasor.Length + 1).Replace('\', '/')
            $girdi = $arsiv.CreateEntry("$ad/$goreli", [System.IO.Compression.CompressionLevel]::Optimal)
            $girdi.LastWriteTime = $f.LastWriteTime
            $hedef = $girdi.Open()
            try {
                $kaynak = [System.IO.File]::OpenRead($f.FullName)
                try { $kaynak.CopyTo($hedef) } finally { $kaynak.Dispose() }
            } finally { $hedef.Dispose() }
        }
    } finally { $arsiv.Dispose() }
} finally { $akis.Dispose() }

# sha256sum biçimi: "<küçük harf onaltılık>  <ad>", LF satır sonu, BOM'suz UTF-8.
$h = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLowerInvariant()
[System.IO.File]::WriteAllText($ozetler, "$h  $ad.zip`n", (New-Object System.Text.UTF8Encoding($false)))

# Özet: zip içeriği ve boyutlar.
$okuma = [System.IO.Compression.ZipFile]::OpenRead($zip)
try {
    Write-Host ''
    Write-Host "Zip içeriği ($($okuma.Entries.Count) girdi):"
    foreach ($g in $okuma.Entries) {
        '{0,12:N0}  {1}' -f $g.Length, $g.FullName | Write-Host
    }
    $acik = ($okuma.Entries | Measure-Object -Property Length -Sum).Sum
} finally { $okuma.Dispose() }
$boyut = (Get-Item $zip).Length
Write-Host ''
Write-Host ('{0}: {1:N0} bayt (açılmış {2:N0} bayt)' -f $zip, $boyut, $acik)
Write-Host "SHA-256: $h"
Write-Host "Özet dosyası: $ozetler"
