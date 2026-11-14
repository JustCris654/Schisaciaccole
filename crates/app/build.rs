#[path = "src/theme_catalog.rs"]
mod theme_catalog;

/// Validates startup configuration and compiles the Slint interface.
///
/// # Errors
/// Fails for a non-Unicode, empty or unknown theme ID, or invalid Slint source.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-env-changed=SCHISACIACCOLE_DEFAULT_THEME");
    let value = match std::env::var("SCHISACIACCOLE_DEFAULT_THEME") {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    let theme = theme_catalog::validate_default(value.as_deref())?;
    println!(
        "cargo:rustc-env=SCHISACIACCOLE_COMPILED_THEME={}",
        theme.id()
    );
    let config = slint_build::CompilerConfiguration::new().with_style("cupertino".into());
    slint_build::compile_with_config("ui/app-window.slint", config)?;
    Ok(())
}
