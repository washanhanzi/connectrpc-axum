use std::fs;
use std::io::Result;
use std::path::Path;

pub(crate) fn append_generated_section(
    target_file: &Path,
    banner: &str,
    generated: &str,
) -> Result<()> {
    let mut content = fs::read_to_string(target_file)?;
    content.push('\n');
    content.push_str(banner);
    content.push('\n');
    content.push_str(generated);
    fs::write(target_file, content)?;
    Ok(())
}

pub(crate) fn append_generated_file(
    target_file: &Path,
    banner: &str,
    source_file: &Path,
) -> Result<()> {
    let generated = fs::read_to_string(source_file)?;
    append_generated_section(target_file, banner, &generated)
}
