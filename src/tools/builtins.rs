use chrono::Local;
use serde_json::Value;

/// Returns the current date and time as a formatted string.
pub fn get_current_time(_args: Value) -> String {
    Local::now().format("%Y-%m-%d %A %H:%M:%S").to_string()
}
