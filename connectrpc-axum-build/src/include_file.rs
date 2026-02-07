use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;
use std::io::Result;
use std::path::{Path, PathBuf};

/// A node in the module tree. Each node represents a Rust module that may:
/// - include a generated `.rs` file
/// - re-export an extern crate via `pub use`
/// - contain child modules
struct ModuleNode {
    /// If set, this module should `include!()` the given file stem (e.g. `"buf.validate"`).
    include_file: Option<String>,
    /// If set, this module should `pub use <path>::*;` (for extern_path shims like pbjson_types).
    reexport: Option<String>,
    /// Child modules keyed by segment name, sorted alphabetically via BTreeMap.
    children: BTreeMap<String, ModuleNode>,
}

struct RenderOptions {
    include_from_out_dir_env: bool,
    absolute_out_dir: Option<PathBuf>,
}

impl RenderOptions {
    fn include_expr(&self, file_stem: &str) -> String {
        if self.include_from_out_dir_env {
            return format!("concat!(env!(\"OUT_DIR\"), \"/{file_stem}.rs\")");
        }

        let out_dir = self
            .absolute_out_dir
            .as_ref()
            .expect("absolute_out_dir must be set when include_from_out_dir_env is false");
        let full_path = out_dir.join(format!("{file_stem}.rs"));
        format!("{:?}", full_path.to_string_lossy())
    }
}

impl ModuleNode {
    fn new() -> Self {
        Self {
            include_file: None,
            reexport: None,
            children: BTreeMap::new(),
        }
    }

    /// Insert a dotted package name (e.g. `"buf.validate"`) into the tree,
    /// marking the leaf with the file to include.
    fn insert_include(&mut self, segments: &[&str], file_stem: &str) {
        if segments.is_empty() {
            self.include_file = Some(file_stem.to_string());
            return;
        }
        let child = self
            .children
            .entry(segments[0].to_string())
            .or_insert_with(ModuleNode::new);
        child.insert_include(&segments[1..], file_stem);
    }

    /// Insert a dotted package name with a re-export path (e.g. `"::pbjson_types"`).
    fn insert_reexport(&mut self, segments: &[&str], reexport_path: &str) {
        if segments.is_empty() {
            self.reexport = Some(reexport_path.to_string());
            return;
        }
        let child = self
            .children
            .entry(segments[0].to_string())
            .or_insert_with(ModuleNode::new);
        child.insert_reexport(&segments[1..], reexport_path);
    }

    /// Render the tree as Rust source code.
    fn render(&self, out: &mut String, depth: usize, options: &RenderOptions) {
        let indent = "    ".repeat(depth);
        for (name, child) in &self.children {
            writeln!(out, "{indent}pub mod {name} {{").unwrap();
            if let Some(ref reexport) = child.reexport {
                writeln!(out, "{indent}    pub use {reexport}::*;").unwrap();
            }
            if let Some(ref file_stem) = child.include_file {
                writeln!(
                    out,
                    "{indent}    include!({});",
                    options.include_expr(file_stem)
                )
                .unwrap();
            }
            child.render(out, depth + 1, options);
            writeln!(out, "{indent}}}").unwrap();
        }
    }
}

/// Scan `out_dir` for generated `.rs` files and write a single include file
/// that provides a properly nested `pub mod` tree.
///
/// `extern_reexports` maps dotted proto package names to Rust paths for
/// packages handled via `extern_path` (e.g. `"google.protobuf"` -> `"::pbjson_types"`).
/// These produce `pub use <path>::*;` instead of `include!()`.
///
/// When `include_from_out_dir_env` is true, nested include paths are emitted as
/// `concat!(env!("OUT_DIR"), "...")`. Otherwise, absolute include paths are emitted.
pub(crate) fn generate(
    include_file_name: &str,
    out_dir: &str,
    extern_reexports: &[(String, String)],
    include_from_out_dir_env: bool,
) -> Result<()> {
    let out_path = Path::new(out_dir);
    let absolute_out_dir = if include_from_out_dir_env {
        None
    } else {
        Some(std::fs::canonicalize(out_path)?)
    };
    let render_options = RenderOptions {
        include_from_out_dir_env,
        absolute_out_dir,
    };

    let mut root = ModuleNode::new();

    // Scan for generated .rs files (skip .serde.rs, the include file itself, etc.)
    for entry in std::fs::read_dir(out_path)? {
        let entry = entry?;
        let path = entry.path();

        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Skip non-.rs files
        if !file_name.ends_with(".rs") {
            continue;
        }

        // Skip the include file itself
        if file_name == include_file_name {
            continue;
        }

        // Skip .serde.rs files (already appended to main files)
        if file_name.ends_with(".serde.rs") {
            continue;
        }

        // Skip empty default file (prost generates _.rs for packageless protos)
        if file_name == "_.rs" {
            continue;
        }

        let file_stem = file_name.strip_suffix(".rs").unwrap();

        // Split dotted package name into segments
        let segments: Vec<&str> = file_stem.split('.').collect();
        root.insert_include(&segments, file_stem);
    }

    // Insert extern_path re-exports (e.g. google.protobuf -> ::pbjson_types)
    for (proto_path, rust_path) in extern_reexports {
        let segments: Vec<&str> = proto_path.split('.').collect();
        root.insert_reexport(&segments, rust_path);
    }

    // Render the tree
    let mut output = String::from("// @generated by connectrpc-axum-build\n");
    root.render(&mut output, 0, &render_options);

    // Write the include file
    let include_path = out_path.join(include_file_name);
    std::fs::write(include_path, output)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_dir(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            fs::write(dir.path().join(f), "// generated").unwrap();
        }
        dir
    }

    #[test]
    fn single_level_packages() {
        let dir = setup_dir(&["hello.rs", "echo.rs"]);
        generate("protos.rs", dir.path().to_str().unwrap(), &[], true).unwrap();
        let content = fs::read_to_string(dir.path().join("protos.rs")).unwrap();
        assert!(content.contains("pub mod echo {"));
        assert!(content.contains("pub mod hello {"));
        assert!(content.contains(r#"include!(concat!(env!("OUT_DIR"), "/echo.rs"));"#));
        assert!(content.contains(r#"include!(concat!(env!("OUT_DIR"), "/hello.rs"));"#));
    }

    #[test]
    fn multi_level_package() {
        let dir = setup_dir(&["buf.validate.rs"]);
        generate("protos.rs", dir.path().to_str().unwrap(), &[], true).unwrap();
        let content = fs::read_to_string(dir.path().join("protos.rs")).unwrap();
        assert!(content.contains("pub mod buf {"));
        assert!(content.contains("pub mod validate {"));
        assert!(content.contains(r#"include!(concat!(env!("OUT_DIR"), "/buf.validate.rs"));"#));
    }

    #[test]
    fn shared_prefix_overlap() {
        // foo.rs (package "foo") + foo.bar.rs (package "foo.bar")
        // The "foo" module should both include foo.rs AND contain child "bar"
        let dir = setup_dir(&["foo.rs", "foo.bar.rs"]);
        generate("protos.rs", dir.path().to_str().unwrap(), &[], true).unwrap();
        let content = fs::read_to_string(dir.path().join("protos.rs")).unwrap();
        assert!(content.contains("pub mod foo {"));
        assert!(content.contains(r#"include!(concat!(env!("OUT_DIR"), "/foo.rs"));"#));
        assert!(content.contains("pub mod bar {"));
        assert!(content.contains(r#"include!(concat!(env!("OUT_DIR"), "/foo.bar.rs"));"#));
    }

    #[test]
    fn extern_reexport_google_protobuf() {
        let dir = setup_dir(&["cerberus.v1.rs"]);
        let reexports = vec![("google.protobuf".to_string(), "::pbjson_types".to_string())];
        generate("protos.rs", dir.path().to_str().unwrap(), &reexports, true).unwrap();
        let content = fs::read_to_string(dir.path().join("protos.rs")).unwrap();
        assert!(content.contains("pub mod google {"));
        assert!(content.contains("pub mod protobuf {"));
        assert!(content.contains("pub use ::pbjson_types::*;"));
        assert!(content.contains("pub mod cerberus {"));
    }

    #[test]
    fn skips_serde_and_include_file() {
        let dir = setup_dir(&["hello.rs", "hello.serde.rs", "protos.rs"]);
        generate("protos.rs", dir.path().to_str().unwrap(), &[], true).unwrap();
        let content = fs::read_to_string(dir.path().join("protos.rs")).unwrap();
        // Should only have hello, not serde or self-referential protos
        assert!(content.contains("pub mod hello {"));
        assert!(!content.contains("serde"));
        // Should not try to include protos.rs itself
        assert!(!content.contains(r#""/protos.rs""#));
    }

    #[test]
    fn skips_underscore_file() {
        let dir = setup_dir(&["_.rs", "hello.rs"]);
        generate("protos.rs", dir.path().to_str().unwrap(), &[], true).unwrap();
        let content = fs::read_to_string(dir.path().join("protos.rs")).unwrap();
        assert!(content.contains("pub mod hello {"));
        assert!(!content.contains("pub mod _ {"));
    }

    #[test]
    fn deterministic_order() {
        let dir = setup_dir(&["zeta.rs", "alpha.rs", "middle.rs"]);
        generate("protos.rs", dir.path().to_str().unwrap(), &[], true).unwrap();
        let content = fs::read_to_string(dir.path().join("protos.rs")).unwrap();
        let alpha_pos = content.find("pub mod alpha").unwrap();
        let middle_pos = content.find("pub mod middle").unwrap();
        let zeta_pos = content.find("pub mod zeta").unwrap();
        assert!(alpha_pos < middle_pos);
        assert!(middle_pos < zeta_pos);
    }

    #[test]
    fn custom_out_dir_uses_absolute_include_paths() {
        let dir = setup_dir(&["hello.rs"]);
        generate("protos.rs", dir.path().to_str().unwrap(), &[], false).unwrap();
        let content = fs::read_to_string(dir.path().join("protos.rs")).unwrap();
        let hello_path = fs::canonicalize(dir.path().join("hello.rs")).unwrap();
        let expected = format!("include!({:?});", hello_path.to_string_lossy());
        assert!(content.contains(&expected));
        assert!(!content.contains("env!(\"OUT_DIR\")"));
    }
}
