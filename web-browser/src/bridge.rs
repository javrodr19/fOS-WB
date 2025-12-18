//! Bridge Module
//!
//! Bidirectional communication between JavaScript and Rust.
//! Provides native system functions to the frontend with minimal overhead.

use std::time::{SystemTime, UNIX_EPOCH};

/// Bridge command from JavaScript
#[derive(Debug, Clone)]
pub enum BridgeCommand {
    /// System: Get memory usage
    SysGetMemoryUsage,
    /// System: Get CPU usage
    SysGetCpuUsage,
    /// System: Get system info
    SysGetInfo,
    /// Drive: Upload file (base64 data)
    DriveUpload { filename: String, data: String },
    /// Drive: List files
    DriveList,
    /// Chat: Send message
    ChatSend { message: String },
    /// Unknown command
    Unknown(String),
}

/// Bridge response to JavaScript
#[derive(Debug, Clone)]
pub struct BridgeResponse {
    pub success: bool,
    pub data: String,
}

impl BridgeCommand {
    /// Parse a command string from JavaScript
    /// Format: "namespace:command:args"
    pub fn parse(input: &str) -> Self {
        let parts: Vec<&str> = input.splitn(3, ':').collect();
        
        match parts.as_slice() {
            ["sys", "getMemoryUsage"] => Self::SysGetMemoryUsage,
            ["sys", "getCpuUsage"] => Self::SysGetCpuUsage,
            ["sys", "getInfo"] => Self::SysGetInfo,
            ["drive", "list"] => Self::DriveList,
            ["drive", "upload", rest] => {
                // Parse filename and data
                if let Some((filename, data)) = rest.split_once(':') {
                    Self::DriveUpload {
                        filename: filename.to_string(),
                        data: data.to_string(),
                    }
                } else {
                    Self::Unknown(input.to_string())
                }
            }
            ["chat", "send", message] => Self::ChatSend {
                message: message.to_string(),
            },
            _ => Self::Unknown(input.to_string()),
        }
    }
}

/// Execute a bridge command and return JSON response
pub fn execute_command(cmd: BridgeCommand) -> String {
    match cmd {
        BridgeCommand::SysGetMemoryUsage => get_memory_usage(),
        BridgeCommand::SysGetCpuUsage => get_cpu_usage(),
        BridgeCommand::SysGetInfo => get_system_info(),
        BridgeCommand::DriveList => list_files(),
        BridgeCommand::DriveUpload { filename, data } => upload_file(&filename, &data),
        BridgeCommand::ChatSend { message } => send_chat_message(&message),
        BridgeCommand::Unknown(cmd) => {
            format!(r#"{{"type":"bridgeResponse","success":false,"error":"Unknown command: {}"}}"#, cmd)
        }
    }
}

/// Get memory usage statistics
fn get_memory_usage() -> String {
    #[cfg(target_os = "linux")]
    {
        // Read from /proc/meminfo
        if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
            let mut total: u64 = 0;
            let mut available: u64 = 0;
            
            for line in meminfo.lines() {
                if line.starts_with("MemTotal:") {
                    total = parse_meminfo_value(line);
                } else if line.starts_with("MemAvailable:") {
                    available = parse_meminfo_value(line);
                }
            }
            
            let used = total.saturating_sub(available);
            let percent = if total > 0 { (used * 100) / total } else { 0 };
            
            return format!(
                r#"{{"type":"bridgeResponse","success":true,"memoryUsage":"{}%","totalMB":{},"usedMB":{},"availableMB":{}}}"#,
                percent,
                total / 1024,
                used / 1024,
                available / 1024
            );
        }
    }
    
    // Fallback for other platforms
    format!(r#"{{"type":"bridgeResponse","success":true,"memoryUsage":"N/A"}}"#)
}

/// Parse a value from /proc/meminfo (format: "MemTotal:     12345 kB")
#[cfg(target_os = "linux")]
fn parse_meminfo_value(line: &str) -> u64 {
    line.split_whitespace()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Get CPU usage (simplified - returns load average on Linux)
fn get_cpu_usage() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(loadavg) = std::fs::read_to_string("/proc/loadavg") {
            let load1 = loadavg.split_whitespace().next().unwrap_or("0");
            return format!(
                r#"{{"type":"bridgeResponse","success":true,"cpuLoad":"{}"}}"#,
                load1
            );
        }
    }
    
    format!(r#"{{"type":"bridgeResponse","success":true,"cpuUsage":"N/A"}}"#)
}

/// Get system information
fn get_system_info() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    format!(
        r#"{{"type":"bridgeResponse","success":true,"platform":"{}","arch":"{}","timestamp":{}}}"#,
        std::env::consts::OS,
        std::env::consts::ARCH,
        timestamp
    )
}

/// List files in the upload directory
fn list_files() -> String {
    let upload_dir = std::path::Path::new("./uploads");
    
    if !upload_dir.exists() {
        return format!(r#"{{"type":"bridgeResponse","success":true,"files":[]}}"#);
    }
    
    let files: Vec<String> = std::fs::read_dir(upload_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .map(|name| format!(r#""{}""#, name))
                .collect()
        })
        .unwrap_or_default();
    
    format!(
        r#"{{"type":"bridgeResponse","success":true,"files":[{}]}}"#,
        files.join(",")
    )
}

/// Upload a file (placeholder - would integrate with file-backend)
fn upload_file(filename: &str, _data: &str) -> String {
    // In production, this would send to the file-backend service
    format!(
        r#"{{"type":"bridgeResponse","success":true,"message":"Upload received: {}"}}"#,
        filename
    )
}

/// Send a chat message (placeholder - would integrate with chat-server)
fn send_chat_message(message: &str) -> String {
    // In production, this would send to the chat WebSocket
    format!(
        r#"{{"type":"bridgeResponse","success":true,"message":"Sent: {}"}}"#,
        message.chars().take(50).collect::<String>()
    )
}
