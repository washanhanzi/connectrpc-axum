use std::fs;
use std::io::Result;
use std::path::Path;

/// Append a generated section to `target_file`, creating it if prost emitted
/// no file for the package (service-only packages).
///
/// Files prost doesn't own persist across build-script reruns in `OUT_DIR`,
/// so a previously appended section with the same banner is stripped first —
/// otherwise every rerun would duplicate the generated items and break the
/// downstream build.
pub(crate) fn append_generated_section(
    target_file: &Path,
    banner: &str,
    generated: &str,
) -> Result<()> {
    let existing = match fs::read_to_string(target_file) {
        Ok(content) => strip_generated_section(&content, banner),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };

    let mut content = existing.trim_end().to_string();
    content.push_str(&format!("\n{banner}\n{generated}"));
    fs::write(target_file, content)
}

pub(crate) fn append_generated_file(
    target_file: &Path,
    banner: &str,
    source_file: &Path,
) -> Result<()> {
    let generated = fs::read_to_string(source_file)?;
    append_generated_section(target_file, banner, &generated)
}

/// Remove the section previously appended under `banner`: from the banner
/// line up to the next section banner or end of file. All generator banners
/// share the `// --- ... ---` shape, which delimits sections.
fn strip_generated_section(content: &str, banner: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut skipping = false;
    for line in content.lines() {
        if line.starts_with("// --- ") && line.ends_with(" ---") {
            skipping = line == banner;
        }
        if !skipping {
            result.push_str(line);
            result.push('\n');
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const BANNER_A: &str = "// --- Connect service/client code ---";
    const BANNER_B: &str = "// --- Tonic gRPC server stubs (extern_path reused messages) ---";

    #[test]
    fn append_is_idempotent_across_reruns() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("svc.rs");

        // Two simulated build-script runs appending the same sections.
        for _ in 0..2 {
            append_generated_section(&file, BANNER_A, "pub struct Builder;\n").unwrap();
            append_generated_section(&file, BANNER_B, "pub struct Server;\n").unwrap();
        }

        let content = fs::read_to_string(&file).unwrap();
        assert_eq!(content.matches("pub struct Builder;").count(), 1);
        assert_eq!(content.matches("pub struct Server;").count(), 1);
        assert_eq!(content.matches(BANNER_A).count(), 1);
        assert_eq!(content.matches(BANNER_B).count(), 1);
    }

    #[test]
    fn rerun_replaces_stale_section_content() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("svc.rs");

        append_generated_section(&file, BANNER_A, "pub struct Old;\n").unwrap();
        append_generated_section(&file, BANNER_A, "pub struct New;\n").unwrap();

        let content = fs::read_to_string(&file).unwrap();
        assert!(!content.contains("pub struct Old;"));
        assert!(content.contains("pub struct New;"));
    }

    #[test]
    fn preserves_prost_emitted_content() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("pkg.rs");
        fs::write(&file, "pub struct Message {}\n").unwrap();

        append_generated_section(&file, BANNER_A, "pub struct Builder;\n").unwrap();
        append_generated_section(&file, BANNER_A, "pub struct Builder;\n").unwrap();

        let content = fs::read_to_string(&file).unwrap();
        assert!(content.starts_with("pub struct Message {}"));
        assert_eq!(content.matches("pub struct Builder;").count(), 1);
    }
}
