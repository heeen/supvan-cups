//! Re-runs the PAPPL pkg-config probe so this crate can `#[cfg(pappl_1_4)]`
//! against the actually linked PAPPL version.
//!
//! Background: `cargo:rustc-cfg` only applies to the crate that emits it,
//! not its dependents. `pappl-sys` already probes and emits `pappl_1_4`,
//! but `pappl-rs` can't see that cfg from there. So we duplicate the
//! probe here. pkg-config is cheap and the result is identical.

fn main() {
    println!("cargo:rustc-check-cfg=cfg(pappl_1_4)");

    let pappl = match pkg_config::Config::new()
        .atleast_version("1.0")
        .cargo_metadata(false) // pappl-sys already emits link flags
        .probe("pappl")
    {
        Ok(p) => p,
        Err(e) => {
            // Don't fail the build here — pappl-sys will have already
            // failed with a clearer error. If it somehow didn't, fall
            // back to "no 1.4 features" rather than a build error.
            eprintln!("pappl-rs/build.rs: pkg-config probe failed ({e}); assuming PAPPL < 1.4");
            return;
        }
    };

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
