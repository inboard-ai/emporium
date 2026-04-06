use crate::validate::{validate_manifest, validate_wasm};
use anyhow::{Context, Result};
use chrono::Utc;
use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use tar::Builder;

const PACKAGER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Create an extension package (.empkg)
pub fn create_package(extension_path: &Path, output_path: &Path) -> Result<()> {
    println!("Packaging extension from: {}", extension_path.display());

    // Canonicalize paths
    let extension_path = extension_path
        .canonicalize()
        .with_context(|| format!("Extension path not found: {}", extension_path.display()))?;

    // Validate manifest
    let manifest = validate_manifest(&extension_path)?;
    println!(
        "  Manifest validated: {} v{}",
        manifest.extension.name, manifest.extension.version
    );

    // Locate and validate WASM file
    let wasm_path = extension_path.join(&manifest.component.entry);
    validate_wasm(&wasm_path)?;
    println!("  WASM validated: {}", manifest.component.entry);

    // Report world
    let world = manifest.component.world.as_deref().unwrap_or("tool-extension");
    println!("  World: {}", world);

    // Compute WASM checksum
    let wasm_bytes =
        fs::read(&wasm_path).with_context(|| format!("Failed to read WASM file: {}", wasm_path.display()))?;
    let checksum = compute_sha256(&wasm_bytes);
    println!("  Checksum: {}", checksum);

    // Create enhanced manifest with [package] section
    let enhanced_manifest = create_enhanced_manifest(&extension_path, &checksum)?;

    // Build package
    let package_name = format!("{}-{}", manifest.extension.id, manifest.extension.version);
    let output_file = output_path.join(format!("{}.empkg", package_name));

    create_tar_gz(
        &output_file,
        &package_name,
        &enhanced_manifest,
        &wasm_bytes,
        &extension_path,
    )?;

    println!("Package created: {}", output_file.display());

    Ok(())
}

/// Compute SHA-256 checksum of data
fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Create enhanced manifest with [package] section
fn create_enhanced_manifest(extension_path: &Path, checksum: &str) -> Result<String> {
    let manifest_path = extension_path.join("manifest.toml");
    let original = fs::read_to_string(&manifest_path)?;

    // Parse as a TOML table so we can add to it
    let mut doc: toml::Table = toml::from_str(&original)?;

    // Resolve the world: use declared value or default to "tool-extension"
    let world = doc
        .get("component")
        .and_then(|c| c.as_table())
        .and_then(|t| t.get("world"))
        .and_then(|v| v.as_str())
        .unwrap_or("tool-extension");

    // Create package section
    let mut package = toml::Table::new();
    package.insert("created_at".to_string(), toml::Value::String(Utc::now().to_rfc3339()));
    package.insert("checksum_sha256".to_string(), toml::Value::String(checksum.to_string()));
    package.insert(
        "packager_version".to_string(),
        toml::Value::String(PACKAGER_VERSION.to_string()),
    );
    package.insert("world".to_string(), toml::Value::String(world.to_string()));

    doc.insert("package".to_string(), toml::Value::Table(package));

    Ok(toml::to_string_pretty(&doc)?)
}

/// Create the tar.gz package
fn create_tar_gz(
    output_file: &Path,
    package_name: &str,
    manifest: &str,
    wasm_bytes: &[u8],
    extension_path: &Path,
) -> Result<()> {
    let file = File::create(output_file)
        .with_context(|| format!("Failed to create output file: {}", output_file.display()))?;

    let encoder = GzEncoder::new(file, Compression::default());
    let mut archive = Builder::new(encoder);

    // Add manifest.toml
    add_bytes_to_archive(
        &mut archive,
        &format!("{}/manifest.toml", package_name),
        manifest.as_bytes(),
    )?;

    // Add extension.wasm
    add_bytes_to_archive(&mut archive, &format!("{}/extension.wasm", package_name), wasm_bytes)?;

    // Add required icon file (icon.svg or icon.png)
    let icon_svg_path = extension_path.join("icon.svg");
    let icon_png_path = extension_path.join("icon.png");
    if icon_svg_path.exists() {
        let icon = fs::read(&icon_svg_path)?;
        add_bytes_to_archive(&mut archive, &format!("{}/icon.svg", package_name), &icon)?;
        println!("  Added: icon.svg");
    } else if icon_png_path.exists() {
        let icon = fs::read(&icon_png_path)?;
        add_bytes_to_archive(&mut archive, &format!("{}/icon.png", package_name), &icon)?;
        println!("  Added: icon.png");
    } else {
        anyhow::bail!(
            "Extension must include an icon file (icon.svg or icon.png) at {}",
            extension_path.display()
        );
    }

    // Add required square icon file (icon-square.svg or icon-square.png)
    let sq_svg_path = extension_path.join("icon-square.svg");
    let sq_png_path = extension_path.join("icon-square.png");
    if sq_svg_path.exists() {
        let icon = fs::read(&sq_svg_path)?;
        add_bytes_to_archive(&mut archive, &format!("{}/icon-square.svg", package_name), &icon)?;
        println!("  Added: icon-square.svg");
    } else if sq_png_path.exists() {
        let icon = fs::read(&sq_png_path)?;
        add_bytes_to_archive(&mut archive, &format!("{}/icon-square.png", package_name), &icon)?;
        println!("  Added: icon-square.png");
    } else {
        anyhow::bail!(
            "Extension must include a square icon file (icon-square.svg or icon-square.png) at {}",
            extension_path.display()
        );
    }

    // Add optional README.md
    let readme_path = extension_path.join("README.md");
    if readme_path.exists() {
        let readme = fs::read(&readme_path)?;
        add_bytes_to_archive(&mut archive, &format!("{}/README.md", package_name), &readme)?;
        println!("  Added: README.md");
    }

    // Add optional LICENSE
    let license_path = extension_path.join("LICENSE");
    if license_path.exists() {
        let license = fs::read(&license_path)?;
        add_bytes_to_archive(&mut archive, &format!("{}/LICENSE", package_name), &license)?;
        println!("  Added: LICENSE");
    }

    // Add optional assets/ directory
    let assets_path = extension_path.join("assets");
    if assets_path.is_dir() {
        add_directory_to_archive(&mut archive, &assets_path, package_name, "assets")?;
        println!("  Added: assets/");
    }

    archive.finish()?;

    Ok(())
}

/// Add bytes as a file to the archive
fn add_bytes_to_archive<W: Write>(archive: &mut Builder<W>, path: &str, data: &[u8]) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    );
    header.set_cksum();

    archive.append_data(&mut header, path, data)?;

    Ok(())
}

/// Recursively add a directory to the archive
fn add_directory_to_archive<W: Write>(
    archive: &mut Builder<W>,
    dir_path: &Path,
    package_name: &str,
    relative_base: &str,
) -> Result<()> {
    for entry in fs::read_dir(dir_path)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();
        let archive_path = format!("{}/{}/{}", package_name, relative_base, file_name_str);

        if path.is_dir() {
            let new_relative = format!("{}/{}", relative_base, file_name_str);
            add_directory_to_archive(archive, &path, package_name, &new_relative)?;
        } else {
            let data = fs::read(&path)?;
            add_bytes_to_archive(archive, &archive_path, &data)?;
        }
    }

    Ok(())
}
