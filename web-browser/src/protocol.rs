//! Custom Protocol Handler
//!
//! Handles fos:// URI scheme for instant loading of internal apps.
//! Bypasses DNS/HTTP stack for internal communication.

use std::borrow::Cow;
use std::path::PathBuf;

/// Protocol response
pub struct ProtocolResponse {
    pub mime_type: String,
    pub data: Vec<u8>,
}

/// Asset directories for internal apps
pub struct AssetPaths {
    pub drive_assets: PathBuf,
    pub chat_assets: PathBuf,
}

impl Default for AssetPaths {
    fn default() -> Self {
        Self {
            drive_assets: PathBuf::from("./assets/drive"),
            chat_assets: PathBuf::from("./assets/chat"),
        }
    }
}

/// Handle fos:// protocol requests
/// 
/// Routes:
/// - fos://drive/* → Drive app assets
/// - fos://chat/* → Chat app assets
/// - fos://api/* → Internal API (handled by bridge)
pub fn handle_protocol_request(uri: &str) -> Option<ProtocolResponse> {
    // Parse the URI (format: fos://app/path)
    let uri = uri.strip_prefix("fos://")?;
    let (app, path) = uri.split_once('/').unwrap_or((uri, "index.html"));
    let path = if path.is_empty() { "index.html" } else { path };
    
    match app {
        "drive" => handle_drive_request(path),
        "chat" => handle_chat_request(path),
        "api" => None, // API requests handled by bridge
        "home" => Some(home_page()),
        _ => Some(not_found_page(app)),
    }
}

/// Handle Drive app asset requests
fn handle_drive_request(path: &str) -> Option<ProtocolResponse> {
    // In production, load from embedded assets or filesystem
    // For now, return a placeholder page
    let mime = guess_mime(path);
    
    if path == "index.html" || path.is_empty() {
        Some(ProtocolResponse {
            mime_type: "text/html".to_string(),
            data: DRIVE_INDEX_HTML.as_bytes().to_vec(),
        })
    } else {
        // Try to load asset from filesystem
        let asset_path = PathBuf::from("./assets/drive").join(path);
        std::fs::read(&asset_path).ok().map(|data| ProtocolResponse {
            mime_type: mime,
            data,
        })
    }
}

/// Handle Chat app asset requests
fn handle_chat_request(path: &str) -> Option<ProtocolResponse> {
    let mime = guess_mime(path);
    
    if path == "index.html" || path.is_empty() {
        Some(ProtocolResponse {
            mime_type: "text/html".to_string(),
            data: CHAT_INDEX_HTML.as_bytes().to_vec(),
        })
    } else {
        let asset_path = PathBuf::from("./assets/chat").join(path);
        std::fs::read(&asset_path).ok().map(|data| ProtocolResponse {
            mime_type: mime,
            data,
        })
    }
}

/// Home page with app launcher
fn home_page() -> ProtocolResponse {
    ProtocolResponse {
        mime_type: "text/html".to_string(),
        data: HOME_HTML.as_bytes().to_vec(),
    }
}

/// 404 page
fn not_found_page(app: &str) -> ProtocolResponse {
    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head><title>Not Found</title></head>
<body style="font-family: system-ui; padding: 40px; background: #1a1a2e; color: #eee;">
    <h1>404 - App Not Found</h1>
    <p>The app <code>{}</code> is not registered.</p>
    <a href="fos://home" style="color: #4fc3f7;">Go Home</a>
</body>
</html>"#,
        app
    );
    ProtocolResponse {
        mime_type: "text/html".to_string(),
        data: html.into_bytes(),
    }
}

/// Guess MIME type from file extension
fn guess_mime(path: &str) -> String {
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "json" => "application/json",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Embedded Drive app HTML
const DRIVE_INDEX_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>fOS Drive</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: 'Segoe UI', system-ui, sans-serif;
            background: linear-gradient(135deg, #1a1a2e 0%, #16213e 100%);
            color: #eee;
            min-height: 100vh;
            padding: 20px;
        }
        .header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 20px;
            background: rgba(255,255,255,0.05);
            border-radius: 12px;
            margin-bottom: 20px;
        }
        h1 { color: #4fc3f7; }
        .stats {
            display: grid;
            grid-template-columns: repeat(3, 1fr);
            gap: 20px;
            margin-bottom: 20px;
        }
        .stat-card {
            background: rgba(255,255,255,0.08);
            padding: 20px;
            border-radius: 12px;
            text-align: center;
        }
        .stat-value { font-size: 2em; color: #4fc3f7; }
        .upload-zone {
            border: 2px dashed #4fc3f7;
            border-radius: 12px;
            padding: 40px;
            text-align: center;
            cursor: pointer;
            transition: all 0.3s;
        }
        .upload-zone:hover { background: rgba(79, 195, 247, 0.1); }
        #output { margin-top: 20px; padding: 10px; background: rgba(0,0,0,0.3); border-radius: 8px; }
    </style>
</head>
<body>
    <div class="header">
        <h1>📁 fOS Drive</h1>
        <a href="fos://home" style="color: #4fc3f7; text-decoration: none;">← Home</a>
    </div>
    
    <div class="stats">
        <div class="stat-card">
            <div class="stat-value" id="memUsage">--</div>
            <div>Memory Usage</div>
        </div>
        <div class="stat-card">
            <div class="stat-value" id="cpuUsage">--</div>
            <div>CPU Usage</div>
        </div>
        <div class="stat-card">
            <div class="stat-value" id="fileCount">0</div>
            <div>Files</div>
        </div>
    </div>
    
    <div class="upload-zone" onclick="testBridge()">
        <h2>📤 Click to Test Bridge</h2>
        <p>Tests fos.sys.getMemoryUsage()</p>
    </div>
    
    <pre id="output"></pre>
    
    <script>
        async function testBridge() {
            document.getElementById('output').textContent = 'Calling bridge...';
            
            // Call Rust bridge via IPC
            window.ipc.postMessage('sys:getMemoryUsage');
        }
        
        // Listen for bridge responses
        window.addEventListener('message', (e) => {
            if (e.data && e.data.type === 'bridgeResponse') {
                document.getElementById('output').textContent = JSON.stringify(e.data, null, 2);
                if (e.data.memoryUsage) {
                    document.getElementById('memUsage').textContent = e.data.memoryUsage;
                }
            }
        });
    </script>
</body>
</html>"#;

/// Embedded Chat app HTML
const CHAT_INDEX_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>fOS Chat</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: 'Segoe UI', system-ui, sans-serif;
            background: linear-gradient(135deg, #1a1a2e 0%, #0f3460 100%);
            color: #eee;
            min-height: 100vh;
            display: flex;
            flex-direction: column;
        }
        .header {
            padding: 20px;
            background: rgba(0,0,0,0.3);
            display: flex;
            justify-content: space-between;
        }
        h1 { color: #e94560; }
        .messages {
            flex: 1;
            padding: 20px;
            overflow-y: auto;
        }
        .message {
            background: rgba(255,255,255,0.1);
            padding: 12px 16px;
            border-radius: 12px;
            margin-bottom: 10px;
            max-width: 70%;
        }
        .message.sent { margin-left: auto; background: #e94560; }
        .input-area {
            padding: 20px;
            background: rgba(0,0,0,0.3);
            display: flex;
            gap: 10px;
        }
        input {
            flex: 1;
            padding: 12px;
            border: none;
            border-radius: 8px;
            background: rgba(255,255,255,0.1);
            color: #eee;
            font-size: 16px;
        }
        button {
            padding: 12px 24px;
            border: none;
            border-radius: 8px;
            background: #e94560;
            color: white;
            cursor: pointer;
            font-size: 16px;
        }
        button:hover { background: #ff6b6b; }
    </style>
</head>
<body>
    <div class="header">
        <h1>💬 fOS Chat</h1>
        <a href="fos://home" style="color: #e94560; text-decoration: none;">← Home</a>
    </div>
    
    <div class="messages" id="messages">
        <div class="message">Welcome to fOS Chat!</div>
        <div class="message">Connect to ws://localhost:9000 for real-time messaging.</div>
    </div>
    
    <div class="input-area">
        <input type="text" id="msgInput" placeholder="Type a message..." />
        <button onclick="sendMessage()">Send</button>
    </div>
    
    <script>
        function sendMessage() {
            const input = document.getElementById('msgInput');
            const msg = input.value.trim();
            if (!msg) return;
            
            const div = document.createElement('div');
            div.className = 'message sent';
            div.textContent = msg;
            document.getElementById('messages').appendChild(div);
            input.value = '';
            
            // Send via bridge
            window.ipc.postMessage('chat:send:' + msg);
        }
        
        document.getElementById('msgInput').addEventListener('keypress', (e) => {
            if (e.key === 'Enter') sendMessage();
        });
    </script>
</body>
</html>"#;

/// Home page HTML
const HOME_HTML: &str = r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>fOS Home</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body {
            font-family: 'Segoe UI', system-ui, sans-serif;
            background: linear-gradient(135deg, #0f0c29 0%, #302b63 50%, #24243e 100%);
            color: #eee;
            min-height: 100vh;
            display: flex;
            flex-direction: column;
            align-items: center;
            justify-content: center;
            padding: 40px;
        }
        h1 {
            font-size: 3em;
            margin-bottom: 10px;
            background: linear-gradient(90deg, #4fc3f7, #e94560);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
        }
        .subtitle { color: #888; margin-bottom: 40px; }
        .apps {
            display: grid;
            grid-template-columns: repeat(2, 1fr);
            gap: 20px;
            max-width: 600px;
        }
        .app-card {
            background: rgba(255,255,255,0.08);
            padding: 40px;
            border-radius: 16px;
            text-align: center;
            text-decoration: none;
            color: #eee;
            transition: all 0.3s;
            border: 1px solid transparent;
        }
        .app-card:hover {
            transform: translateY(-5px);
            border-color: #4fc3f7;
            background: rgba(255,255,255,0.12);
        }
        .app-icon { font-size: 3em; margin-bottom: 10px; }
        .app-name { font-size: 1.2em; font-weight: 600; }
        .app-desc { color: #888; font-size: 0.9em; margin-top: 5px; }
    </style>
</head>
<body>
    <h1>⚡ fOS</h1>
    <p class="subtitle">Your unified workspace</p>
    
    <div class="apps">
        <a href="fos://drive" class="app-card">
            <div class="app-icon">📁</div>
            <div class="app-name">Drive</div>
            <div class="app-desc">File management</div>
        </a>
        <a href="fos://chat" class="app-card">
            <div class="app-icon">💬</div>
            <div class="app-name">Chat</div>
            <div class="app-desc">Real-time messaging</div>
        </a>
    </div>
</body>
</html>"#;
