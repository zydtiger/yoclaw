use std::sync::LazyLock;

// TODO: change default config path
pub static CONFIG_DIR: LazyLock<String> =
    LazyLock::new(|| std::env::var("CONFIG_PATH").unwrap_or_else(|_| ".".to_string()));
