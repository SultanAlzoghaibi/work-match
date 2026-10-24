# Builds a self-contained llama-server.exe (static, CPU backend) and copies it
# to llama-server-<target-triple>.exe, the name Tauri sidecars need. The
# official Windows release splits llama-server into an exe plus llama/ggml/
# OpenMP DLLs, which a single-file sidecar can't carry. Needs Visual Studio 2022
# with "Desktop development with C++" (MSVC; its bundled cmake is used when
# cmake isn't on PATH) and git. Windows counterpart of build-llama-server.sh.
#
#   powershell -ExecutionPolicy Bypass -File build-llama-server.ps1
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
$tag = if ($env:LLAMA_TAG) { $env:LLAMA_TAG } else { "b11669" }
$src = Join-Path $env:TEMP "llama.cpp-$tag"

$vs = & "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * `
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw "Visual Studio with the C++ tools isn't installed" }
$cmake = (Get-Command cmake -ErrorAction SilentlyContinue).Source
if (-not $cmake) { $cmake = "$vs\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe" }

if (-not (Test-Path $src)) {
    git clone --depth 1 --branch $tag https://github.com/ggml-org/llama.cpp $src
    if ($LASTEXITCODE -ne 0) { throw "git clone failed" }
}
# Static CRT (MultiThreaded) so the exe doesn't need the VC++ redistributable;
# OpenMP off because MSVC's OpenMP runtime is only a DLL (ggml then uses its
# own thread pool). CMP0091 makes cmake honour CMAKE_MSVC_RUNTIME_LIBRARY.
& $cmake -S $src -B "$src\build" `
    -DBUILD_SHARED_LIBS=OFF -DGGML_OPENMP=OFF `
    -DCMAKE_POLICY_DEFAULT_CMP0091=NEW -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded `
    -DLLAMA_CURL=OFF -DLLAMA_OPENSSL=OFF `
    -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_EXAMPLES=OFF
if ($LASTEXITCODE -ne 0) { throw "cmake configure failed" }
& $cmake --build "$src\build" --config Release --target llama-server -j
if ($LASTEXITCODE -ne 0) { throw "cmake build failed" }

$triple = (rustc -vV 2>$null | Select-String '^host: (.+)$').Matches.Groups[1].Value
if (-not $triple) { $triple = "x86_64-pc-windows-msvc" }
$out = "llama-server-$triple.exe"
# Fresh timestamp, or cargo keeps the old sidecar (see README.md).
Copy-Item "$src\build\bin\Release\llama-server.exe" $out -Force
(Get-Item $out).LastWriteTime = Get-Date
Write-Output "built binaries\$out"
# Should list only Windows system DLLs (KERNEL32, ADVAPI32, WS2_32, ...),
# no llama, ggml, vcruntime or OpenMP DLLs.
$dumpbin = Get-ChildItem "$vs\VC\Tools\MSVC\*\bin\Hostx64\x64\dumpbin.exe" | Select-Object -First 1
& $dumpbin.FullName /nologo /dependents $out
