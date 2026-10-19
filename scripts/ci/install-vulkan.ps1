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
foreach ($file in @('Include/vulkan/vulkan.h', 'Include/spirv/unified1/spirv.hpp', 'Bin/glslc.exe', 'Lib/vulkan-1.lib')) {
    if (!(Test-Path (Join-Path $sdk $file))) { throw "Incomplete Vulkan SDK: missing $sdk/$file" }
}
"VULKAN_SDK=$sdk" | Out-File $env:GITHUB_ENV -Append -Encoding utf8
"CMAKE_PREFIX_PATH=$sdk" | Out-File $env:GITHUB_ENV -Append -Encoding utf8
"$sdk\Bin" | Out-File $env:GITHUB_PATH -Append -Encoding utf8
& "$sdk\Bin\glslc.exe" --version
if ($LASTEXITCODE -ne 0) { throw 'Vulkan shader compiler validation failed' }
