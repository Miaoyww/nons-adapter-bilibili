use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    if let Some(root) = std::env::var_os("NONS_ROOT") {
        return PathBuf::from(root);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../Nons")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("../../../Nons"))
}

fn main() {
    let root = workspace_root();
    let source = [
        root.join("plugins/wit/plugin.wit"),
        root.join("packages/adapter-host/wit/host.wit"),
    ];
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("generated-wit");
    std::fs::create_dir_all(&output).unwrap();
    for file in source {
        let name = file.file_name().unwrap();
        std::fs::copy(&file, output.join(name))
            .unwrap_or_else(|error| panic!("copy WIT {} failed: {error}", file.display()));
        println!("cargo:rerun-if-changed={}", file.display());
    }
    println!("cargo:rerun-if-env-changed=NONS_ROOT");
}
