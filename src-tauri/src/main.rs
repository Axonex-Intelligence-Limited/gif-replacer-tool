// Phase 2 - Tauri GUI for GIF Replacer Tool

mod profile;
mod parser;
mod builder;
mod config;

use tauri::Manager;

#[tauri::command]
fn load_config() -> config::Config {
    config::load_config()
}

#[tauri::command]
fn save_config(config: config::Config) -> Result<(), String> {
    config::save_config(&config)
}

#[tauri::command]
fn get_active_profile(project_path: String) -> Result<ProfileResponse, String> {
    let profile = profile::detect_active_profile(&project_path)
        .map_err(|e| e.to_string())?;

    Ok(ProfileResponse {
        active_profile: profile.profile,
        emotions: profile.emotions,
        symbols: profile.symbols,
    })
}

#[tauri::command]
fn list_available_profiles(project_path: String) -> Result<Vec<String>, String> {
    profile::list_available_profiles(&project_path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_active_profile(project_path: String, profile_name: String) -> Result<(), String> {
    profile::set_active_profile(&project_path, &profile_name)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_default_emotion(project_path: String) -> Result<String, String> {
    profile::get_default_emotion(&project_path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_default_emotion(project_path: String, emotion: String) -> Result<(), String> {
    profile::set_default_emotion(&project_path, &emotion)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn create_profile_clone(project_path: String, base_name: String) -> Result<String, String> {
    profile::create_profile_clone(&project_path, &base_name)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn reset_profile_to_default(project_path: String, profile_name: String) -> Result<(), String> {
    profile::reset_profile_to_default(&project_path, &profile_name)
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct SerialPortInfo {
    port_name: String,
    port_type: String,
}

#[tauri::command]
fn list_serial_ports() -> Result<Vec<SerialPortInfo>, String> {
    let ports = serialport::available_ports()
        .map_err(|e| format!("Failed to list ports: {}", e))?;

    let port_list: Vec<SerialPortInfo> = ports
        .into_iter()
        // macOS lists each USB serial device twice: /dev/tty.* and /dev/cu.*
        // (two device files for the same physical port). Keep only cu.* —
        // it's the one used for flashing/writing.
        .filter(|p| {
            #[cfg(target_os = "macos")]
            {
                !p.port_name.contains("/tty.")
            }
            #[cfg(not(target_os = "macos"))]
            {
                true
            }
        })
        .map(|p| {
            let port_type = match p.port_type {
                serialport::SerialPortType::UsbPort(_) => "USB".to_string(),
                serialport::SerialPortType::PciPort => "PCI".to_string(),
                serialport::SerialPortType::BluetoothPort => "Bluetooth".to_string(),
                serialport::SerialPortType::Unknown => "Unknown".to_string(),
            };
            SerialPortInfo {
                port_name: p.port_name,
                port_type,
            }
        })
        .collect();

    Ok(port_list)
}

#[derive(serde::Serialize)]
struct ProfileResponse {
    active_profile: String,
    emotions: Vec<String>,
    symbols: Vec<String>,
}

#[tauri::command]
fn validate_c_file(file_path: String) -> Result<ValidateResponse, String> {
    let info = parser::validate_lvgl_image(&file_path)
        .map_err(|e| e.to_string())?;

    Ok(ValidateResponse {
        original_name: info.original_name,
        file_size_bytes: info.file_size_bytes,
    })
}

#[derive(serde::Serialize)]
struct ValidateResponse {
    original_name: String,
    file_size_bytes: usize,
}

#[tauri::command]
async fn replace_and_build_flash(
    app_handle: tauri::AppHandle,
    project_path: String,
    c_file_path: String,
    target_emotion: String,
    serial_port: String,
) -> Result<BuildFlashResponse, String> {
    // Step 1: Read and rename
    let content = std::fs::read_to_string(&c_file_path)
        .map_err(|e| format!("Read error: {}", e))?;
    let renamed = parser::rename_symbols(&content, &target_emotion)
        .map_err(|e| e.to_string())?;

    // Step 2: Get profile and replace file
    let profile = profile::detect_active_profile(&project_path)
        .map_err(|e| e.to_string())?;
    builder::replace_gif_file(&profile, &target_emotion, &renamed)
        .map_err(|e| e.to_string())?;

    emit_log(&app_handle, &format!("✓ Replaced main/{}/gif/{}.c", profile.profile, target_emotion));

    // Step 3: Build + flash
    let mut result = do_build_and_flash(app_handle, project_path, serial_port).await?;
    if result.success {
        result.message = "✅ Done! GIF updated on board.".to_string();
    }
    Ok(result)
}

/// Shared build + flash step, used by both the replace flow and the
/// set-default-emotion flow.
async fn do_build_and_flash(
    app_handle: tauri::AppHandle,
    project_path: String,
    serial_port: String,
) -> Result<BuildFlashResponse, String> {
    // idf.py build reads the generated sdkconfig, whose value overrides
    // sdkconfig.defaults — so sync it to the active profile before building,
    // otherwise the build compiles the wrong profile (e.g. the base instead of
    // the clone).
    profile::sync_generated_sdkconfig(&project_path)
        .map_err(|e| e.to_string())?;

    emit_log(&app_handle, "🔨 Starting build (this takes 2-3 minutes)...");
    let build_result = tokio::task::spawn_blocking({
        let project_path = project_path.clone();
        let app_handle = app_handle.clone();
        move || {
            match builder::run_build_sync(&project_path) {
                Ok(result) => {
                    // Emit output lines
                    for line in result.output.lines() {
                        let _ = app_handle.emit_all("build-output", BuildOutputEvent {
                            line: line.to_string(),
                        });
                    }
                    Ok(result)
                }
                Err(e) => Err(e.to_string())
            }
        }
    })
    .await
    .map_err(|e| format!("Build task error: {}", e))?
    .map_err(|e| e)?;

    if !build_result.success {
        return Ok(BuildFlashResponse {
            success: false,
            message: format!("Build failed with exit code {}", build_result.exit_code),
        });
    }

    emit_log(&app_handle, "✓ Build succeeded");

    // Flash
    emit_log(&app_handle, &format!("⚡ Starting flash to {}...", serial_port));
    let flash_result = tokio::task::spawn_blocking({
        let app_handle = app_handle.clone();
        let serial_port = serial_port.clone();
        move || {
            match builder::run_flash_sync(&project_path, &serial_port) {
                Ok(result) => {
                    // Emit output lines
                    for line in result.output.lines() {
                        let _ = app_handle.emit_all("build-output", BuildOutputEvent {
                            line: line.to_string(),
                        });
                    }
                    Ok(result)
                }
                Err(e) => Err(e.to_string())
            }
        }
    })
    .await
    .map_err(|e| format!("Flash task error: {}", e))?
    .map_err(|e| e)?;

    Ok(BuildFlashResponse {
        success: flash_result.success,
        message: if flash_result.success {
            "✅ Done! Build and flash complete.".to_string()
        } else {
            format!("Flash failed with exit code {}", flash_result.exit_code)
        },
    })
}

/// Builds and flashes without replacing a GIF file — used after changing the
/// default startup emotion so the new value is actually written to the board.
#[tauri::command]
async fn build_and_flash(
    app_handle: tauri::AppHandle,
    project_path: String,
    serial_port: String,
) -> Result<BuildFlashResponse, String> {
    let mut result = do_build_and_flash(app_handle, project_path, serial_port).await?;
    if result.success {
        result.message = "✅ Done! Default emotion updated on board.".to_string();
    }
    Ok(result)
}

#[derive(serde::Serialize)]
struct BuildFlashResponse {
    success: bool,
    message: String,
}

#[derive(Clone, serde::Serialize)]
struct BuildOutputEvent {
    line: String,
}

fn emit_log(app_handle: &tauri::AppHandle, message: &str) {
    let _ = app_handle.emit_all("build-output", BuildOutputEvent {
        line: message.to_string(),
    });
}

#[tauri::command]
async fn browse_folder() -> Option<String> {
    use tauri::api::dialog::blocking::FileDialogBuilder;
    FileDialogBuilder::new()
        .pick_folder()
        .map(|p| p.to_string_lossy().to_string())
}

#[cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            load_config,
            save_config,
            get_active_profile,
            list_available_profiles,
            set_active_profile,
            get_default_emotion,
            set_default_emotion,
            create_profile_clone,
            reset_profile_to_default,
            list_serial_ports,
            validate_c_file,
            replace_and_build_flash,
            build_and_flash,
            browse_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
