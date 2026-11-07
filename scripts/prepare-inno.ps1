param()
$ErrorActionPreference='Stop'
$root=Split-Path $PSScriptRoot -Parent
$destination=Join-Path $root '.tools\InnoSetup'
$archive=Join-Path $root '.tools\innosetup-6.7.3.exe'
New-Item -ItemType Directory -Force (Split-Path $archive) | Out-Null
if (!(Test-Path $archive)){Invoke-WebRequest 'https://github.com/jrsoftware/issrc/releases/download/is-6_7_3/innosetup-6.7.3.exe' -OutFile $archive}
if ((Get-FileHash $archive -Algorithm SHA256).Hash -ne '9C73C3BAE7ED48D44112A0F48E66742C00090BDB5BEF71D9D3C056C66E97B732'){throw 'Inno Setup SHA-256 mismatch'}
if (!(Test-Path "$destination\ISCC.exe")){
 $process=Start-Process -FilePath $archive -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/SP-','/CURRENTUSER',('/DIR="{0}"' -f $destination)) -WindowStyle Hidden -PassThru -Wait
 if ($process.ExitCode -ne 0){throw "Inno Setup installation failed: $($process.ExitCode)"}
}
