use std::env;
use std::path::PathBuf;

fn main() {
    // Probe libpappl and libcups for include paths and link flags
    let pappl = pkg_config::probe_library("pappl").expect("pkg-config: pappl not found");
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
}
