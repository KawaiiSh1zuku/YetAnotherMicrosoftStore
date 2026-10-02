fn main() {
    if std::env::var_os("CARGO_FEATURE_BROKER_DEPENDENCY").is_none() {
        tauri_build::build()
    }
}
