$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$destination = Join-Path $root 'web/assets/chart.umd.min.js'
Invoke-WebRequest -Uri 'https://cdn.jsdelivr.net/npm/chart.js@4.5.1/dist/chart.umd.min.js' -OutFile $destination
$expected = '48444a82d4edcb5bec0f1965faacdde18d9c17db3063d042abada2f705c9f54a'
$actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $destination).Hash.ToLowerInvariant()
if ($actual -ne $expected) { throw "Chart.js checksum mismatch: $actual" }

$hammer = Join-Path $root 'web/assets/hammer.min.js'
Invoke-WebRequest -Uri 'https://cdn.jsdelivr.net/npm/hammerjs@2.0.8/hammer.min.js' -OutFile $hammer
$hammerExpected = '7953631f0e54794d2352a3cfa591c0914d73e14f90141058e3cf16bee7939bcf'
$hammerActual = (Get-FileHash -Algorithm SHA256 -LiteralPath $hammer).Hash.ToLowerInvariant()
if ($hammerActual -ne $hammerExpected) { throw "Hammer.js checksum mismatch: $hammerActual" }

$zoom = Join-Path $root 'web/assets/chartjs-plugin-zoom.min.js'
Invoke-WebRequest -Uri 'https://cdn.jsdelivr.net/npm/chartjs-plugin-zoom@2.2.0/dist/chartjs-plugin-zoom.min.js' -OutFile $zoom
$zoomExpected = 'e4a088e5bab93be6ee47c939eeb9ebaa80e0b39156d4bdfd1af9c844be81b6c4'
$zoomActual = (Get-FileHash -Algorithm SHA256 -LiteralPath $zoom).Hash.ToLowerInvariant()
if ($zoomActual -ne $zoomExpected) { throw "Chart.js zoom plugin checksum mismatch: $zoomActual" }
