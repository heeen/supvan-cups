/// Return true if SUPVAN_MOCK=1.
pub fn is_mock_mode() -> bool {
    std::env::var("SUPVAN_MOCK")
        .map(|v| v == "1")
        .unwrap_or(false)
}
