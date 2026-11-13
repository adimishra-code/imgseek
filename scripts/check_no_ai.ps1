# PowerShell No-AI verification script
$ErrorActionPreference = "Stop"

Write-Host "Running No-AI verification..."

$denyRegex = '^(tch|torch-sys|ort|onnxruntime.*|tract-.*|candle-.*|burn(-.*)?|tensorflow.*|tflite.*|dfdx|rust-bert|llm|llama.*|openvino.*|mxnet.*)$'
$tree = cargo tree --workspace --all-features --prefix none 2>$null

$foundCrates = 0
foreach ($line in ($tree -split "`n")) {
    $crate = ($line.Trim() -split '\s+')[0]
    if ([string]::IsNullOrWhiteSpace($crate)) { continue }
    if ($crate -match $denyRegex) {
        Write-Error "Forbidden ML/AI crate found: $crate"
        $foundCrates = 1
    }
}

if ($foundCrates -ne 0) {
    exit 1
}

$forbiddenExts = '\.(onnx|pt|pth|safetensors|h5|pb|tflite|gguf|ckpt)$'
$tracked = git ls-files
$foundFiles = 0
foreach ($file in ($tracked -split "`n")) {
    $file = $file.Trim()
    if ([string]::IsNullOrWhiteSpace($file)) { continue }
    if ($file -match $forbiddenExts) {
        Write-Error "Forbidden model file tracked: $file"
        $foundFiles = 1
    }
}

if ($foundFiles -ne 0) {
    exit 1
}

Write-Host "No-AI check passed: 0 ML crates, 0 model files."
