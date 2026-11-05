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
        let destination = output.join(name);
        std::fs::copy(&file, &destination)
            .unwrap_or_else(|error| panic!("copy WIT {} failed: {error}", file.display()));
        if name == "host.wit" {
            // Keep the generated copy on the transport version this adapter
            // imports while the main workspace catches up.
            let mut wit = std::fs::read_to_string(&destination).unwrap();
            let package = wit
                .lines()
                .find(|line| line.starts_with("package nons:host@"))
                .unwrap_or_else(|| panic!("{} has no nons:host package", file.display()));
            if package != "package nons:host@1.1.0;" {
                wit = wit.replacen(package, "package nons:host@1.1.0;", 1);
            }
            if !wit.contains("follow-redirects:") {
                let before = wit.clone();
                wit = wit.replacen(
                    "        body: list<u8>,",
                    "        body: list<u8>,\n        follow-redirects: bool,",
                    1,
                );
                assert_ne!(wit, before, "{} has no request body field", file.display());
            }
            let response = wit
                .lines()
                .find(|line| line.trim_start().starts_with("record response {"))
                .unwrap_or_else(|| panic!("{} has no response record", file.display()));
            if !response.contains("url: string") {
                let before = wit.clone();
                wit = wit.replacen(
                    "record response { status: u16, headers: list<header>, body: list<u8> }",
                    "record response { status: u16, headers: list<header>, body: list<u8>, url: string }",
                    1,
                );
                assert_ne!(
                    wit,
                    before,
                    "{} has an unknown response record",
                    file.display()
                );
            }
            std::fs::write(&destination, wit).unwrap();
        }
        println!("cargo:rerun-if-changed={}", file.display());
    }
    println!("cargo:rerun-if-env-changed=NONS_ROOT");
}
