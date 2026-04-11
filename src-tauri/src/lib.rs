mod cdp;
mod crypto;
mod store;

use tauri::Manager;
use tracing::error;


#[tauri::command]
fn store_exists() -> Result<bool, String> {
    store::store_exists()
}

#[tauri::command]
fn store_format() -> Result<String, String> {
    store::store_format()
}

#[tauri::command]
fn is_store_unlocked() -> bool {
    store::is_unlocked()
}

#[tauri::command]
fn create_store(password: String) -> Result<(), String> {
    store::create_store(&password)
}

#[tauri::command]
fn migrate_store(password: String) -> Result<(), String> {
    store::migrate_store(&password)
}

#[tauri::command]
fn unlock_store(password: String) -> Result<(), String> {
    store::unlock_store(&password)
}

#[tauri::command]
fn change_password(old_password: String, new_password: String) -> Result<(), String> {
    store::change_password(&old_password, &new_password)
}

#[tauri::command]
fn export_store() -> Result<String, String> {
    store::export_store()
}

#[tauri::command]
fn import_store(json_data: String, password: String) -> Result<(), String> {
    store::import_store(&json_data, &password)
}

#[tauri::command]
fn reset_store() -> Result<(), String> {
    store::reset_store()
}

#[tauri::command]
fn cdp_connect(app_handle: tauri::AppHandle) -> Result<cdp::FetchResult, String> {
    cdp::connect(app_handle)
}

#[tauri::command]
fn check_discord_status() -> cdp::DiscordStatus {
    cdp::check_discord_status()
}

#[tauri::command]
fn launch_discord_with_cdp() -> Result<String, String> {
    cdp::launch_discord_with_cdp()
}

#[tauri::command]
fn cdp_fetch_messages() -> Result<cdp::FetchResult, String> {
    cdp::fetch_messages()
}

#[tauri::command]
fn cdp_send_message(message: String, encrypt_for: Option<String>) -> Result<(), String> {
    cdp::send_message(&message, encrypt_for.as_deref())
}

#[tauri::command]
fn cdp_handshake(app_handle: tauri::AppHandle) -> Result<cdp::HandshakeResult, String> {
    cdp::handshake(app_handle)
}

#[tauri::command]
fn get_my_public_key() -> Result<String, String> {
    store::get_public_key()
}

#[tauri::command]
fn add_contact(username: String, public_key_hex: String) -> Result<(), String> {
    store::add_contact(&username, &public_key_hex)
}

#[tauri::command]
fn remove_contact(username: String) -> Result<(), String> {
    store::remove_contact(&username)
}

#[tauri::command]
fn list_contacts() -> Result<Vec<store::ContactInfo>, String> {
    store::list_contacts()
}


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| {
                    error!("Failed to get app data directory: {}", e);
                    e
                })?;
            store::init(data_dir).map_err(|e| {
                error!("Failed to set store path: {}", e);
                Box::<dyn std::error::Error>::from(e)
            })?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            store_exists,
            store_format,
            is_store_unlocked,
            create_store,
            migrate_store,
            unlock_store,
            change_password,
            export_store,
            import_store,
            reset_store,
            cdp_connect,
            check_discord_status,
            launch_discord_with_cdp,
            cdp_fetch_messages,
            cdp_send_message,
            cdp_handshake,
            get_my_public_key,
            add_contact,
            remove_contact,
            list_contacts,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
