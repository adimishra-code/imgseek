use std::path::Path;

const DENY_CRATES: &[&str] = &[
    "tch",
    "torch-sys",
    "ort",
    "onnxruntime",
    "onnxruntime-sys",
    "tract-core",
    "tract-onnx",
    "candle-core",
    "candle-nn",
    "burn",
    "burn-core",
    "tensorflow",
    "tensorflow-sys",
    "tflite",
    "dfdx",
    "rust-bert",
    "llm",
    "openvino",
    "openvino-sys",
    "mxnet",
];

const FORBIDDEN_EXTENSIONS: &[&str] = &[
    "onnx",
    "pt",
    "pth",
    "safetensors",
    "h5",
    "pb",
    "tflite",
    "gguf",
    "ckpt",
];

#[test]
fn test_no_forbidden_ml_crates_in_lockfile() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let lockfile_path = manifest_dir.join("../../Cargo.lock");
    if lockfile_path.exists() {
        let lock_content =
            std::fs::read_to_string(&lockfile_path).expect("Failed to read Cargo.lock");
        for crate_name in DENY_CRATES {
            let needle = format!("name = \"{}\"", crate_name);
            assert!(
                !lock_content.contains(&needle),
                "Forbidden ML crate '{}' found in Cargo.lock",
                crate_name
            );
        }
    }
}

#[test]
fn test_no_forbidden_model_files_in_workspace() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root_dir = manifest_dir.join("../..");
    check_dir_recursive(&root_dir);
}

fn check_dir_recursive(dir: &Path) {
    if !dir.is_dir() {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if file_name == "target" || file_name == ".git" || file_name == ".imgseek" {
            continue;
        }
        if path.is_dir() {
            check_dir_recursive(&path);
        } else if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            assert!(
                !FORBIDDEN_EXTENSIONS.contains(&ext),
                "Forbidden model file found: {:?}",
                path
            );
        }
    }
}
