use crate::store;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};
use tauri::Emitter;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect as ws_connect, Message};
use url::Url;

const CDP_HOST: &str = "127.0.0.1";
const CDP_PORT: u16 = 9222;

static MSG_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    MSG_ID.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordMessage {
    pub author: String,
    pub content: String,
    #[serde(default)]
    pub encrypted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchResult {
    pub messages: Vec<DiscordMessage>,
    pub page_title: String,
    pub channel_url: String,
    #[serde(default)]
    pub dm_usernames: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeAnalysis {
    #[serde(default)]
    pub channel_id: Option<String>,
    #[serde(default)]
    pub our_username: Option<String>,
    #[serde(default)]
    pub our_key_present: bool,
    #[serde(default)]
    pub their_keys: Vec<TheirKey>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TheirKey {
    pub username: String,
    pub key_hex: String,
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HandshakeResult {
    pub status: String,
    pub message: String,
    pub contact_added: Option<String>,
    pub key_sent: bool,
}

type CdpWs = tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>;

struct CdpCommand {
    method: String,
    params: Value,
    response_tx: mpsc::Sender<Result<Value, String>>,
}

struct CdpSession {
    cmd_tx: mpsc::Sender<CdpCommand>,
}

static CDP_SESSION: Mutex<Option<CdpSession>> = Mutex::new(None);

fn send_cdp_command(method: &str, params: Value) -> Result<Value, String> {
    let (resp_tx, resp_rx) = mpsc::channel();
    {
        let session = CDP_SESSION.lock().map_err(|e| format!("Lock error: {}", e))?;
        let session = session.as_ref().ok_or("Not connected. Click Connect first.")?;
        session
            .cmd_tx
            .send(CdpCommand {
                method: method.into(),
                params,
                response_tx: resp_tx,
            })
            .map_err(|_| "Connection lost. Try reconnecting.".to_string())?;
    }
    resp_rx
        .recv_timeout(Duration::from_secs(15))
        .map_err(|_| "CDP response timeout".to_string())?
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CdpTarget {
    #[serde(default)]
    title: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    web_socket_debugger_url: String,
}

fn discover_targets() -> Result<Vec<CdpTarget>, String> {
    let url = format!("http://{}:{}/json", CDP_HOST, CDP_PORT);
    let resp = ureq::get(&url)
        .call()
        .map_err(|e| format!("Cannot connect to CDP at {}:{} — {}\n\nMake sure Discord is running with --remote-debugging-port=9222", CDP_HOST, CDP_PORT, e))?;
    let targets: Vec<CdpTarget> = resp
        .into_json()
        .map_err(|e| format!("Failed to parse CDP targets: {}", e))?;
    Ok(targets)
}

fn find_discord_target(targets: &[CdpTarget]) -> Result<&CdpTarget, String> {
    for t in targets {
        if t.r#type == "page" && (t.url.contains("discord.com") || t.title.contains("Discord")) {
            return Ok(t);
        }
    }
    for t in targets {
        if t.r#type == "page" {
            return Ok(t);
        }
    }
    Err("No Discord target found. Make sure Discord is open and showing a channel.".into())
}

const JS_SETUP: &str = r#"
(() => {
    window._discryptGetDmUsernames = function() {
        try {
            const chatEl = document.querySelector('[class*="chat_"], [class*="chat-"]');
            if (!chatEl) return [];
            const fiberKey = Object.keys(chatEl).find(
                k => k.startsWith('__reactFiber$') || k.startsWith('__reactInternalInstance$')
            );
            if (!fiberKey) return [];
            let fiber = chatEl[fiberKey];
            let attempts = 0;
            while (fiber && attempts < 80) {
                const props = fiber.memoizedProps || fiber.pendingProps;
                if (props && props.channel) {
                    const ch = props.channel;
                    const recipients = ch.rawRecipients || ch.recipients || [];
                    if (recipients.length > 0 && recipients[0].username) {
                        return recipients.map(r => r.username);
                    }
                }
                fiber = fiber.return;
                attempts++;
            }
        } catch(_) {}
        return [];
    };

    window._discryptExtract = function() {
        let reactMessages = [];
        try {
            const scroller = document.querySelector('[class*="scrollerInner"]');
            if (scroller) {
                const fiberKey = Object.keys(scroller).find(
                    k => k.startsWith('__reactFiber$') || k.startsWith('__reactInternalInstance$')
                );
                if (fiberKey) {
                    let fiber = scroller[fiberKey];
                    let attempts = 0;
                    while (fiber && attempts < 50) {
                        if (fiber.memoizedProps && fiber.memoizedProps.messages) {
                            const msgs = fiber.memoizedProps.messages;
                            if (Array.isArray(msgs)) {
                                msgs.forEach(m => {
                                    reactMessages.push({
                                        author: m.author
                                            ? (m.author.username || m.author.globalName || "?")
                                            : "?",
                                        content: m.content || ""
                                    });
                                });
                            }
                            break;
                        }
                        fiber = fiber.return;
                        attempts++;
                    }
                }
            }
        } catch (_) {}

        let results = [];
        if (reactMessages.length === 0) {
            const scroller = document.querySelector('[class*="scrollerInner"]');
            if (scroller) {
                const messageEls = scroller.querySelectorAll(
                    '[class*="messageContent"], [class*="markup_"], [class*="markup-"]'
                );
                const usernameEls = scroller.querySelectorAll(
                    '[class*="username_"], [class*="username-"]'
                );
                const usernames = [];
                usernameEls.forEach(el => {
                    usernames.push({
                        text: el.textContent.trim(),
                        top: el.getBoundingClientRect().top
                    });
                });
                messageEls.forEach(el => {
                    if (el.closest('[role="textbox"]')) return;
                    const content = el.textContent.trim();
                    if (!content) return;
                    const msgTop = el.getBoundingClientRect().top;
                    let author = "Unknown";
                    let minDist = Infinity;
                    for (const u of usernames) {
                        const dist = msgTop - u.top;
                        if (dist >= 0 && dist < minDist) {
                            minDist = dist;
                            author = u.text;
                        }
                    }
                    results.push({ author, content });
                });
            }
        }

        const messages = reactMessages.length > 0 ? reactMessages : results;
        return JSON.stringify({
            messages: messages,
            page_title: document.title,
            channel_url: window.location.href,
            dm_usernames: window._discryptGetDmUsernames()
        });
    };

    if (window._discryptObserver) {
        window._discryptObserver.disconnect();
    }

    let debounceTimer = null;
    let lastData = '';
    try { lastData = window._discryptExtract(); } catch(e) {}

    function onMutation() {
        clearTimeout(debounceTimer);
        debounceTimer = setTimeout(() => {
            try {
                const data = window._discryptExtract();
                if (data !== lastData) {
                    lastData = data;
                    window.discryptUpdate(data);
                }
            } catch(e) {}
        }, 150);
    }

    const target = document.getElementById('app-mount') || document.body;
    window._discryptObserver = new MutationObserver(onMutation);
    window._discryptObserver.observe(target, {
        childList: true,
        subtree: true,
        characterData: true
    });

    window._discryptGetCurrentUser = function() {
        try {
            let user = null;
            webpackChunkdiscord_app.push([
                [Symbol()], {},
                (req) => {
                    for (const id in req.c) {
                        try {
                            const exp = req.c[id]?.exports;
                            if (exp?.default?.getCurrentUser) { user = exp.default.getCurrentUser(); }
                        } catch(_) {}
                        if (user) break;
                    }
                }
            ]);
            webpackChunkdiscord_app.pop();
            return user ? { id: user.id, username: user.username } : null;
        } catch(_) {}
        return null;
    };

    window._discryptGetChannelId = function() {
        const m = window.location.href.match(/channels\/(?:\d+|@me)\/(\d+)/);
        return m ? m[1] : null;
    };

    return 'setup_complete';
})();
"#;

const JS_FETCH: &str = "window._discryptExtract()";

const JS_HANDSHAKE_ANALYZE: &str = r#"
(async () => {
    const currentUser = window._discryptGetCurrentUser();
    if (!currentUser) return JSON.stringify({ error: 'Could not determine current user.' });

    const channelId = window._discryptGetChannelId();
    if (!channelId) return JSON.stringify({ error: 'No channel open. Navigate to a DM first.' });

    const KEY_TAG = '[DISCRYPT-KEY]';

    function findPinsPopout() {
        for (const sel of ['[class*="messagesPopout"]', '[class*="messagesPopoutWrap"]']) {
            const el = document.querySelector(sel);
            if (el && el.querySelectorAll('*').length > 10) return el;
        }
        const ariaEls = document.querySelectorAll('[aria-label*="Pinned Messages"]');
        for (const el of ariaEls) {
            if (el.querySelectorAll('*').length > 10) return el;
        }
        return null;
    }

    let popout = findPinsPopout();
    const wasOpen = !!popout;

    if (!wasOpen) {
        document.dispatchEvent(new KeyboardEvent('keydown', {
            key: 'p', code: 'KeyP', keyCode: 80,
            ctrlKey: true, bubbles: true, cancelable: true
        }));
        for (let i = 0; i < 40; i++) {
            await new Promise(r => setTimeout(r, 100));
            popout = findPinsPopout();
            if (popout) break;
        }
    }

    const theirKeys = [];
    let ourKeyPresent = false;

    if (popout) {
        await new Promise(r => setTimeout(r, 300));

        const allEls = popout.querySelectorAll('*');
        const seen = new Set();
        for (const el of allEls) {
            const fiberKey = Object.keys(el).find(k => k.startsWith('__reactFiber$'));
            if (!fiberKey) continue;
            let fiber = el[fiberKey];
            let attempts = 0;
            while (fiber && attempts < 30) {
                const props = fiber.memoizedProps || fiber.pendingProps || {};
                const msg = props.message;
                if (msg && msg.id && !seen.has(msg.id)) {
                    seen.add(msg.id);
                    if (typeof msg.content === 'string' && msg.content.startsWith(KEY_TAG)) {
                        const keyHex = msg.content.substring(KEY_TAG.length).trim();
                        if (msg.author && msg.author.id === currentUser.id) {
                            ourKeyPresent = true;
                        } else if (msg.author) {
                            theirKeys.push({
                                username: msg.author.username,
                                key_hex: keyHex,
                                message_id: msg.id
                            });
                        }
                    }
                }
                fiber = fiber.return;
                attempts++;
                if (msg && msg.id) break;
            }
        }

        // Fallback: text scraping
        if (!ourKeyPresent && theirKeys.length === 0) {
            const contentEls = popout.querySelectorAll('[class*="markup"], [class*="messageContent"], [class*="content"]');
            for (const cel of contentEls) {
                const txt = cel.textContent.trim();
                if (txt.startsWith(KEY_TAG)) {
                    const keyHex = txt.substring(KEY_TAG.length).trim();
                    const msgContainer = cel.closest('[class*="message"]') || cel.parentElement?.parentElement;
                    let authorName = null;
                    if (msgContainer) {
                        const nameEl = msgContainer.querySelector('[class*="username"], [class*="headerText"] span');
                        if (nameEl) authorName = nameEl.textContent.trim();
                    }
                    if (authorName && authorName === currentUser.username) {
                        ourKeyPresent = true;
                    } else if (authorName) {
                        theirKeys.push({ username: authorName, key_hex: keyHex, message_id: '' });
                    } else {
                        theirKeys.push({ username: '__unknown__', key_hex: keyHex, message_id: '' });
                    }
                }
            }
        }
    }

    if (!wasOpen && popout) {
        document.dispatchEvent(new KeyboardEvent('keydown', {
            key: 'p', code: 'KeyP', keyCode: 80,
            ctrlKey: true, bubbles: true, cancelable: true
        }));
    }

    return JSON.stringify({
        channel_id: channelId,
        our_username: currentUser.username,
        our_key_present: ourKeyPresent,
        their_keys: theirKeys
    });
})()
"#;

const JS_RIGHT_CLICK_KEY_MSG: &str = r#"
(() => {
    const KEY_TAG = '[DISCRYPT-KEY]';
    const scroller = document.querySelector('[class*="scrollerInner"]');
    if (!scroller) return JSON.stringify({ error: 'No message scroller found.' });
    const items = scroller.querySelectorAll('[id^="chat-messages-"]');
    let targetEl = null;
    for (let i = items.length - 1; i >= 0; i--) {
        const txt = items[i].textContent || '';
        if (txt.includes(KEY_TAG)) {
            targetEl = items[i];
            break;
        }
    }
    if (!targetEl) return JSON.stringify({ error: 'Could not find the key message.' });

    // Find a content element within the message to right-click on
    const contentEl = targetEl.querySelector('[class*="messageContent"], [class*="markup"]') || targetEl;
    const rect = contentEl.getBoundingClientRect();
    const x = rect.x + rect.width / 2;
    const y = rect.y + rect.height / 2;

    // Dispatch contextmenu event (right-click) to open Discord's context menu
    contentEl.dispatchEvent(new MouseEvent('contextmenu', {
        bubbles: true, cancelable: true, view: window,
        clientX: x, clientY: y, button: 2, buttons: 2
    }));

    return JSON.stringify({ success: true, x: x, y: y });
})()
"#;

const JS_CLICK_PIN_MENU_ITEM: &str = r#"
(() => {
    const menuItems = document.querySelectorAll('[role="menuitem"]');
    for (const mi of menuItems) {
        const text = mi.textContent.trim();
        if (/^pin message$/i.test(text)) {
            const rect = mi.getBoundingClientRect();
            mi.dispatchEvent(new MouseEvent('click', {
                bubbles: true, cancelable: true, view: window,
                shiftKey: true,
                clientX: rect.x + rect.width / 2,
                clientY: rect.y + rect.height / 2
            }));
            return JSON.stringify({ success: true, action: 'pinned' });
        }
        if (/^unpin message$/i.test(text)) {
            document.dispatchEvent(new KeyboardEvent('keydown', {
                key: 'Escape', code: 'Escape', keyCode: 27,
                bubbles: true, cancelable: true
            }));
            return JSON.stringify({ success: true, action: 'already_pinned' });
        }
    }
    document.dispatchEvent(new KeyboardEvent('keydown', {
        key: 'Escape', code: 'Escape', keyCode: 27,
        bubbles: true, cancelable: true
    }));
    return JSON.stringify({ error: 'Pin Message not found in context menu.' });
})()
"#;

fn cdp_eval_json(expression: &str, await_promise: bool) -> Result<Value, String> {
    let resp = send_cdp_command(
        "Runtime.evaluate",
        json!({ "expression": expression, "returnByValue": true, "awaitPromise": await_promise }),
    )?;
    let s = resp
        .get("result")
        .and_then(|r| r.get("result"))
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_str())
        .ok_or("JS evaluation returned no value")?;
    serde_json::from_str(s).map_err(|e| format!("JSON parse: {}", e))
}

fn pin_last_key_message() -> Result<bool, String> {
    // Right-click the key message to open Discord's context menu
    let click_result = cdp_eval_json(JS_RIGHT_CLICK_KEY_MSG, false)?;
    if let Some(e) = click_result.get("error") {
        return Err(e.as_str().unwrap_or("unknown").to_string());
    }
    // Wait for context menu to appear
    std::thread::sleep(Duration::from_millis(500));

    let pin_result = cdp_eval_json(JS_CLICK_PIN_MENU_ITEM, false)?;
    if let Some(e) = pin_result.get("error") {
        return Err(e.as_str().unwrap_or("unknown").to_string());
    }

    let action = pin_result.get("action").and_then(|a| a.as_str()).unwrap_or("");
    Ok(action == "already_pinned")
}

fn cdp_send_direct(ws: &mut CdpWs, method: &str, params: Value) -> Result<Value, String> {
    let id = next_id();
    let payload = json!({ "id": id, "method": method, "params": params });
    ws.send(Message::Text(payload.to_string()))
        .map_err(|e| format!("WebSocket send error: {}", e))?;

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if Instant::now() > deadline {
            return Err("CDP response timeout".into());
        }
        let msg = ws
            .read()
            .map_err(|e| format!("WebSocket read error: {}", e))?;
        if let Message::Text(txt) = msg {
            let data: Value =
                serde_json::from_str(&txt).map_err(|e| format!("JSON parse error: {}", e))?;
            if data.get("id").and_then(|v| v.as_u64()) == Some(id) {
                return Ok(data);
            }
        }
    }
}

fn set_read_timeout(ws: &CdpWs, timeout: Option<Duration>) {
    if let MaybeTlsStream::Plain(tcp) = ws.get_ref() {
        let _ = tcp.set_read_timeout(timeout);
    }
}

fn parse_fetch_result(resp: &Value) -> Result<FetchResult, String> {
    let result_value = resp
        .get("result")
        .and_then(|r| r.get("result"))
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            let exc = resp
                .get("result")
                .and_then(|r| r.get("exceptionDetails"))
                .map(|e| format!("{}", e))
                .unwrap_or_default();
            format!("No result from JS evaluation. {}", exc)
        })?;
    serde_json::from_str(result_value)
        .map_err(|e| format!("Failed to parse messages: {}", e))
}

pub fn decrypt_messages(result: &mut FetchResult) {
    for msg in &mut result.messages {
        if crate::crypto::is_encrypted(&msg.content) {
            if let Some(plaintext) = store::try_decrypt(&msg.content) {
                msg.content = plaintext;
                msg.encrypted = true;
            }
        }
    }
}

fn process_cdp_event(txt: &str, app_handle: &tauri::AppHandle) {
    let data: Value = match serde_json::from_str(txt) {
        Ok(v) => v,
        Err(_) => return,
    };
    let method = data
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("");
    if method == "Runtime.bindingCalled" {
        let name = data
            .get("params")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("");
        if name == "discryptUpdate" {
            if let Some(payload_str) = data
                .get("params")
                .and_then(|p| p.get("payload"))
                .and_then(|p| p.as_str())
            {
                if let Ok(mut result) = serde_json::from_str::<FetchResult>(payload_str) {
                    decrypt_messages(&mut result);
                    let _ = app_handle.emit("cdp-messages-updated", result);
                }
            }
        }
    }
}

fn execute_command(
    ws: &mut CdpWs,
    cmd: &CdpCommand,
    app_handle: &tauri::AppHandle,
) -> Result<Value, String> {
    let id = next_id();
    let payload = json!({ "id": id, "method": &cmd.method, "params": &cmd.params });
    ws.send(Message::Text(payload.to_string()))
        .map_err(|e| format!("WebSocket send error: {}", e))?;

    set_read_timeout(ws, Some(Duration::from_secs(10)));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if Instant::now() > deadline {
            return Err("CDP response timeout".into());
        }
        match ws.read() {
            Ok(Message::Text(txt)) => {
                let data: Value = serde_json::from_str(&txt)
                    .map_err(|e| format!("JSON parse error: {}", e))?;
                if data.get("id").and_then(|v| v.as_u64()) == Some(id) {
                    return Ok(data);
                }
                process_cdp_event(&txt, app_handle);
            }
            Ok(_) => {}
            Err(e) => return Err(format!("WebSocket read error: {}", e)),
        }
    }
}

fn cdp_event_loop(
    mut ws: CdpWs,
    cmd_rx: mpsc::Receiver<CdpCommand>,
    app_handle: tauri::AppHandle,
) {
    set_read_timeout(&ws, Some(Duration::from_millis(50)));

    loop {
        match cmd_rx.try_recv() {
            Ok(cmd) => {
                let result = execute_command(&mut ws, &cmd, &app_handle);
                let _ = cmd.response_tx.send(result);
                set_read_timeout(&ws, Some(Duration::from_millis(50)));
            }
            Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) => {}
        }

        match ws.read() {
            Ok(Message::Text(txt)) => {
                process_cdp_event(&txt, &app_handle);
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut => {}
            Err(_) => {
                // WebSocket broke — attempt auto-reconnect
                let _ = app_handle.emit("cdp-reconnecting", ());
                if let Some(new_ws) = try_reconnect(&app_handle) {
                    ws = new_ws;
                    set_read_timeout(&ws, Some(Duration::from_millis(50)));
                    let _ = app_handle.emit("cdp-reconnected", ());
                    // Re-fetch initial messages after reconnect
                    if let Ok(resp) = cdp_send_direct(
                        &mut ws,
                        "Runtime.evaluate",
                        json!({ "expression": JS_FETCH, "returnByValue": true, "awaitPromise": false }),
                    ) {
                        if let Ok(mut result) = parse_fetch_result(&resp) {
                            decrypt_messages(&mut result);
                            let _ = app_handle.emit("cdp-messages-updated", result);
                        }
                    }
                } else {
                    let _ = app_handle.emit("cdp-disconnected", ());
                    break;
                }
            }
        }
    }
}

fn try_reconnect(_app_handle: &tauri::AppHandle) -> Option<CdpWs> {
    let delays = [2, 4, 8, 16, 30]; // seconds
    for (attempt, &delay_secs) in delays.iter().enumerate() {
        eprintln!("CDP reconnect attempt {} in {}s…", attempt + 1, delay_secs);
        std::thread::sleep(Duration::from_secs(delay_secs));

        let targets = match discover_targets() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let target = match find_discord_target(&targets) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if target.web_socket_debugger_url.is_empty() {
            continue;
        }
        let url = match Url::parse(&target.web_socket_debugger_url) {
            Ok(u) => u,
            Err(_) => continue,
        };
        let (mut ws, _) = match ws_connect(url) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if cdp_send_direct(&mut ws, "Runtime.enable", json!({})).is_err() {
            continue;
        }
        let _ = cdp_send_direct(&mut ws, "Runtime.addBinding", json!({ "name": "discryptUpdate" }));
        if cdp_send_direct(
            &mut ws,
            "Runtime.evaluate",
            json!({ "expression": JS_SETUP, "returnByValue": true }),
        )
        .is_err()
        {
            continue;
        }

        eprintln!("CDP reconnected on attempt {}", attempt + 1);
        return Some(ws);
    }
    None
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscordStatus {
    pub running: bool,
    pub cdp_available: bool,
}

pub fn check_discord_status() -> DiscordStatus {
    let cdp_available = discover_targets().is_ok();
    let running = cdp_available || is_discord_process_running();
    DiscordStatus { running, cdp_available }
}

fn is_discord_process_running() -> bool {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq Discord.exe", "/NH"])
            .output()
            .map(|o| {
                let out = String::from_utf8_lossy(&o.stdout);
                out.contains("Discord.exe")
            })
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("pgrep")
            .arg("-x")
            .arg("Discord")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

pub fn launch_discord_with_cdp() -> Result<String, String> {
    #[cfg(target_os = "windows")]
    {
        let local_app_data = std::env::var("LOCALAPPDATA")
            .map_err(|_| "Could not read LOCALAPPDATA environment variable".to_string())?;

        let update_exe = std::path::PathBuf::from(&local_app_data)
            .join("Discord")
            .join("Update.exe");

        if !update_exe.exists() {
            return Err(format!(
                "Discord not found at expected path: {}\n\nPlease launch Discord manually with --remote-debugging-port=9222",
                update_exe.display()
            ));
        }

        // Kill existing Discord first
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/IM", "Discord.exe"])
            .output();
        std::thread::sleep(Duration::from_secs(2));

        std::process::Command::new(&update_exe)
            .args(["--processStart", "Discord.exe", "--process-start-args", "--remote-debugging-port=9222"])
            .spawn()
            .map_err(|e| format!("Failed to launch Discord: {}", e))?;

        Ok("Discord launched with CDP enabled. Wait a few seconds then click Connect.".into())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Auto-launch is only supported on Windows. Please launch Discord manually with --remote-debugging-port=9222".into())
    }
}

pub fn connect(app_handle: tauri::AppHandle) -> Result<FetchResult, String> {
    // Close existing session
    {
        let mut session = CDP_SESSION.lock().map_err(|e| format!("Lock error: {}", e))?;
        *session = None;
    }

    let targets = discover_targets()?;
    let target = find_discord_target(&targets)?;
    if target.web_socket_debugger_url.is_empty() {
        return Err("No WebSocket URL for target. The debugger may already be attached.".into());
    }

    let ws_url = target.web_socket_debugger_url.clone();
    let url = Url::parse(&ws_url).map_err(|e| format!("Invalid WS URL: {}", e))?;
    let (mut ws, _) =
        ws_connect(url).map_err(|e| format!("WebSocket connection failed: {}", e))?;

    cdp_send_direct(&mut ws, "Runtime.enable", json!({}))?;
    let _ = cdp_send_direct(
        &mut ws,
        "Runtime.addBinding",
        json!({ "name": "discryptUpdate" }),
    );

    cdp_send_direct(
        &mut ws,
        "Runtime.evaluate",
        json!({ "expression": JS_SETUP, "returnByValue": true }),
    )?;

    let resp = cdp_send_direct(
        &mut ws,
        "Runtime.evaluate",
        json!({ "expression": JS_FETCH, "returnByValue": true, "awaitPromise": false }),
    )?;
    let mut initial = parse_fetch_result(&resp)?;
    decrypt_messages(&mut initial);

    let (cmd_tx, cmd_rx) = mpsc::channel();
    let handle = app_handle.clone();
    std::thread::spawn(move || cdp_event_loop(ws, cmd_rx, handle));

    {
        let mut session = CDP_SESSION.lock().map_err(|e| format!("Lock error: {}", e))?;
        *session = Some(CdpSession { cmd_tx });
    }

    Ok(initial)
}

pub fn fetch_messages() -> Result<FetchResult, String> {
    let resp = send_cdp_command(
        "Runtime.evaluate",
        json!({ "expression": JS_FETCH, "returnByValue": true, "awaitPromise": false }),
    )?;
    let mut result = parse_fetch_result(&resp)?;
    decrypt_messages(&mut result);
    Ok(result)
}

pub fn send_message_via_cdp(text: &str) -> Result<(), String> {
    send_cdp_command("DOM.enable", json!({}))?;

    let doc_resp = send_cdp_command("DOM.getDocument", json!({ "depth": 0 }))?;
    let root_node_id = doc_resp
        .get("result")
        .and_then(|r| r.get("root"))
        .and_then(|r| r.get("nodeId"))
        .and_then(|v| v.as_i64())
        .ok_or("Failed to get document root nodeId")?;

    let qs_resp = send_cdp_command(
        "DOM.querySelector",
        json!({ "nodeId": root_node_id, "selector": "[role=\"textbox\"]" }),
    )?;
    let textbox_node_id = qs_resp
        .get("result")
        .and_then(|r| r.get("nodeId"))
        .and_then(|v| v.as_i64())
        .ok_or("Could not find Discord's message textbox. Make sure a channel or DM is open.")?;

    if textbox_node_id == 0 {
        return Err("Message textbox not found (nodeId=0). Open a channel or DM first.".into());
    }

    send_cdp_command("DOM.focus", json!({ "nodeId": textbox_node_id }))?;
    std::thread::sleep(Duration::from_millis(50));

    send_cdp_command("Input.insertText", json!({ "text": text }))?;
    std::thread::sleep(Duration::from_millis(50));

    send_cdp_command(
        "Input.dispatchKeyEvent",
        json!({ "type": "keyDown", "key": "Enter", "code": "Enter", "windowsVirtualKeyCode": 13, "nativeVirtualKeyCode": 13 }),
    )?;
    send_cdp_command(
        "Input.dispatchKeyEvent",
        json!({ "type": "keyUp", "key": "Enter", "code": "Enter", "windowsVirtualKeyCode": 13, "nativeVirtualKeyCode": 13 }),
    )?;

    Ok(())
}

pub fn send_message(message: &str, encrypt_for: Option<&str>) -> Result<(), String> {
    if message.trim().is_empty() {
        return Err("Message cannot be empty.".into());
    }

    let text_to_send = if let Some(contact) = encrypt_for {
        store::encrypt_for(contact, message)?
    } else {
        message.to_string()
    };

    send_message_via_cdp(&text_to_send)
}

pub fn handshake(app_handle: tauri::AppHandle) -> Result<HandshakeResult, String> {
    let _ = app_handle.emit("handshake-step", "Analyzing pinned messages…");

    let resp = send_cdp_command(
        "Runtime.evaluate",
        json!({
            "expression": JS_HANDSHAKE_ANALYZE,
            "returnByValue": true,
            "awaitPromise": true
        }),
    )?;

    let result_str = resp
        .get("result")
        .and_then(|r| r.get("result"))
        .and_then(|r| r.get("value"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            let exc = resp
                .get("result")
                .and_then(|r| r.get("exceptionDetails"))
                .map(|e| format!("{}", e))
                .unwrap_or_default();
            format!("Handshake JS failed. {}", exc)
        })?;

    let analysis: HandshakeAnalysis = serde_json::from_str(result_str)
        .map_err(|e| format!("Failed to parse handshake result: {}", e))?;

    if let Some(ref err) = analysis.error {
        return Err(err.clone());
    }

    let my_pubkey = store::get_public_key()?;
    let mut contact_added: Option<String> = None;
    let mut our_key_present = analysis.our_key_present;

    let _ = app_handle.emit("handshake-step", "Looking for their key…");

    for key_info in &analysis.their_keys {
        if key_info.username == "__unknown__" {
            if key_info.key_hex == my_pubkey {
                our_key_present = true;
            }
            continue;
        }
        if key_info.key_hex == my_pubkey {
            our_key_present = true;
            continue;
        }
        if !store::has_contact(&key_info.username)? {
            store::add_contact(&key_info.username, &key_info.key_hex)?;
            contact_added = Some(key_info.username.clone());
        }
    }

    let key_sent = if !our_key_present {
        let _ = app_handle.emit("handshake-step", "Sending your key…");
        let key_msg = format!("[DISCRYPT-KEY]{}", my_pubkey);

        send_message_via_cdp(&key_msg)?;
        std::thread::sleep(Duration::from_millis(800));

        let _ = app_handle.emit("handshake-step", "Pinning key message…");

        match pin_last_key_message() {
            Ok(true) => {
                our_key_present = true;
                false
            }
            Ok(false) => true,
            Err(e) => {
                eprintln!("Pin warning: {}", e);
                true
            }
        }
    } else {
        false
    };

    let their_key_found = analysis.their_keys.iter().any(|k| {
        k.username != "__unknown__" && k.key_hex != my_pubkey
    });
    let have_their_key = contact_added.is_some() || their_key_found;

    let (status, message) = match (have_their_key, our_key_present, key_sent, contact_added.as_ref()) {
        (_, _, true, Some(name)) => (
            "complete".to_string(),
            format!("Handshake complete with {}! Encryption active.", name),
        ),
        (_, true, false, Some(name)) => (
            "complete".to_string(),
            format!("Handshake complete with {}! Encryption active.", name),
        ),
        (true, _, true, None) => (
            "complete".to_string(),
            "Your key has been sent and pinned. Encryption active.".to_string(),
        ),
        (false, _, true, None) => (
            "key_sent".to_string(),
            "Your key has been sent and pinned. Waiting for the other user to handshake.".to_string(),
        ),
        (true, true, false, None) => (
            "complete".to_string(),
            "Handshake already complete. Encryption is active.".to_string(),
        ),
        (false, true, false, None) => (
            "waiting".to_string(),
            "Your key is already pinned. Waiting for the other user to share theirs.".to_string(),
        ),
        _ => (
            "waiting".to_string(),
            "Handshake in progress.".to_string(),
        ),
    };

    if let Some(ref name) = contact_added {
        let hs_status = if our_key_present { "complete" } else { "pending_ours" };
        let _ = store::set_handshake_status(name, hs_status);
    }

    Ok(HandshakeResult {
        status,
        message,
        contact_added,
        key_sent,
    })
}
