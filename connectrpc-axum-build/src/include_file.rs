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

/// Write a single include file that provides a properly nested `pub mod` tree
/// for the packages compiled in this run.
///
/// `file_stems` holds the generated file stems (e.g. `"buf.validate"`, `"_"`
/// for packageless protos) derived from this run's descriptor set. Stems
/// whose `.rs` file does not exist in `out_dir` are skipped (e.g. packages
/// handled via `extern_path`, or imported packages prost did not generate).
/// Stale files from other builder runs sharing `out_dir` are never picked up.
///
/// The packageless `_` stem is included at the include-file root rather than
/// inside a module, mirroring how prost places its contents in the crate-level
/// namespace.
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
    file_stems: &std::collections::BTreeSet<String>,
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

    for file_stem in file_stems {
        let file_name = format!("{file_stem}.rs");

        // Never include the include file itself
        if file_name == include_file_name {
            continue;
        }

        // Skip packages without a generated file (e.g. extern_path'd packages)
        if !out_path.join(&file_name).exists() {
            continue;
        }

        if file_stem == "_" {
            // Packageless protos live at the include-file root
            root.insert_include(&[], file_stem);
        } else {
            // Split dotted package name into segments
            let segments: Vec<&str> = file_stem.split('.').collect();
            root.insert_include(&segments, file_stem);
        }
    }

    // Insert extern_path re-exports (e.g. google.protobuf -> ::pbjson_types)
    for (proto_path, rust_path) in extern_reexports {
        let segments: Vec<&str> = proto_path.split('.').collect();
        root.insert_reexport(&segments, rust_path);
    }

    // Render the tree
    let mut output = String::from("// @generated by connectrpc-axum-build\n");
    if let Some(ref file_stem) = root.include_file {
        writeln!(
            output,
            "include!({});",
            render_options.include_expr(file_stem)
        )
        .unwrap();
    }
    root.render(&mut output, 0, &render_options);

    // Write the include file
    let include_path = out_path.join(include_file_name);
    std::fs::write(include_path, output)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::fs;

    fn setup_dir(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            fs::write(dir.path().join(f), "// generated").unwrap();
        }
        dir
    }

    fn stems(stems: &[&str]) -> BTreeSet<String> {
        stems.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn single_level_packages() {
        let dir = setup_dir(&["hello.rs", "echo.rs"]);
        generate(
            "protos.rs",
            dir.path().to_str().unwrap(),
            &[],
            true,
            &stems(&["hello", "echo"]),
        )
        .unwrap();
        let content = fs::read_to_string(dir.path().join("protos.rs")).unwrap();
        assert!(content.contains("pub mod echo {"));
        assert!(content.contains("pub mod hello {"));
        assert!(content.contains(r#"include!(concat!(env!("OUT_DIR"), "/echo.rs"));"#));
        assert!(content.contains(r#"include!(concat!(env!("OUT_DIR"), "/hello.rs"));"#));
    }

    #[test]
    fn multi_level_package() {
        let dir = setup_dir(&["buf.validate.rs"]);
        generate(
            "protos.rs",
            dir.path().to_str().unwrap(),
            &[],
            true,
            &stems(&["buf.validate"]),
        )
        .unwrap();
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
        generate(
            "protos.rs",
            dir.path().to_str().unwrap(),
            &[],
            true,
            &stems(&["foo", "foo.bar"]),
        )
        .unwrap();
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
        generate(
            "protos.rs",
            dir.path().to_str().unwrap(),
            &reexports,
            true,
            &stems(&["cerberus.v1", "google.protobuf"]),
        )
        .unwrap();
        let content = fs::read_to_string(dir.path().join("protos.rs")).unwrap();
        assert!(content.contains("pub mod google {"));
        assert!(content.contains("pub mod protobuf {"));
        assert!(content.contains("pub use ::pbjson_types::*;"));
        assert!(content.contains("pub mod cerberus {"));
        // extern_path'd package has no generated file to include
        assert!(!content.contains(r#""/google.protobuf.rs""#));
    }

    #[test]
    fn skips_include_file_itself() {
        let dir = setup_dir(&["hello.rs", "protos.rs"]);
        generate(
            "protos.rs",
            dir.path().to_str().unwrap(),
            &[],
            true,
            &stems(&["hello", "protos"]),
        )
        .unwrap();
        let content = fs::read_to_string(dir.path().join("protos.rs")).unwrap();
        assert!(content.contains("pub mod hello {"));
        // Should not try to include protos.rs itself
        assert!(!content.contains(r#""/protos.rs""#));
    }

    #[test]
    fn skips_files_not_generated_by_this_run() {
        // Simulate a second builder run sharing OUT_DIR: stale files (and a
        // previous include file) exist but are not part of this run's packages.
        let dir = setup_dir(&["hello.rs", "stale.rs", "protos1.rs", "hello.serde.rs"]);
        fs::write(dir.path().join("protos1.rs"), "pub mod stale {}").unwrap();
        generate(
            "protos2.rs",
            dir.path().to_str().unwrap(),
            &[],
            true,
            &stems(&["hello"]),
        )
        .unwrap();
        let content = fs::read_to_string(dir.path().join("protos2.rs")).unwrap();
        assert!(content.contains("pub mod hello {"));
        assert!(!content.contains("stale"));
        assert!(!content.contains("protos1"));
        assert!(!content.contains("serde"));
    }

    #[test]
    fn packageless_file_included_at_root() {
        let dir = setup_dir(&["_.rs", "hello.rs"]);
        generate(
            "protos.rs",
            dir.path().to_str().unwrap(),
            &[],
            true,
            &stems(&["_", "hello"]),
        )
        .unwrap();
        let content = fs::read_to_string(dir.path().join("protos.rs")).unwrap();
        assert!(content.contains("pub mod hello {"));
        assert!(!content.contains("pub mod _ {"));
        // Packageless contents land at the include-file root
        assert!(content.contains(r#"include!(concat!(env!("OUT_DIR"), "/_.rs"));"#));
    }

    #[test]
    fn missing_package_files_are_skipped() {
        let dir = setup_dir(&["hello.rs"]);
        generate(
            "protos.rs",
            dir.path().to_str().unwrap(),
            &[],
            true,
            &stems(&["hello", "google.protobuf"]),
        )
        .unwrap();
        let content = fs::read_to_string(dir.path().join("protos.rs")).unwrap();
        assert!(content.contains("pub mod hello {"));
        assert!(!content.contains("google"));
    }

    #[test]
    fn deterministic_order() {
        let dir = setup_dir(&["zeta.rs", "alpha.rs", "middle.rs"]);
        generate(
            "protos.rs",
            dir.path().to_str().unwrap(),
            &[],
            true,
            &stems(&["zeta", "alpha", "middle"]),
        )
        .unwrap();
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
        generate(
            "protos.rs",
            dir.path().to_str().unwrap(),
            &[],
            false,
            &stems(&["hello"]),
        )
        .unwrap();
        let content = fs::read_to_string(dir.path().join("protos.rs")).unwrap();
        let hello_path = fs::canonicalize(dir.path().join("hello.rs")).unwrap();
        let expected = format!("include!({:?});", hello_path.to_string_lossy());
        assert!(content.contains(&expected));
        assert!(!content.contains("env!(\"OUT_DIR\")"));
    }
}
