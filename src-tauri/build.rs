//! Build script.
//!
//! Besides the usual Tauri codegen, this packs the portable payload when the
//! `portable` feature is on: the harness runtime and a Node interpreter, in a
//! single compressed blob the binary embeds and unpacks on first run.

fn main() {
    #[cfg(feature = "portable")]
    portable::pack();

    tauri_build::build()
}

#[cfg(feature = "portable")]
mod portable {
    use std::{
        env,
        fs::{self, File},
        io::{self, BufReader, Write},
        path::{Path, PathBuf},
    };

    use flate2::{write::DeflateEncoder, Compression};

    /// Everything that goes into the payload, as `(source, path inside the
    /// unpacked runtime)`.
    fn sources(root: &Path) -> Vec<(PathBuf, PathBuf)> {
        let mut entries = Vec::new();

        // The Node interpreter, fetched by CI before the build.
        let node = root.join("vendor").join(node_file_name());
        if node.is_file() {
            entries.push((node, PathBuf::from(node_file_name())));
        } else {
            println!(
                "cargo:warning=vendor/{} is missing; the portable build will fall back to the \
                 system Node",
                node_file_name()
            );
        }

        // The harness runtime, installed by `npm run backend:install`.
        let modules = root
            .join("..")
            .join("backend")
            .join("node_modules")
            .canonicalize()
            .unwrap_or_else(|_| {
                panic!(
                    "the portable build needs backend/node_modules; run `npm run backend:install`"
                )
            });
        collect(&modules, Path::new("backend/node_modules"), &mut entries);

        entries
    }

    const fn node_file_name() -> &'static str {
        if cfg!(windows) {
            "node.exe"
        } else {
            "node"
        }
    }

    /// Recursively list files under `dir`, recording each one's path relative
    /// to `prefix` in the unpacked layout.
    fn collect(dir: &Path, prefix: &Path, out: &mut Vec<(PathBuf, PathBuf)>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        for entry in read.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let relative = prefix.join(entry.file_name());
            // Symlinks are skipped: npm creates them for `.bin` shims, and
            // following them would duplicate megabytes into the payload.
            if kind.is_dir() {
                collect(&path, &relative, out);
            } else if kind.is_file() {
                out.push((path, relative));
            }
        }
    }

    /// Write one record: path length, path, data length, data.
    fn write_entry<W: Write>(sink: &mut W, source: &Path, name: &Path) -> io::Result<u64> {
        let text = name.to_string_lossy().replace('\\', "/");
        let bytes = text.as_bytes();
        let size = source.metadata()?.len();

        sink.write_all(&(bytes.len() as u32).to_le_bytes())?;
        sink.write_all(bytes)?;
        sink.write_all(&size.to_le_bytes())?;

        let mut file = BufReader::new(File::open(source)?);
        let copied = io::copy(&mut file, sink)?;
        assert_eq!(copied, size, "{} changed size while being packed", text);
        Ok(size)
    }

    pub fn pack() {
        let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
        let out = PathBuf::from(env::var("OUT_DIR").expect("out dir")).join("portable-payload.bin");

        println!("cargo:rerun-if-changed=vendor");
        println!("cargo:rerun-if-changed=../backend/package.json");

        let entries = sources(&root);
        let mut sink = DeflateEncoder::new(
            File::create(&out).expect("create payload"),
            // Level 6: level 9 costs minutes here for a couple of per cent.
            Compression::new(6),
        );

        let mut total = 0u64;
        for (source, name) in &entries {
            total += write_entry(&mut sink, source, name).unwrap_or_else(|e| {
                panic!("packing {} failed: {e}", source.display());
            });
        }
        sink.finish().expect("finish payload");

        let packed = fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
        println!(
            "cargo:warning=portable payload: {} files, {:.0} MB packed from {:.0} MB",
            entries.len(),
            packed as f64 / 1e6,
            total as f64 / 1e6
        );
    }
}
