use std::env;
use std::path::PathBuf;

fn main() {
    // Probe libpappl and libcups for include paths and link flags.
    //
    // Floor at PAPPL 1.0 so the failure is a clear pkg-config error rather
    // than a cryptic "undefined symbol" later. Anything ≥1.0 builds; the
    // 1.4+ paths (e.g. papplSystemCreatePrinters auto-add) are guarded by
    // wrappers in `lib.rs`, so older PAPPL still works with a degraded
    // discovery path.
    let pappl = pkg_config::Config::new()
        .atleast_version("1.0")
        .probe("pappl")
        .expect("pkg-config: pappl not found (need ≥1.0)");
    let cups = pkg_config::probe_library("cups").expect("pkg-config: cups not found");

    let mut builder = bindgen::Builder::default()
        .header("wrapper.h")
        // Types
        .allowlist_type("pappl_.*")
        .allowlist_type("cups_option_t")
        .allowlist_type("cups_page_header2_t")
        .allowlist_type("ipp_t")
        .allowlist_type("ipp_orient_t")
        .allowlist_type("ipp_quality_t")
        // Functions
        .allowlist_function("pappl.*")
        // Vars / constants
        .allowlist_var("PAPPL_.*")
        .allowlist_var("IPP_ORIENT_.*")
        .allowlist_var("IPP_QUALITY_.*")
        // Keep PAPPL internals opaque
        .opaque_type("_pappl_.*_s")
        // Large structs need Default
        .derive_default(true)
        // Don't emit doc comments from C headers
        .generate_comments(false);

    // Add include paths from pkg-config
    for path in pappl.include_paths.iter().chain(cups.include_paths.iter()) {
        builder = builder.clang_arg(format!("-I{}", path.display()));
    }

    let bindings = builder.generate().expect("bindgen failed");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("failed to write bindings");

    // Emit cfg flags based on PAPPL version for optional API usage.
    // papplSystemCreatePrinters was added in PAPPL 1.4.
    //
    // Note: `cargo:rustc-cfg` applies only to *this* crate. Dependent
    // crates can't read it; they must call the wrapper exposed in
    // `lib.rs` (e.g. `try_system_create_printers`).
    println!("cargo:rustc-check-cfg=cfg(pappl_1_4)");
    {
        let parts: Vec<u32> = pappl
            .version
            .split('.')
            .filter_map(|s: &str| s.parse().ok())
            .collect();
        let (major, minor) = (
            parts.first().copied().unwrap_or(0),
            parts.get(1).copied().unwrap_or(0),
        );
        if major > 1 || (major == 1 && minor >= 4) {
            println!("cargo:rustc-cfg=pappl_1_4");
        }
    }
}
