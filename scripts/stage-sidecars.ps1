param(
  [ValidateSet("debug", "release")]
  [string]$Profile = "release",
  [string]$TargetTriple = "x86_64-pc-windows-msvc"
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$srcTauriDir = Join-Path $repoRoot "src-tauri"
$binariesDir = Join-Path $srcTauriDir "binaries"
$cargoExe = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
$cmakeScriptsDir = Join-Path $env:APPDATA "Python\Python314\Scripts"
$vcvarsCandidates = @(
  "C:\Program Files\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat",
  "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat",
  "C:\Program Files\Microsoft Visual Studio\18\Community\VC\Auxiliary\Build\vcvars64.bat"
)
$vcvarsPath = $vcvarsCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $vcvarsPath) {
  throw "Unable to locate vcvars64.bat. Install the Visual Studio Desktop C++ toolchain first."
}

if (-not (Test-Path $cargoExe)) {
  throw "Cargo was not found at $cargoExe"
}

$cmakeExe = Join-Path $cmakeScriptsDir "cmake.exe"
if (-not (Test-Path $cmakeExe)) {
  throw "cmake.exe was not found at $cmakeExe. Install the user-local cmake package first."
}

New-Item -ItemType Directory -Force -Path $binariesDir | Out-Null

$sidecars = @("wisprtype-stt-worker", "wisprtype-refinement-worker")
foreach ($sidecar in $sidecars) {
  $placeholder = Join-Path $binariesDir "$sidecar-$TargetTriple.exe"
  if (-not (Test-Path $placeholder)) {
    New-Item -ItemType File -Path $placeholder | Out-Null
  }
}

$cmakeBuildDir = Join-Path $srcTauriDir "target\$Profile\build"
if (Test-Path $cmakeBuildDir) {
  Get-ChildItem $cmakeBuildDir -Directory -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -like 'whisper-rs-sys-*' -or $_.Name -like 'llama-cpp-sys-2-*' } |
    Remove-Item -Recurse -Force
}

$profileFlag = if ($Profile -eq "release") { "--release" } else { "" }
$builds = @(
  @{
    Bin = "wisprtype-stt-worker"
    Feature = "stt-engine"
    TargetDir = Join-Path $srcTauriDir "target\sidecar-stt"
  },
  @{
    Bin = "wisprtype-refinement-worker"
    Feature = "refinement-engine"
    TargetDir = Join-Path $srcTauriDir "target\sidecar-refinement"
  }
)

Write-Host "Building sidecars for profile '$Profile'..."
foreach ($build in $builds) {
  $buildCommand = @(
    "cd /d `"$srcTauriDir`"",
    "set PATH=$cmakeScriptsDir;%PATH%",
    "set ""CMAKE_GENERATOR=NMake Makefiles""",
    "set ""CMAKE_GENERATOR_INSTANCE=""",
    "set ""CARGO_TARGET_DIR=$($build.TargetDir)""",
    """$cargoExe"" build $profileFlag --features ""$($build.Feature)"" --bin $($build.Bin)"
  ) -join " && "

  $fullCommand = "call `"$vcvarsPath`" >nul && $buildCommand"
  cmd /c $fullCommand
  if ($LASTEXITCODE -ne 0) {
    throw "Sidecar build failed for $($build.Bin) with exit code $LASTEXITCODE"
  }
}

foreach ($build in $builds) {
  $source = Join-Path $build.TargetDir "$Profile\$($build.Bin).exe"
  if (-not (Test-Path $source)) {
    throw "Built sidecar not found: $source"
  }

  $destination = Join-Path $binariesDir "$($build.Bin)-$TargetTriple.exe"
  Copy-Item -LiteralPath $source -Destination $destination -Force
  Write-Host "Staged $destination"
}
