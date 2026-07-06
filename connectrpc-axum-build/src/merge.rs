use std::fs;
use std::io::{Result, Write};
use std::path::Path;

pub(crate) fn append_generated_section(
    target_file: &Path,
    banner: &str,
    generated: &str,
) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(target_file)?;
    write!(file, "\n{banner}\n{generated}")?;
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
