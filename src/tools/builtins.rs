use chrono::Local;

/// Returns the current date and time as a formatted string.
pub fn get_current_time() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
