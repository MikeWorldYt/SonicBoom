# Pin reviewed SDK bytes. Source: LunarG /sdk/sha/1.4.328.1/windows/vulkan_sdk.exe.json
$ErrorActionPreference = 'Stop'
$version = '1.4.328.1'
$sha256 = 'a8675df6d538079c2a719a9373994948091db785b48f142e024254e76348d16c'
$installer = Join-Path $env:RUNNER_TEMP 'vulkan-sdk.exe'
Invoke-WebRequest "https://sdk.lunarg.com/sdk/download/$version/windows/vulkan_sdk.exe" -OutFile $installer
if ((Get-FileHash $installer -Algorithm SHA256).Hash.ToLowerInvariant() -ne $sha256) {
    throw 'LunarG Vulkan SDK SHA256 mismatch; refusing to execute installer'
}
$sdk = "C:\VulkanSDK\$version"
$process = Start-Process $installer -Wait -PassThru -ArgumentList @('--root', $sdk, '--accept-licenses', '--default-answer', '--confirm-command', 'install')
if ($process.ExitCode -ne 0) { throw "Vulkan SDK installer failed: $($process.ExitCode)" }
foreach ($file in @('Include/vulkan/vulkan.h', 'Bin/glslc.exe', 'Lib/vulkan-1.lib')) {
    if (!(Test-Path (Join-Path $sdk $file))) { throw "Incomplete Vulkan SDK: missing $sdk/$file" }
}
# The LunarG default install does not ship SPIRV-Headers (no
# Include/spirv/unified1/spirv.hpp), which ggml-vulkan includes directly.
# Install the pinned upstream headers separately, with integrity check.
$spirvCommit = '86f980c731e62ae4eaf383d320449d71687936bf'
$spirvSha256 = '5abe65f980fad5c3365a1060acf3f16c552dea41db40aae54a27dfbebb840995'
$spirvZip = Join-Path $env:RUNNER_TEMP 'spirv-headers.zip'
Invoke-WebRequest "https://github.com/KhronosGroup/SPIRV-Headers/archive/$spirvCommit.zip" -OutFile $spirvZip
if ((Get-FileHash $spirvZip -Algorithm SHA256).Hash.ToLowerInvariant() -ne $spirvSha256) {
    throw 'SPIRV-Headers SHA256 mismatch; refusing to use downloaded headers'
}
$spirvSource = Join-Path $env:RUNNER_TEMP 'spirv-headers-src'
Expand-Archive $spirvZip -DestinationPath $spirvSource -Force
$spirvRoot = Join-Path $env:RUNNER_TEMP 'spirv-headers'
cmake -S (Join-Path $spirvSource "SPIRV-Headers-$spirvCommit") -B (Join-Path $env:RUNNER_TEMP 'spirv-headers-build') "-DCMAKE_INSTALL_PREFIX=$spirvRoot"
if ($LASTEXITCODE -ne 0) { throw 'SPIRV-Headers CMake configure failed' }
cmake --install (Join-Path $env:RUNNER_TEMP 'spirv-headers-build')
if ($LASTEXITCODE -ne 0) { throw 'SPIRV-Headers CMake install failed' }
if (!(Test-Path (Join-Path $spirvRoot 'include/spirv/unified1/spirv.hpp'))) {
    throw "Incomplete SPIRV-Headers install: missing $spirvRoot/include/spirv/unified1/spirv.hpp"
}
"VULKAN_SDK=$sdk" | Out-File $env:GITHUB_ENV -Append -Encoding utf8
"CMAKE_PREFIX_PATH=$sdk;$spirvRoot" | Out-File $env:GITHUB_ENV -Append -Encoding utf8
"SONICBOOM_SPIRV_INCLUDE_DIR=$spirvRoot/include" | Out-File $env:GITHUB_ENV -Append -Encoding utf8
"$sdk\Bin" | Out-File $env:GITHUB_PATH -Append -Encoding utf8
& "$sdk\Bin\glslc.exe" --version
if ($LASTEXITCODE -ne 0) { throw 'Vulkan shader compiler validation failed' }
