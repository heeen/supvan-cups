//! `SUPVAN_*` settings from the environment.

/// Read `name` and parse it. A set but unparseable value warns and yields
/// `None`, like an unset one, so the caller falls back to its default rather
/// than failing.
pub fn parsed<T>(name: &str, expected: &str, parse: impl FnOnce(&str) -> Option<T>) -> Option<T> {
    let raw = std::env::var(name).ok()?;
    let value = parse(raw.trim());
    if value.is_none() {
        log::warn!("{name}: expected {expected}, got {raw:?}; using the default");
    }
    value
}
