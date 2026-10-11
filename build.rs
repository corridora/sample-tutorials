use std::{fs, path::Path};
fn walk(p: &Path, base: &Path, out: &mut String) {
    if let Ok(entries) = fs::read_dir(p) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, base, out);
            } else {
                let rel = p.strip_prefix(base).unwrap().to_str().unwrap();
                let key = if rel == "index.html" {
                    "/".to_owned()
                } else {
                    format!("/{rel}")
                };
                let ct = if rel.ends_with(".js") {
                    "application/javascript"
                } else if rel.ends_with(".css") {
                    "text/css"
                } else {
                    "text/html"
                };
                out.push_str(&format!(
                    "{key:?} => Some((include_bytes!({:?}).to_vec(),{ct:?}.into())),\n",
                    fs::canonicalize(&p).unwrap()
                ));
            }
        }
    }
}
fn main() {
    println!("cargo:rerun-if-changed=web/dist");
    println!("cargo:rerun-if-changed=src");
    fn source_hash(p: &Path, h: &mut u64) {
        if let Ok(entries) = fs::read_dir(p) {
            let mut files: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            files.sort();
            for file in files {
                if file.is_dir() {
                    source_hash(&file, h);
                } else if let Ok(bytes) = fs::read(file) {
                    for b in bytes {
                        *h = (*h ^ b as u64).wrapping_mul(1099511628211);
                    }
                }
            }
        }
    }
    let mut hash = 14695981039346656037u64;
    source_hash(Path::new("src"), &mut hash);
    let version =
        std::env::var("SAMPLE_BUILD_VERSION").unwrap_or_else(|_| format!("source-{hash:016x}"));
    println!("cargo:rustc-env=SAMPLE_BUILD_VERSION={version}");
    println!("cargo:rerun-if-env-changed=SAMPLE_BUILD_VERSION");
    let mut s = "pub fn get(p:&str)->Option<(Vec<u8>,String)>{match p {".to_owned();
    walk(Path::new("web/dist"), Path::new("web/dist"), &mut s);
    s.push_str("_=>None}}\n");
    fs::write(
        Path::new(&std::env::var("OUT_DIR").unwrap()).join("assets.rs"),
        s,
    )
    .unwrap();
}
