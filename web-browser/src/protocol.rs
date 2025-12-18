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

/// Embedded Drive app HTML - connects to file-backend at localhost:3000
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
        .status { padding: 8px 16px; border-radius: 20px; font-size: 0.9em; }
        .status.online { background: rgba(76, 175, 80, 0.3); color: #81c784; }
        .status.offline { background: rgba(244, 67, 54, 0.3); color: #e57373; }
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
            margin-bottom: 20px;
        }
        .upload-zone:hover { background: rgba(79, 195, 247, 0.1); }
        .upload-zone.dragover { background: rgba(79, 195, 247, 0.2); border-color: #81d4fa; }
        .file-list { background: rgba(0,0,0,0.2); border-radius: 12px; padding: 20px; }
        .file-item {
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 12px;
            background: rgba(255,255,255,0.05);
            border-radius: 8px;
            margin-bottom: 8px;
        }
        .file-item:hover { background: rgba(255,255,255,0.1); }
        .file-name { display: flex; align-items: center; gap: 10px; }
        .file-actions button {
            padding: 8px 16px;
            border: none;
            border-radius: 6px;
            cursor: pointer;
            margin-left: 8px;
        }
        .btn-download { background: #4fc3f7; color: #000; }
        .btn-thumb { background: #9c27b0; color: #fff; }
        .progress { display: none; margin-top: 10px; }
        .progress-bar { height: 4px; background: #4fc3f7; border-radius: 2px; transition: width 0.3s; }
        #output { margin-top: 20px; padding: 10px; background: rgba(0,0,0,0.3); border-radius: 8px; font-family: monospace; font-size: 0.9em; max-height: 150px; overflow-y: auto; }
        input[type="file"] { display: none; }
    </style>
</head>
<body>
    <div class="header">
        <h1>📁 fOS Drive</h1>
        <div>
            <span class="status offline" id="serverStatus">● Offline</span>
            <a href="fos://home" style="color: #4fc3f7; text-decoration: none; margin-left: 20px;">← Home</a>
        </div>
    </div>
    
    <div class="stats">
        <div class="stat-card">
            <div class="stat-value" id="memUsage">--</div>
            <div>Memory</div>
        </div>
        <div class="stat-card">
            <div class="stat-value" id="cpuLoad">--</div>
            <div>CPU Load</div>
        </div>
        <div class="stat-card">
            <div class="stat-value" id="fileCount">0</div>
            <div>Files</div>
        </div>
    </div>
    
    <div class="upload-zone" id="uploadZone" onclick="document.getElementById('fileInput').click()">
        <h2>📤 Drop files here or click to upload</h2>
        <p>Files are uploaded to the fOS file-backend</p>
        <input type="file" id="fileInput" multiple />
        <div class="progress" id="progress">
            <div class="progress-bar" id="progressBar" style="width: 0%"></div>
        </div>
    </div>
    
    <div class="file-list">
        <h3 style="margin-bottom: 15px;">📂 Files</h3>
        <div id="files">Loading...</div>
    </div>
    
    <pre id="output"></pre>
    
    <script>
        const API_BASE = 'http://localhost:3000';
        let authToken = null;
        
        // Initialize
        async function init() {
            await login();
            await loadFiles();
            await loadStats();
            setInterval(loadStats, 5000);
        }
        
        // Login to get token
        async function login() {
            try {
                const res = await fetch(`${API_BASE}/auth/login`, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ username: 'fos-user', password: 'fos-pass' })
                });
                if (res.ok) {
                    const data = await res.json();
                    authToken = data.token;
                    document.getElementById('serverStatus').textContent = '● Online';
                    document.getElementById('serverStatus').className = 'status online';
                    log('Connected to file-backend');
                }
            } catch (e) {
                log('File-backend offline. Start with: cd file-backend && cargo run --release');
            }
        }
        
        // Load file list
        async function loadFiles() {
            // Use bridge for local file list
            window.ipc.postMessage('drive:list');
        }
        
        // Load system stats via bridge
        async function loadStats() {
            window.ipc.postMessage('sys:getMemoryUsage');
            window.ipc.postMessage('sys:getCpuUsage');
        }
        
        // Upload file
        async function uploadFile(file) {
            if (!authToken) { log('Not connected'); return; }
            
            const formData = new FormData();
            formData.append('file', file);
            
            document.getElementById('progress').style.display = 'block';
            document.getElementById('progressBar').style.width = '50%';
            
            try {
                const res = await fetch(`${API_BASE}/upload`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${authToken}` },
                    body: formData
                });
                
                document.getElementById('progressBar').style.width = '100%';
                
                if (res.ok) {
                    const data = await res.json();
                    log(`Uploaded: ${data.id} (${data.size} bytes)`);
                    addFileToList(data.id, data.size);
                } else {
                    log('Upload failed: ' + res.status);
                }
            } catch (e) {
                log('Upload error: ' + e.message);
            }
            
            setTimeout(() => {
                document.getElementById('progress').style.display = 'none';
                document.getElementById('progressBar').style.width = '0%';
            }, 1000);
        }
        
        // Add file to UI list
        function addFileToList(id, size) {
            const filesDiv = document.getElementById('files');
            if (filesDiv.textContent === 'Loading...' || filesDiv.textContent === 'No files yet') {
                filesDiv.innerHTML = '';
            }
            
            const item = document.createElement('div');
            item.className = 'file-item';
            item.innerHTML = `
                <div class="file-name">📄 ${id}</div>
                <div class="file-actions">
                    <button class="btn-thumb" onclick="viewThumb('${id}')">🖼️</button>
                    <button class="btn-download" onclick="downloadFile('${id}')">⬇️</button>
                </div>
            `;
            filesDiv.appendChild(item);
            
            const count = filesDiv.querySelectorAll('.file-item').length;
            document.getElementById('fileCount').textContent = count;
        }
        
        // Download file
        function downloadFile(id) {
            if (!authToken) return;
            window.open(`${API_BASE}/stream/${id}`, '_blank');
        }
        
        // View thumbnail
        async function viewThumb(id) {
            if (!authToken) return;
            window.open(`${API_BASE}/thumbnail/${id}`, '_blank');
        }
        
        function log(msg) {
            const output = document.getElementById('output');
            output.textContent = `[${new Date().toLocaleTimeString()}] ${msg}\n` + output.textContent;
        }
        
        // Drag and drop
        const zone = document.getElementById('uploadZone');
        zone.addEventListener('dragover', (e) => { e.preventDefault(); zone.classList.add('dragover'); });
        zone.addEventListener('dragleave', () => zone.classList.remove('dragover'));
        zone.addEventListener('drop', (e) => {
            e.preventDefault();
            zone.classList.remove('dragover');
            [...e.dataTransfer.files].forEach(uploadFile);
        });
        
        // File input
        document.getElementById('fileInput').addEventListener('change', (e) => {
            [...e.target.files].forEach(uploadFile);
        });
        
        // Bridge response handler
        window.addEventListener('message', (e) => {
            if (e.data && e.data.type === 'bridgeResponse') {
                if (e.data.memoryUsage) document.getElementById('memUsage').textContent = e.data.memoryUsage;
                if (e.data.cpuLoad) document.getElementById('cpuLoad').textContent = e.data.cpuLoad;
                if (e.data.files) {
                    const filesDiv = document.getElementById('files');
                    if (e.data.files.length === 0) {
                        filesDiv.textContent = 'No files yet';
                    } else {
                        filesDiv.innerHTML = '';
                        e.data.files.forEach(f => addFileToList(f, 0));
                    }
                }
            }
        });
        
        init();
    </script>
</body>
</html>"#;

/// Embedded Chat app HTML - connects to chat-server WebSocket at localhost:9000
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
            align-items: center;
        }
        h1 { color: #e94560; }
        .status { padding: 8px 16px; border-radius: 20px; font-size: 0.9em; }
        .status.online { background: rgba(76, 175, 80, 0.3); color: #81c784; }
        .status.offline { background: rgba(244, 67, 54, 0.3); color: #e57373; }
        .status.connecting { background: rgba(255, 193, 7, 0.3); color: #ffd54f; }
        .room-info {
            padding: 10px 20px;
            background: rgba(0,0,0,0.2);
            display: flex;
            gap: 20px;
            align-items: center;
        }
        .room-info input {
            padding: 8px 12px;
            border: none;
            border-radius: 6px;
            background: rgba(255,255,255,0.1);
            color: #eee;
        }
        .room-info button {
            padding: 8px 16px;
            border: none;
            border-radius: 6px;
            background: #e94560;
            color: white;
            cursor: pointer;
        }
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
        .message .sender { font-size: 0.8em; color: #4fc3f7; margin-bottom: 4px; }
        .message .time { font-size: 0.7em; color: #888; margin-top: 4px; }
        .message.sent { margin-left: auto; background: #e94560; }
        .message.system { background: rgba(233, 69, 96, 0.2); max-width: 100%; text-align: center; font-style: italic; }
        .input-area {
            padding: 20px;
            background: rgba(0,0,0,0.3);
            display: flex;
            gap: 10px;
        }
        input#msgInput {
            flex: 1;
            padding: 12px;
            border: none;
            border-radius: 8px;
            background: rgba(255,255,255,0.1);
            color: #eee;
            font-size: 16px;
        }
        button.send-btn {
            padding: 12px 24px;
            border: none;
            border-radius: 8px;
            background: #e94560;
            color: white;
            cursor: pointer;
            font-size: 16px;
        }
        button:hover { background: #ff6b6b; }
        button:disabled { background: #555; cursor: not-allowed; }
        .user-count { background: rgba(79, 195, 247, 0.2); padding: 4px 12px; border-radius: 12px; }
    </style>
</head>
<body>
    <div class="header">
        <h1>💬 fOS Chat</h1>
        <div>
            <span class="status offline" id="wsStatus">● Disconnected</span>
            <a href="fos://home" style="color: #e94560; text-decoration: none; margin-left: 20px;">← Home</a>
        </div>
    </div>
    
    <div class="room-info">
        <span>Room:</span>
        <input type="text" id="roomInput" value="general" placeholder="Room name" />
        <span>Username:</span>
        <input type="text" id="usernameInput" placeholder="Your name" />
        <button onclick="joinRoom()" id="joinBtn">Join</button>
        <span class="user-count" id="userCount">0 users</span>
    </div>
    
    <div class="messages" id="messages">
        <div class="message system">Welcome to fOS Chat! Enter your username and join a room.</div>
    </div>
    
    <div class="input-area">
        <input type="text" id="msgInput" placeholder="Type a message..." disabled />
        <button class="send-btn" onclick="sendMessage()" id="sendBtn" disabled>Send</button>
    </div>
    
    <script>
        const WS_URL = 'ws://localhost:9000';
        let ws = null;
        let currentRoom = null;
        let username = null;
        
        function setStatus(status, text) {
            const el = document.getElementById('wsStatus');
            el.textContent = text;
            el.className = 'status ' + status;
        }
        
        function connect() {
            setStatus('connecting', '● Connecting...');
            
            try {
                ws = new WebSocket(WS_URL);
                ws.binaryType = 'arraybuffer';
                
                ws.onopen = () => {
                    setStatus('online', '● Connected');
                    addSystemMessage('Connected to chat server');
                };
                
                ws.onclose = () => {
                    setStatus('offline', '● Disconnected');
                    addSystemMessage('Disconnected from server');
                    document.getElementById('msgInput').disabled = true;
                    document.getElementById('sendBtn').disabled = true;
                    // Reconnect after 3 seconds
                    setTimeout(connect, 3000);
                };
                
                ws.onerror = () => {
                    setStatus('offline', '● Error');
                    addSystemMessage('Connection error. Is chat-server running? Start with: cd chat-server && cargo run --release');
                };
                
                ws.onmessage = (event) => {
                    // For now, handle as text (protobuf would need WASM decoder)
                    // This is a simplified version - real impl would decode protobuf
                    try {
                        const data = JSON.parse(event.data);
                        handleServerMessage(data);
                    } catch (e) {
                        // Binary protobuf message - would need WASM decoder
                        addSystemMessage('Received binary message (protobuf)');
                    }
                };
            } catch (e) {
                setStatus('offline', '● Failed');
                addSystemMessage('Failed to connect: ' + e.message);
            }
        }
        
        function handleServerMessage(msg) {
            switch(msg.type) {
                case 'chat':
                    addMessage(msg.sender, msg.content, msg.timestamp, msg.sender === username);
                    break;
                case 'userJoined':
                    addSystemMessage(`${msg.username} joined the room`);
                    break;
                case 'userLeft':
                    addSystemMessage(`${msg.username} left the room`);
                    break;
                case 'roomInfo':
                    document.getElementById('userCount').textContent = `${msg.userCount} users`;
                    break;
            }
        }
        
        function joinRoom() {
            const room = document.getElementById('roomInput').value.trim() || 'general';
            const user = document.getElementById('usernameInput').value.trim();
            
            if (!user) {
                addSystemMessage('Please enter a username');
                return;
            }
            
            username = user;
            currentRoom = room;
            
            // Send join via bridge (which would forward to WebSocket)
            window.ipc.postMessage(`chat:join:${room}:${username}`);
            
            addSystemMessage(`Joined room: ${room} as ${username}`);
            document.getElementById('msgInput').disabled = false;
            document.getElementById('sendBtn').disabled = false;
            document.getElementById('msgInput').focus();
        }
        
        function sendMessage() {
            const input = document.getElementById('msgInput');
            const msg = input.value.trim();
            if (!msg || !currentRoom) return;
            
            // Add to UI immediately
            addMessage(username, msg, Date.now(), true);
            
            // Send via bridge
            window.ipc.postMessage('chat:send:' + msg);
            
            input.value = '';
        }
        
        function addMessage(sender, content, timestamp, isSent) {
            const div = document.createElement('div');
            div.className = 'message' + (isSent ? ' sent' : '');
            div.innerHTML = `
                <div class="sender">${sender}</div>
                <div class="content">${escapeHtml(content)}</div>
                <div class="time">${new Date(timestamp).toLocaleTimeString()}</div>
            `;
            document.getElementById('messages').appendChild(div);
            div.scrollIntoView({ behavior: 'smooth' });
        }
        
        function addSystemMessage(text) {
            const div = document.createElement('div');
            div.className = 'message system';
            div.textContent = text;
            document.getElementById('messages').appendChild(div);
            div.scrollIntoView({ behavior: 'smooth' });
        }
        
        function escapeHtml(text) {
            const div = document.createElement('div');
            div.textContent = text;
            return div.innerHTML;
        }
        
        document.getElementById('msgInput').addEventListener('keypress', (e) => {
            if (e.key === 'Enter') sendMessage();
        });
        
        document.getElementById('usernameInput').addEventListener('keypress', (e) => {
            if (e.key === 'Enter') joinRoom();
        });
        
        // Try to connect on load
        connect();
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
