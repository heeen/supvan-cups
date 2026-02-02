fn main() {
    // Link to CUPS libraries for raster reading
    if let Ok(lib) = pkg_config::probe_library("cups") {
        for path in &lib.link_paths {
            println!("cargo:rustc-link-search=native={}", path.display());
        }
    } else {
        // Fallback: assume standard library paths
        println!("cargo:rustc-link-search=native=/usr/lib");
        println!("cargo:rustc-link-search=native=/usr/lib/x86_64-linux-gnu");
    }
    println!("cargo:rustc-link-lib=cupsimage");
    println!("cargo:rustc-link-lib=cups");
}
