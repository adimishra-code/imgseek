#!/usr/bin/env bash
set -euo pipefail

echo "Running No-AI verification..."

# 1. Check Cargo tree for forbidden ML/AI dependencies
DENY_REGEX='^(tch|torch-sys|ort|onnxruntime.*|tract-.*|candle-.*|burn(-.*)?|tensorflow.*|tflite.*|dfdx|rust-bert|llm|llama.*|openvino.*|mxnet.*)$'

TREE_OUTPUT=$(cargo tree --workspace --all-features --prefix none 2>/dev/null | awk '{print $1}' | sort -u || true)

FOUND_CRATES=0
while IFS= read -r crate; do
    if [[ -z "$crate" ]]; then
        continue
    fi
    if echo "$crate" | grep -Eq "$DENY_REGEX"; then
        echo "ERROR: Forbidden ML/AI crate found in workspace dependencies: $crate" >&2
        FOUND_CRATES=1
    fi
done <<< "$TREE_OUTPUT"

if [ "$FOUND_CRATES" -ne 0 ]; then
    echo "FAILED: ML dependencies detected." >&2
    exit 1
fi

# 2. Check tracked files for forbidden pretrained model files
FORBIDDEN_EXTS='\.(onnx|pt|pth|safetensors|h5|pb|tflite|gguf|ckpt)$'
TRACKED_FILES=$(git ls-files || true)

FOUND_FILES=0
while IFS= read -r file; do
    if [[ -z "$file" ]]; then
        continue
    fi
    if echo "$file" | grep -Eq "$FORBIDDEN_EXTS"; then
        echo "ERROR: Forbidden model file tracked in git: $file" >&2
        FOUND_FILES=1
    fi
done <<< "$TRACKED_FILES"

if [ "$FOUND_FILES" -ne 0 ]; then
    echo "FAILED: Model files detected in repository." >&2
    exit 1
fi

# 3. Check Python requirements files if any exist
PYTHON_REQ_DENY='^(torch|tensorflow|transformers|onnx.*|scikit-learn|keras)\b'
if compgen -G "python/requirements*.txt" > /dev/null; then
    for req in python/requirements*.txt; do
        if grep -Ei "$PYTHON_REQ_DENY" "$req"; then
            echo "ERROR: Forbidden Python ML dependency found in $req" >&2
            exit 1
        fi
    done
fi

echo "No-AI check passed: 0 ML crates, 0 model files, 0 forbidden Python dependencies."

