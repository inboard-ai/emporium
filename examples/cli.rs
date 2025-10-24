use sipper::{Sipper, StreamExt};
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), emporium::Error> {
    let mut extensions = HashMap::new();
    let path = format!("{}/marketplace/build", std::env::var("CARGO_MANIFEST_DIR").unwrap());

    eprintln!("Scanning extensions directory: {path}");

    let mut list = emporium::list(&path).pin();

    // Collect extensions as they stream in
    while let Some((path, manifest)) = list.next().await {
        eprintln!("Found extension: {}", manifest.id);
        extensions.insert(manifest.id.clone(), (path, manifest));
    }

    // Await the sipper to handle any errors
    // list.await?;

    eprintln!("\nScanned {} extensions", extensions.len());
    eprintln!("Extensions: {:#?}", extensions);

    Ok(())
}
