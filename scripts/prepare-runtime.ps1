param()
$ErrorActionPreference='Stop'
$root=Split-Path $PSScriptRoot -Parent
$archive=Join-Path $root 'runtime\pdfium-win-x64.tgz'
$expected='D20A06B7049FE6679F8CF4B0BC4CC312CBEFC6534C6BC4D412DDFEA40460B745'
New-Item -ItemType Directory -Force (Join-Path $root 'runtime') | Out-Null
if (!(Test-Path $archive)) { Invoke-WebRequest 'https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/7725/pdfium-win-x64.tgz' -OutFile $archive }
if ((Get-FileHash $archive -Algorithm SHA256).Hash -ne $expected) { throw 'PDFium SHA-256 mismatch' }
$destination=Join-Path $root 'runtime\pdfium'
New-Item -ItemType Directory -Force $destination | Out-Null
tar -xzf $archive -C $destination
if ($LASTEXITCODE -ne 0) {throw 'PDFium extraction failed'}
