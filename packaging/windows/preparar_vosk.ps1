param(
  [string]$ReleaseDir = (Join-Path $PSScriptRoot '..\..\app\build\windows\x64\runner\Release')
)

$ErrorActionPreference = 'Stop'
$release = [System.IO.Path]::GetFullPath($ReleaseDir)
if (-not (Test-Path -LiteralPath $release -PathType Container)) {
  throw "No existe la carpeta Release: $release. Compila primero con flutter build windows."
}

$workRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$work = Join-Path $workRoot ("dictar-vosk-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null

function Descargar-Archivo([string]$Uri, [string]$Destino) {
  & curl.exe --fail --location --retry 5 --retry-all-errors --retry-delay 2 --max-time 900 --output $Destino $Uri
  if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $Destino -PathType Leaf)) {
    throw "No se pudo descargar: $Uri"
  }
}

try {
  $models = Join-Path $release 'models'
  $voskFilesReady = @('libvosk.dll', 'libgcc_s_seh-1.dll', 'libstdc++-6.dll', 'libwinpthread-1.dll') |
    ForEach-Object { Test-Path -LiteralPath (Join-Path $release $_) -PathType Leaf } |
    Where-Object { -not $_ } | Measure-Object | Select-Object -ExpandProperty Count
  $voskFilesReady = $voskFilesReady -eq 0 -and
    (Test-Path -LiteralPath (Join-Path $models 'vosk-model-small-es-0.42\am\final.mdl')) -and
    (Test-Path -LiteralPath (Join-Path $models 'vosk-model-small-en-us-0.15\am\final.mdl')) -and
    (Test-Path -LiteralPath (Join-Path $release 'Vosk-LICENSE.txt'))
  if (-not $voskFilesReady) {
    $runtimeZip = Join-Path $work 'runtime.zip'
    $modelEsZip = Join-Path $work 'model-es.zip'
    $modelEnZip = Join-Path $work 'model-en.zip'
    Descargar-Archivo 'https://github.com/alphacep/vosk-api/releases/download/v0.3.45/vosk-win64-0.3.45.zip' $runtimeZip
    Descargar-Archivo 'https://alphacephei.com/vosk/models/vosk-model-small-es-0.42.zip' $modelEsZip
    Descargar-Archivo 'https://alphacephei.com/vosk/models/vosk-model-small-en-us-0.15.zip' $modelEnZip
    $modelEnHash = (Get-FileHash -LiteralPath $modelEnZip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($modelEnHash -ne '30f26242c4eb449f948e42cb302dd7a686cb29a3423a8367f99ff41780942498') {
      throw "El SHA-256 del modelo inglés no coincide: $modelEnHash"
    }
    Expand-Archive -LiteralPath $runtimeZip -DestinationPath $work
    Expand-Archive -LiteralPath $modelEsZip -DestinationPath $work
    Expand-Archive -LiteralPath $modelEnZip -DestinationPath $work

    $runtime = Join-Path $work 'vosk-win64-0.3.45'
    foreach ($dll in @('libvosk.dll', 'libgcc_s_seh-1.dll', 'libstdc++-6.dll', 'libwinpthread-1.dll')) {
      $source = Join-Path $runtime $dll
      if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
        throw "El paquete oficial de Vosk no contiene $dll."
      }
      Copy-Item -LiteralPath $source -Destination $release -Force
    }

    New-Item -ItemType Directory -Force -Path $models | Out-Null
    foreach ($modelName in @('vosk-model-small-es-0.42', 'vosk-model-small-en-us-0.15')) {
      $modelSource = Join-Path $work $modelName
      if (-not (Test-Path -LiteralPath (Join-Path $modelSource 'am\final.mdl') -PathType Leaf)) {
        throw "El archivo descargado no contiene el modelo esperado: $modelName"
      }
      Copy-Item -LiteralPath $modelSource -Destination $models -Recurse -Force
    }
    Descargar-Archivo 'https://www.apache.org/licenses/LICENSE-2.0.txt' (Join-Path $release 'Vosk-LICENSE.txt')
  }

  # Sherpa-ONNX Spanish streaming model, pinned to an immutable revision and
  # verified file-by-file before packaging.
  $sherpaModelName = 'sherpa-onnx-streaming-zipformer-es-kroko-2025-08-06'
  $sherpaRevision = '20cf7a4921613397841d31168796cade5b866585'
  $sherpaHashes = @{
    'encoder.onnx' = '2d9f5ef87d1a5257f8a6687e21501c56f3aa2fcbfcfab9364dcc4ce4e06ae81b'
    'decoder.onnx' = 'd4ce176b94b25f7acc88717bc3f704fcf5d6e131aaac2e0cabab3885541181ee'
    'joiner.onnx' = 'dae35df88d676e320fcdb99217328e66dcf722bf11b0f2459e14ddb5b982ded5'
    'tokens.txt' = '1be5e0a58e05d06d327df4c6b7b5e4f8aba01da6981eb016fcaceafc6a56680f'
  }
  $sherpaTemp = Join-Path $work 'sherpa-es'
  New-Item -ItemType Directory -Force -Path $sherpaTemp | Out-Null
  foreach ($file in $sherpaHashes.Keys) {
    $destination = Join-Path $sherpaTemp $file
    $uri = "https://huggingface.co/csukuangfj/$sherpaModelName/resolve/$sherpaRevision/${file}?download=true"
    # Hugging Face serves large LFS objects through redirects; curl handles
    # those responses more reliably than Windows PowerShell's web cmdlet.
    Descargar-Archivo $uri $destination
    $hash = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($hash -ne $sherpaHashes[$file]) {
      throw "Sherpa model SHA-256 mismatch for ${file}: $hash"
    }
  }
  $models = Join-Path $release 'models'
  $sherpaPackagedModel = Join-Path $models $sherpaModelName
  New-Item -ItemType Directory -Force -Path $sherpaPackagedModel | Out-Null
  Get-ChildItem -LiteralPath $sherpaTemp -File | Copy-Item -Destination $sherpaPackagedModel -Force
  Descargar-Archivo 'https://creativecommons.org/licenses/by-sa/4.0/legalcode.txt' (Join-Path $release 'CC-BY-SA-4.0.txt')
  Descargar-Archivo 'https://raw.githubusercontent.com/k2-fsa/sherpa-onnx/v1.13.8/LICENSE' (Join-Path $release 'Sherpa-ONNX-LICENSE.txt')
  $atribucion = @'
Spanish streaming ASR model: sherpa-onnx-streaming-zipformer-es-kroko-2025-08-06
Model author/source: Banafo, Kroko ASR community model.
Model repository: https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-es-kroko-2025-08-06
Model license: Creative Commons Attribution-ShareAlike 4.0 International (CC BY-SA 4.0).
The model files are distributed unmodified. The sherpa-onnx runtime is separately licensed under Apache License 2.0.
'@
  [System.IO.File]::WriteAllText((Join-Path $release 'Sherpa-Spanish-MODEL-ATTRIBUTION.txt'), $atribucion, [System.Text.UTF8Encoding]::new($false))

  $repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
  $cargoTarget = $env:CARGO_TARGET_DIR
  if ([string]::IsNullOrWhiteSpace($cargoTarget)) {
    $cargoTarget = Join-Path $repoRoot 'target'
  }
  $sherpaRuntime = Join-Path ([System.IO.Path]::GetFullPath($cargoTarget)) 'release'
  foreach ($dll in @('sherpa-onnx-c-api.dll', 'sherpa-onnx-cxx-api.dll', 'onnxruntime.dll', 'onnxruntime_providers_shared.dll')) {
    $source = Join-Path $sherpaRuntime $dll
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
      throw "Missing $dll in $sherpaRuntime. Build dictar-api with Sherpa-ONNX shared before packaging."
    }
    Copy-Item -LiteralPath $source -Destination $release -Force
  }

  # Tiny is the default live Whisper model when no model preference exists.
  # Pin the upstream revision and verify the exact artifact before packaging.
  $tiny = Join-Path $work 'ggml-tiny-q5_1.bin'
  Descargar-Archivo 'https://huggingface.co/ggerganov/whisper.cpp/resolve/fb5ed9369c26eaf08392504d8446da431e0fffb9/ggml-tiny-q5_1.bin?download=true' $tiny
  $tinyHash = (Get-FileHash -LiteralPath $tiny -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($tinyHash -ne '818710568da3ca15689e31a743197b520007872ff9576237bda97bd1b469c3d7') {
    throw "El SHA-256 del modelo Whisper Tiny no coincide: $tinyHash"
  }
  Copy-Item -LiteralPath $tiny -Destination (Join-Path $models 'ggml-tiny-q5_1.bin') -Force
  Descargar-Archivo 'https://raw.githubusercontent.com/ggml-org/whisper.cpp/master/LICENSE' (Join-Path $release 'Whisper-MODEL-LICENSE.txt')

  if (-not (Test-Path -LiteralPath (Join-Path $release 'libvosk.dll'))) {
    throw 'No se copió libvosk.dll a Release.'
  }
  foreach ($dll in @('sherpa-onnx-c-api.dll', 'sherpa-onnx-cxx-api.dll', 'onnxruntime.dll', 'onnxruntime_providers_shared.dll')) {
    if (-not (Test-Path -LiteralPath (Join-Path $release $dll))) {
      throw "No se copió el runtime Sherpa-ONNX requerido: ${dll}"
    }
  }
  foreach ($file in $sherpaHashes.Keys) {
    if (-not (Test-Path -LiteralPath (Join-Path $sherpaPackagedModel $file))) {
      throw "No se empaquetó el archivo Sherpa requerido: $file"
    }
  }
  Write-Host 'Modelos locales preparados en Release (Sherpa-ONNX, Vosk, Whisper Tiny y licencias).'
}
finally {
  $resolvedWork = [System.IO.Path]::GetFullPath($work)
  if (-not $resolvedWork.StartsWith($workRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Se cancela la limpieza: la ruta temporal salió de $workRoot."
  }
  Remove-Item -LiteralPath $resolvedWork -Recurse -Force
}
