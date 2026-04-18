use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Serialize, Deserialize)]
pub struct Device {
    pub serial: String,
    pub status: String,
    pub model: Option<String>,
    pub device_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub serial: String,
    pub model: String,
    pub manufacturer: String,
    pub android_version: String,
    pub sdk_version: String,
    pub screen_resolution: String,
    pub density: String,
    pub battery_level: String,
    pub battery_status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppInfo {
    pub package_name: String,
    pub app_name: Option<String>,
    pub version_name: Option<String>,
    pub version_code: Option<String>,
    pub is_system_app: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: Option<String>,
    pub permissions: Option<String>,
    pub modified_time: Option<String>,
}

fn run_adb_command(args: &[&str]) -> Result<String, String> {
    eprintln!("ADB command: adb {}", args.join(" "));
    let output = Command::new("adb")
        .args(args)
        .output()
        .map_err(|e| format!("Failed to execute adb: {}", e))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        eprintln!("ADB stdout: {}", stdout);
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        eprintln!("ADB stderr: {}", stderr);
        Err(stderr)
    }
}

#[tauri::command]
fn get_devices() -> Result<Vec<Device>, String> {
    let output = run_adb_command(&["devices", "-l"])?;
    let mut devices = Vec::new();

    for line in output.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let serial = parts[0].to_string();
            let status = parts[1].to_string();
            let mut model = None;
            let mut device_type = None;

            for part in &parts[2..] {
                if part.starts_with("model:") {
                    model = Some(part[6..].to_string());
                } else if part.starts_with("device:") {
                    device_type = Some(part[7..].to_string());
                }
            }

            devices.push(Device {
                serial,
                status,
                model,
                device_type,
            });
        }
    }

    Ok(devices)
}

#[tauri::command]
fn get_device_info(serial: &str) -> Result<DeviceInfo, String> {
    let get_prop = |prop: &str| -> String {
        run_adb_command(&["-s", serial, "shell", "getprop", prop])
            .unwrap_or_default()
            .trim()
            .to_string()
    };

    let model = get_prop("ro.product.model");
    let manufacturer = get_prop("ro.product.manufacturer");
    let android_version = get_prop("ro.build.version.release");
    let sdk_version = get_prop("ro.build.version.sdk");

    let screen_output = run_adb_command(&["-s", serial, "shell", "wm", "size"]).unwrap_or_default();
    let screen_resolution = screen_output
        .lines()
        .next()
        .and_then(|line| line.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let density_output = run_adb_command(&["-s", serial, "shell", "wm", "density"]).unwrap_or_default();
    let density = density_output
        .lines()
        .next()
        .and_then(|line| line.split(':').nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let battery_output = run_adb_command(&["-s", serial, "shell", "dumpsys", "battery"]).unwrap_or_default();
    let mut battery_level = String::new();
    let mut battery_status = String::new();

    for line in battery_output.lines() {
        if line.contains("level:") {
            battery_level = line.split(':').nth(1).map(|s| s.trim().to_string()).unwrap_or_default();
        }
        if line.contains("status:") {
            battery_status = line.split(':').nth(1).map(|s| s.trim().to_string()).unwrap_or_default();
        }
    }

    Ok(DeviceInfo {
        serial: serial.to_string(),
        model,
        manufacturer,
        android_version,
        sdk_version,
        screen_resolution,
        density,
        battery_level,
        battery_status,
    })
}

#[tauri::command]
fn connect_device(ip: &str, port: Option<u16>) -> Result<String, String> {
    let port = port.unwrap_or(5555);
    let address = format!("{}:{}", ip, port);
    run_adb_command(&["connect", &address])
}

#[tauri::command]
fn disconnect_device(address: Option<&str>) -> Result<String, String> {
    match address {
        Some(addr) => run_adb_command(&["disconnect", addr]),
        None => run_adb_command(&["disconnect"]),
    }
}

#[tauri::command]
fn install_apk(serial: &str, apk_path: &str) -> Result<String, String> {
    run_adb_command(&["-s", serial, "install", "-r", apk_path])
}

#[tauri::command]
fn uninstall_app(serial: &str, package_name: &str) -> Result<String, String> {
    run_adb_command(&["-s", serial, "uninstall", package_name])
}

#[tauri::command]
fn get_installed_apps(serial: &str, filter: Option<&str>) -> Result<Vec<AppInfo>, String> {
    let flag = match filter {
        Some("system") => "-s",
        Some("third") => "-3",
        _ => "",
    };

    let output = if flag.is_empty() {
        run_adb_command(&["-s", serial, "shell", "pm", "list", "packages", "-f"])?
    } else {
        run_adb_command(&["-s", serial, "shell", "pm", "list", "packages", "-f", flag])?
    };

    let mut apps = Vec::new();

    for line in output.lines() {
        if line.starts_with("package:") {
            // 找到最后一个 '=' 的位置，因为包名在最后一个 '=' 后面
            if let Some(pos) = line.rfind('=') {
                let package_name = line[pos + 1..].to_string();
                let is_system_app = filter == Some("system") || 
                    (!package_name.starts_with("com.android") && 
                     !package_name.starts_with("com.google") &&
                     line.contains("/system/"));

                apps.push(AppInfo {
                    package_name,
                    app_name: None,
                    version_name: None,
                    version_code: None,
                    is_system_app,
                });
            }
        }
    }

    Ok(apps)
}

#[tauri::command]
fn start_app(serial: &str, package_name: &str, activity: Option<&str>) -> Result<String, String> {
    let component = match activity {
        Some(act) => format!("{}/{}", package_name, act),
        None => format!("{}/.MainActivity", package_name),
    };
    run_adb_command(&["-s", serial, "shell", "am", "start", "-n", &component])
}

#[tauri::command]
fn stop_app(serial: &str, package_name: &str) -> Result<String, String> {
    run_adb_command(&["-s", serial, "shell", "am", "force-stop", package_name])
}

#[tauri::command]
fn clear_app_data(serial: &str, package_name: &str) -> Result<String, String> {
    run_adb_command(&["-s", serial, "shell", "pm", "clear", package_name])
}

#[tauri::command]
fn list_data_apps(serial: &str) -> Result<Vec<AppInfo>, String> {
    let output = run_adb_command(&[
        "-s", serial, "shell", "pm", "list", "packages",
    ])?;

    let mut apps = Vec::new();

    for line in output.lines() {
        if line.starts_with("package:") {
            let package_name = line.trim_start_matches("package:").trim().to_string();
            
            let test_cmd = run_adb_command(&[
                "-s", serial, "shell", "run-as", &package_name, "id",
            ]);

            let is_accessible = match test_cmd {
                Ok(output) => {
                    let s = output.trim();
                    !s.starts_with("run-as:") && !s.contains("not debuggable")
                },
                Err(_) => false,
            };

            if is_accessible {
                apps.push(AppInfo {
                    package_name,
                    app_name: None,
                    version_name: None,
                    version_code: None,
                    is_system_app: false,
                });
            }
        }
    }

    Ok(apps)
}

#[tauri::command]
fn test_app_debuggable(serial: &str, package_name: &str) -> Result<bool, String> {
    let test_cmd = run_adb_command(&[
        "-s", serial, "shell", "run-as", package_name, "id",
    ]);

    match test_cmd {
        Ok(output) => {
            let s = output.trim();
            Ok(!s.starts_with("run-as:") && !s.contains("not debuggable"))
        },
        Err(_) => Ok(false),
    }
}

#[tauri::command]
fn run_as_list_files(serial: &str, package_name: &str, path: &str) -> Result<Vec<FileInfo>, String> {
    let clean_path = if path.is_empty() || path == "/" { "" } else { path };
    
    let output = run_adb_command(&[
        "-s", serial, "shell", "run-as", package_name, "ls", "-la", clean_path,
    ])?;

    let trimmed = output.trim();
    
    if trimmed.is_empty() {
        return Err(format!("路径 '{}' 不存在或为空", path));
    }
    
    if trimmed.starts_with("run-as:") {
        return Err(format!("无法访问应用 {}: {}", package_name, trimmed));
    }
    if trimmed.contains("not debuggable") {
        return Err(format!("应用 {} 不可调试", package_name));
    }
    if trimmed.contains("does not exist") || trimmed.contains("No such file") {
        return Err(format!("路径 '{}' 不存在", path));
    }

    parse_file_list(&output, clean_path)
}

#[tauri::command]
fn list_files(serial: &str, path: &str) -> Result<Vec<FileInfo>, String> {
    let clean_path = if path.is_empty() { "/" } else { path };
    
    let output = run_adb_command(&[
        "-s", serial, "shell", "ls", "-la", clean_path,
    ])?;

    if output.contains("Permission denied") {
        if clean_path.starts_with("/data/data/") {
            let after_data = &clean_path[11..];
            if let Some(slash_idx) = after_data.find('/') {
                let package_name = &after_data[..slash_idx];
                let relative_path = &after_data[slash_idx + 1..];
                
                let run_as_output = run_adb_command(&[
                    "-s", serial, "shell", "run-as", package_name, "ls", "-la", relative_path,
                ])?;

                if run_as_output.contains("Package") && run_as_output.contains("not debuggable") {
                    return Err(format!(
                        "无法访问: {}\n\n提示: 应用 {} 不可调试。\nrun-as 命令只能访问可调试应用的数据。",
                        clean_path, package_name
                    ));
                }

                return parse_file_list(&run_as_output, &relative_path);
            }
        }
        
        return Err(format!(
            "权限不足: {}\n\n提示: /data/data 需要 root 权限才能访问。\n- 模拟器通常默认有 root 权限\n- 真机需要已 root 并开启 ADB root",
            clean_path
        ));
    }

    parse_file_list(&output, clean_path)
}

fn parse_file_list(output: &str, base_path: &str) -> Result<Vec<FileInfo>, String> {
    let mut files = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("total") || trimmed == "ls:" {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 6 {
            continue;
        }

        // 跳过隐藏文件/目录（以 . 开头）
        let name = parts.last().unwrap_or(&"");
        if name.starts_with('.') {
            continue;
        }

        let permissions = parts[0].to_string();
        let is_dir = permissions.starts_with('d') || permissions.starts_with('l');
        
        let size: Option<String> = if !is_dir && parts.len() >= 5 {
            parts[4].parse::<u64>().ok().map(|s| format!("{} B", s))
        } else {
            None
        };
        
        let modified_time: Option<String> = if parts.len() >= 8 {
            Some(format!("{} {} {}", parts[5], parts[6], parts[7]))
        } else if parts.len() >= 7 {
            Some(format!("{} {}", parts[5], parts[6]))
        } else {
            None
        };
        
        let name = parts.last().unwrap_or(&"").to_string();
        let clean_base = base_path.trim_end_matches('/');
        let full_path = if clean_base == "" {
            format!("/{}", name)
        } else {
            format!("{}/{}", clean_base, name)
        };

        files.push(FileInfo {
            name,
            path: full_path,
            is_dir,
            size,
            permissions: Some(permissions),
            modified_time,
        });
    }

    Ok(files)
}

#[tauri::command]
fn push_file(serial: &str, local_path: &str, remote_path: &str) -> Result<String, String> {
    run_adb_command(&["-s", serial, "push", local_path, remote_path])
}

#[tauri::command]
fn pull_file(serial: &str, remote_path: &str, local_path: &str) -> Result<String, String> {
    run_adb_command(&["-s", serial, "pull", remote_path, local_path])
}

#[tauri::command]
fn delete_file(serial: &str, remote_path: &str) -> Result<String, String> {
    run_adb_command(&["-s", serial, "shell", "rm", "-rf", remote_path])
}

#[tauri::command]
fn take_screenshot(serial: &str, save_path: &str) -> Result<String, String> {
    let temp_path = "/sdcard/screenshot.png";
    run_adb_command(&["-s", serial, "shell", "screencap", "-p", temp_path])?;
    run_adb_command(&["-s", serial, "pull", temp_path, save_path])?;
    run_adb_command(&["-s", serial, "shell", "rm", temp_path])?;
    Ok("Screenshot saved successfully".to_string())
}

#[tauri::command]
fn start_screen_record(serial: &str, save_path: &str, duration: Option<u32>) -> Result<String, String> {
    let time_limit = duration.unwrap_or(30);
    let temp_path = "/sdcard/screenrecord.mp4";
    run_adb_command(&[
        "-s", serial, "shell", "screenrecord", 
        "--time-limit", &time_limit.to_string(),
        temp_path
    ])?;
    run_adb_command(&["-s", serial, "pull", temp_path, save_path])?;
    run_adb_command(&["-s", serial, "shell", "rm", temp_path])?;
    Ok("Screen recording saved successfully".to_string())
}

#[tauri::command]
fn reboot_device(serial: &str, mode: Option<&str>) -> Result<String, String> {
    match mode {
        Some("recovery") => run_adb_command(&["-s", serial, "reboot", "recovery"]),
        Some("bootloader") => run_adb_command(&["-s", serial, "reboot", "bootloader"]),
        _ => run_adb_command(&["-s", serial, "reboot"]),
    }
}

#[tauri::command]
fn run_shell_command(serial: &str, command: &str) -> Result<String, String> {
    let args: Vec<&str> = command.split_whitespace().collect();
    let mut cmd_args = vec!["-s", serial, "shell"];
    cmd_args.extend(args);
    run_adb_command(&cmd_args)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_devices,
            get_device_info,
            connect_device,
            disconnect_device,
            install_apk,
            uninstall_app,
            get_installed_apps,
            list_data_apps,
            test_app_debuggable,
            run_as_list_files,
            start_app,
            stop_app,
            clear_app_data,
            list_files,
            push_file,
            pull_file,
            delete_file,
            take_screenshot,
            start_screen_record,
            reboot_device,
            run_shell_command,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
