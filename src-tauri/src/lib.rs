use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::OnceLock;
use std::sync::mpsc::{channel, Sender, TryRecvError};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

pub struct OAuthState(pub Mutex<Option<Sender<()>>>);

// 录屏会话：保存本地 adb 子进程、设备序列号、远端文件路径
pub struct RecordingSession {
    pub child: Child,
    pub serial: String,
    pub remote_path: String,
}

pub struct RecordingState(pub Mutex<Option<RecordingSession>>);

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
    pub density_dpi: String,
    pub smallest_width: String,
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

// 双击 .app 启动时进程的 PATH 不包含用户 shell 路径，找不到 adb。
// 这里在常见位置依次找 adb，并缓存绝对路径，所有 adb 调用都走这个。
static ADB_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

fn locate_adb() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    // 1) PATH 里直接能找到（dev / 终端启动场景）
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            candidates.push(dir.join(if cfg!(windows) { "adb.exe" } else { "adb" }));
        }
    }

    // 2) macOS / Linux 常见位置
    if let Ok(home) = std::env::var("HOME") {
        let home = std::path::Path::new(&home);
        candidates.push(home.join("Library/Android/sdk/platform-tools/adb"));
        candidates.push(home.join("Android/Sdk/platform-tools/adb"));
        candidates.push(home.join(".android/sdk/platform-tools/adb"));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/adb")); // Apple Silicon brew
    candidates.push(PathBuf::from("/usr/local/bin/adb")); // Intel brew
    candidates.push(PathBuf::from("/usr/bin/adb"));

    // 3) ANDROID_HOME / ANDROID_SDK_ROOT
    for env_key in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Ok(root) = std::env::var(env_key) {
            candidates.push(std::path::Path::new(&root).join("platform-tools/adb"));
        }
    }

    // 4) Windows
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        candidates.push(std::path::Path::new(&local).join("Android/Sdk/platform-tools/adb.exe"));
    }

    candidates.into_iter().find(|p| p.is_file())
}

fn adb_path() -> Result<&'static PathBuf, String> {
    let resolved = ADB_PATH.get_or_init(locate_adb);
    resolved
        .as_ref()
        .ok_or_else(|| "未找到 adb 可执行文件。请安装 Android Platform Tools，或确认其在 PATH 中。".to_string())
}

fn run_adb_command(args: &[&str]) -> Result<String, String> {
    let adb = adb_path()?;
    eprintln!("ADB command: {} {}", adb.display(), args.join(" "));
    let output = Command::new(adb)
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

    // wm size / wm density 可能返回 "Physical ..." 和 "Override ..." 两行。
    // 生效值以 Override 为准；没有 Override 时使用 Physical。
    // smallest_width（dp）= 短边像素 × 160 / 生效 dpi，与开发者选项中的"最小宽度"一致。
    let screen_output = run_adb_command(&["-s", serial, "shell", "wm", "size"]).unwrap_or_default();
    let (physical_size, override_size) = parse_wm_two_values(&screen_output);
    let effective_size = override_size.clone().or_else(|| physical_size.clone());
    let screen_resolution = effective_size.clone().unwrap_or_default();

    let density_output = run_adb_command(&["-s", serial, "shell", "wm", "density"]).unwrap_or_default();
    let (physical_density, override_density) = parse_wm_two_values(&density_output);
    let effective_density = override_density.clone().or_else(|| physical_density.clone());
    let density_dpi = effective_density.clone().unwrap_or_default();
    // density（比例）= densityDpi / 160，保留两位小数
    let density = effective_density
        .as_ref()
        .and_then(|s| s.trim().parse::<f32>().ok())
        .map(|dpi| format!("{:.2}", dpi / 160.0))
        .unwrap_or_default();

    let smallest_width = compute_smallest_width_dp(&effective_size, &effective_density)
        .map(|dp| format!("{} dp", dp))
        .unwrap_or_default();

    Ok(DeviceInfo {
        serial: serial.to_string(),
        model,
        manufacturer,
        android_version,
        sdk_version,
        screen_resolution,
        density,
        density_dpi,
        smallest_width,
    })
}

// 解析 `wm size` / `wm density` 形如：
//   "Physical size: 1080x2400\nOverride size: 1080x2400"
// 返回 (physical_value, override_value)，都 trim 过且不含前缀
fn parse_wm_two_values(output: &str) -> (Option<String>, Option<String>) {
    let mut physical = None;
    let mut override_v = None;
    for line in output.lines() {
        let trimmed = line.trim();
        let value = trimmed.split(':').nth(1).map(|s| s.trim().to_string());
        if trimmed.starts_with("Physical") {
            physical = value;
        } else if trimmed.starts_with("Override") {
            override_v = value;
        }
    }
    (physical, override_v)
}

// size 形如 "1080x2400"，density 形如 "446"，计算 sw_dp
fn compute_smallest_width_dp(size: &Option<String>, density: &Option<String>) -> Option<u32> {
    let size_str = size.as_ref()?;
    let (w, h) = size_str.split_once('x')?;
    let w: u32 = w.trim().parse().ok()?;
    let h: u32 = h.trim().parse().ok()?;
    let dpi: u32 = density.as_ref()?.trim().parse().ok()?;
    if dpi == 0 {
        return None;
    }
    Some(w.min(h) * 160 / dpi)
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
    // -r: 覆盖安装  -t: 允许安装 testOnly 标记的 APK（debug 包）
    run_adb_command(&["-s", serial, "install", "-r", "-t", apk_path])
}

// 调用 aapt 解析 APK 的包名。先从 PATH 找，再尝试 ANDROID_HOME/build-tools 下最新版本
#[tauri::command]
fn get_apk_package_name(apk_path: &str) -> Result<String, String> {
    let aapt_candidates = find_aapt_binaries();
    if aapt_candidates.is_empty() {
        return Err(
            "未找到 aapt 工具。请安装 Android SDK Build Tools，或设置 ANDROID_HOME 环境变量。"
                .to_string(),
        );
    }

    for aapt in &aapt_candidates {
        let result = Command::new(aapt)
            .args(["dump", "badging", apk_path])
            .output();
        if let Ok(output) = result {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(pkg) = parse_package_from_aapt(&stdout) {
                    return Ok(pkg);
                }
            }
        }
    }

    Err("aapt 解析失败：未能从 APK 中提取 package 名".to_string())
}

fn parse_package_from_aapt(output: &str) -> Option<String> {
    let line = output.lines().find(|l| l.starts_with("package:"))?;
    let start = line.find("name='")? + 6;
    let rest = &line[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

fn find_aapt_binaries() -> Vec<std::path::PathBuf> {
    let mut list = Vec::new();

    // 1. 先假定 aapt 在 PATH 中（Command 会自动走 PATH 查找）
    list.push(std::path::PathBuf::from("aapt"));
    list.push(std::path::PathBuf::from("aapt2"));

    // 2. 扫描 ANDROID_HOME / ANDROID_SDK_ROOT / macOS 默认路径下的 build-tools
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(h) = std::env::var("ANDROID_HOME") {
        roots.push(h.into());
    }
    if let Ok(h) = std::env::var("ANDROID_SDK_ROOT") {
        roots.push(h.into());
    }
    if let Ok(home) = std::env::var("HOME") {
        roots.push(std::path::Path::new(&home).join("Library/Android/sdk"));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        roots.push(std::path::Path::new(&local).join("Android/Sdk"));
    }

    for root in roots {
        let bt = root.join("build-tools");
        if let Ok(entries) = std::fs::read_dir(&bt) {
            let mut versions: Vec<_> = entries.filter_map(|e| e.ok()).collect();
            versions.sort_by_key(|e| e.file_name()); // 字符串升序，最后一个通常是最新版
            if let Some(latest) = versions.last() {
                let p = latest.path();
                let aapt = p.join(if cfg!(windows) { "aapt.exe" } else { "aapt" });
                if aapt.exists() {
                    list.push(aapt);
                }
                let aapt2 = p.join(if cfg!(windows) { "aapt2.exe" } else { "aapt2" });
                if aapt2.exists() {
                    list.push(aapt2);
                }
            }
        }
    }

    list
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
    // 若指定了 activity，仍然用 am start -n；否则用 monkey 让系统自动解析 LAUNCHER Activity，
    // 这样无论启动页是 SplashActivity 还是别的名字都能正确打开。
    if let Some(act) = activity {
        let component = format!("{}/{}", package_name, act);
        return run_adb_command(&["-s", serial, "shell", "am", "start", "-n", &component]);
    }

    run_adb_command(&[
        "-s",
        serial,
        "shell",
        "monkey",
        "-p",
        package_name,
        "-c",
        "android.intent.category.LAUNCHER",
        "1",
    ])
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
        // 标准 `ls -la` 输出 8 列：perm links owner group size date time name[...]
        // 符号链接会多出 "-> target" 两段
        if parts.len() < 8 {
            continue;
        }

        let permissions = parts[0].to_string();
        let is_symlink = permissions.starts_with('l');
        let is_dir = permissions.starts_with('d') || is_symlink;

        // 提取文件名：从第 8 个字段（index 7）起到末尾 join 成一整个 name
        // 符号链接遇到 "->" 则截断，取 " -> " 前面的部分作为链接名
        let name_start = 7;
        let name_end = if is_symlink {
            parts[name_start..]
                .iter()
                .position(|&p| p == "->")
                .map(|i| name_start + i)
                .unwrap_or(parts.len())
        } else {
            parts.len()
        };
        if name_start >= name_end {
            continue;
        }
        let name = parts[name_start..name_end].join(" ");

        // 跳过隐藏文件/目录（以 . 开头）
        if name.starts_with('.') {
            continue;
        }
        // 跳过 Android 框架在应用 data 目录下自动生成的内部子目录（Context.getDir(name) → app_<name>），
        // 例如 app_webview / app_textures / app_tmppccache 等，日常调试基本用不到
        if name.starts_with("app_") {
            continue;
        }

        let size: Option<String> = if !is_dir {
            parts[4].parse::<u64>().ok().map(|s| format!("{} B", s))
        } else {
            None
        };

        let modified_time: Option<String> = Some(format!("{} {}", parts[5], parts[6]));

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
    // 先尝试普通 adb pull
    let pull_res = run_adb_command(&["-s", serial, "pull", remote_path, local_path]);
    if pull_res.is_ok() {
        return pull_res;
    }
    let err = pull_res.unwrap_err();

    // /data/data/<pkg>/<rel> 下的文件在非 root 设备 adb pull 必然失败，
    // 走 run-as 回退（与 preview_remote_file 行为一致）
    if remote_path.starts_with("/data/data/") {
        let after = &remote_path[11..];
        if let Some(i) = after.find('/') {
            let pkg = &after[..i];
            let rel = &after[i + 1..];
            exec_out_run_as_cat(serial, pkg, rel, std::path::Path::new(local_path))?;
            return Ok(format!("已通过 run-as 拉取到 {}", local_path));
        }
    }
    Err(err)
}

#[tauri::command]
fn delete_file(serial: &str, remote_path: &str) -> Result<String, String> {
    // /data/data/<pkg>/<rel> 下的文件在非 root 设备直接 rm 会 Permission denied，
    // 走 run-as 回退（与 pull/preview 行为一致）
    if remote_path.starts_with("/data/data/") {
        let after = &remote_path[11..];
        if let Some(i) = after.find('/') {
            let pkg = &after[..i];
            let rel = &after[i + 1..];
            let output = Command::new(adb_path()?)
                .args(["-s", serial, "shell", "run-as", pkg, "rm", "-rf", rel])
                .output()
                .map_err(|e| format!("run-as rm 执行失败: {}", e))?;
            if !output.status.success() {
                return Err(format!(
                    "run-as rm 失败: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            return Ok(String::from_utf8_lossy(&output.stdout).to_string());
        }
    }
    run_adb_command(&["-s", serial, "shell", "rm", "-rf", remote_path])
}

// ---------- 文件预览：拉到本地临时目录 → 读取 → 返回内容 ----------

#[derive(Debug, Serialize)]
pub struct FilePreview {
    pub kind: String,              // "text" | "image" | "binary"
    pub mime: String,
    pub size: u64,
    pub text: Option<String>,      // kind == "text"
    pub data_url: Option<String>,  // kind == "image"
    pub temp_path: String,
}

const PREVIEW_MAX_BYTES: u64 = 5 * 1024 * 1024; // 5 MB

fn preview_temp_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("adbtools-preview")
}

fn ext_lower(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default()
}

fn is_text_ext(ext: &str) -> bool {
    matches!(
        ext,
        "txt" | "log" | "json" | "xml" | "html" | "htm" | "css" | "js" | "ts"
            | "kt" | "java" | "md" | "properties" | "sh" | "conf" | "yaml" | "yml"
            | "ini" | "toml"
    )
}

fn image_mime(ext: &str) -> Option<&'static str> {
    match ext {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        _ => None,
    }
}

// 对无扩展或未知扩展的文件做内容嗅探：前 4KB 能解为 UTF-8 且不含过多控制字节 → 文本
fn sniff_is_text(bytes: &[u8]) -> bool {
    let probe = &bytes[..bytes.len().min(4096)];
    if probe.is_empty() {
        return true;
    }
    let s = match std::str::from_utf8(probe) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let total = s.chars().count() as f32;
    if total == 0.0 {
        return true;
    }
    let printable = s
        .chars()
        .filter(|c| !c.is_control() || matches!(*c, '\n' | '\r' | '\t'))
        .count() as f32;
    printable / total > 0.95
}

#[tauri::command]
fn preview_remote_file(serial: &str, remote_path: &str) -> Result<FilePreview, String> {
    let dir = preview_temp_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建临时目录失败: {}", e))?;

    let original_name = std::path::Path::new(remote_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let temp_file = dir.join(format!("{}_{}", ts, original_name));
    let temp_path_str = temp_file.to_string_lossy().to_string();

    // 先尝试 adb pull
    let pull_res = run_adb_command(&["-s", serial, "pull", remote_path, &temp_path_str]);

    // 若 pull 失败且路径在 /data/data/ 下，走 run-as cat 回退
    if let Err(err) = &pull_res {
        let need_run_as = err.contains("Permission denied") || err.contains("does not exist")
            || remote_path.starts_with("/data/data/");
        if need_run_as && remote_path.starts_with("/data/data/") {
            let after = &remote_path[11..];
            let (pkg, rel) = match after.find('/') {
                Some(i) => (&after[..i], &after[i + 1..]),
                None => return Err(format!("adb pull 失败: {}", err)),
            };
            // 把 run-as cat 的原始字节写入本地临时文件
            let output = Command::new(adb_path()?)
                .args(["-s", serial, "shell", "run-as", pkg, "cat", rel])
                .output()
                .map_err(|e| format!("run-as cat 执行失败: {}", e))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(format!("无法读取文件: {}", stderr));
            }
            // 注意：adb shell 会把输出里的 \n 转成 \r\n，二进制文件（如图片）会被破坏。
            // 文本查看影响不大，这里尽量还原单个 \n。
            let mut fixed = Vec::with_capacity(output.stdout.len());
            let mut i = 0;
            while i < output.stdout.len() {
                let b = output.stdout[i];
                if b == b'\r' && i + 1 < output.stdout.len() && output.stdout[i + 1] == b'\n' {
                    fixed.push(b'\n');
                    i += 2;
                } else {
                    fixed.push(b);
                    i += 1;
                }
            }
            std::fs::write(&temp_file, fixed)
                .map_err(|e| format!("写入临时文件失败: {}", e))?;
        } else {
            return Err(format!("adb pull 失败: {}", err));
        }
    }

    let meta = std::fs::metadata(&temp_file)
        .map_err(|e| format!("读取临时文件元数据失败: {}", e))?;
    let size = meta.len();
    if size > PREVIEW_MAX_BYTES {
        let _ = std::fs::remove_file(&temp_file);
        return Err(format!(
            "文件过大（{:.2} MB），超过 5 MB 上限，请下载后查看",
            size as f64 / (1024.0 * 1024.0)
        ));
    }

    let ext = ext_lower(original_name);

    // 图片
    if let Some(mime) = image_mime(&ext) {
        use base64::{engine::general_purpose, Engine as _};
        let bytes = std::fs::read(&temp_file).map_err(|e| format!("读取文件失败: {}", e))?;
        let encoded = general_purpose::STANDARD.encode(&bytes);
        return Ok(FilePreview {
            kind: "image".into(),
            mime: mime.into(),
            size,
            text: None,
            data_url: Some(format!("data:{};base64,{}", mime, encoded)),
            temp_path: temp_path_str,
        });
    }

    // 文本（白名单或嗅探）
    let bytes = std::fs::read(&temp_file).map_err(|e| format!("读取文件失败: {}", e))?;
    let treat_as_text = is_text_ext(&ext) || (ext.is_empty() && sniff_is_text(&bytes));
    if treat_as_text {
        // 转 UTF-8；非法字节用 replace
        let text = String::from_utf8_lossy(&bytes).to_string();
        return Ok(FilePreview {
            kind: "text".into(),
            mime: "text/plain; charset=utf-8".into(),
            size,
            text: Some(text),
            data_url: None,
            temp_path: temp_path_str,
        });
    }

    // 其它视为二进制
    Ok(FilePreview {
        kind: "binary".into(),
        mime: "application/octet-stream".into(),
        size,
        text: None,
        data_url: None,
        temp_path: temp_path_str,
    })
}

#[tauri::command]
fn cleanup_preview_temp(temp_path: &str) -> Result<(), String> {
    // 只允许删 adbtools-preview 目录下的文件，避免前端参数被篡改误删
    let base = preview_temp_dir();
    let target = std::path::PathBuf::from(temp_path);
    if target.starts_with(&base) && target.exists() {
        std::fs::remove_file(&target).map_err(|e| format!("删除临时文件失败: {}", e))?;
    }
    Ok(())
}

// 把预览临时文件另存到用户选择的位置（给浮层「下载到本地」按钮用）
#[tauri::command]
fn copy_local_file(src: &str, dest: &str) -> Result<(), String> {
    std::fs::copy(src, dest).map_err(|e| format!("复制文件失败: {}", e))?;
    Ok(())
}

// ---------- 应用权限：批量查看/切换 ----------

#[derive(Debug, Serialize, Deserialize)]
pub struct PermissionInfo {
    pub name: String,
    pub granted: bool,
    pub flags: String,
}

#[tauri::command]
fn list_runtime_permissions(
    serial: &str,
    package: &str,
) -> Result<Vec<PermissionInfo>, String> {
    let out = run_adb_command(&["-s", serial, "shell", "dumpsys", "package", package])?;

    // dumpsys package 输出里会有多个 "User 0: ... runtime permissions:" 段，格式：
    //   runtime permissions:
    //     android.permission.CAMERA: granted=true, flags=[ USER_SET ]
    //     com.foo.permission.X: granted=false, flags=[ ... ]
    // 另外 "install permissions:" 段里的普通权限不参与 runtime 授予，跳过。
    use std::collections::HashMap;
    let mut latest: HashMap<String, PermissionInfo> = HashMap::new();
    let mut in_runtime = false;
    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("runtime permissions:") {
            in_runtime = true;
            continue;
        }
        if trimmed.ends_with("permissions:") || trimmed.starts_with("User ") {
            in_runtime = trimmed.starts_with("runtime permissions:")
                || (in_runtime && trimmed.starts_with("User "));
            if !trimmed.starts_with("runtime permissions:") {
                in_runtime = false;
            }
            continue;
        }
        if !in_runtime {
            continue;
        }
        if !trimmed.contains(".permission.") || !trimmed.contains("granted=") {
            continue;
        }
        let Some((name_part, rest)) = trimmed.split_once(':') else {
            continue;
        };
        let name = name_part.trim().to_string();
        let granted = rest.contains("granted=true");
        let flags = rest
            .split("flags=")
            .nth(1)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        latest.insert(
            name.clone(),
            PermissionInfo {
                name,
                granted,
                flags,
            },
        );
    }

    let mut list: Vec<PermissionInfo> = latest.into_values().collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(list)
}

#[tauri::command]
fn set_permission(
    serial: &str,
    package: &str,
    permission: &str,
    granted: bool,
) -> Result<String, String> {
    let action = if granted { "grant" } else { "revoke" };
    run_adb_command(&["-s", serial, "shell", "pm", action, package, permission])
}

// ---------- SQLite 直接查看（本地 rusqlite，避免依赖设备端 sqlite3） ----------

#[derive(Debug, Serialize)]
pub struct SqliteResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

fn is_valid_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

fn sqlite_temp_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("adbtools-sqlite")
}

// 使用 `adb exec-out` 取原始字节（不会被 pty 做 \n → \r\n 转换），写到本地
// 用于 /data/data/<pkg>/ 下的文件（需要 run-as）
fn exec_out_run_as_cat(
    serial: &str,
    package: &str,
    rel: &str,
    local: &std::path::Path,
) -> Result<(), String> {
    let output = Command::new(adb_path()?)
        .args([
            "-s", serial, "exec-out", "run-as", package, "cat", rel,
        ])
        .output()
        .map_err(|e| format!("exec-out run-as cat 执行失败: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "exec-out run-as cat 失败: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    std::fs::write(local, &output.stdout).map_err(|e| format!("写入本地失败: {}", e))
}

// 把 `/data/data/<pkg>/<rel>` 及其可选的 -wal / -shm 文件拉到本地。
// 返回本地主 db 文件路径
fn pull_db_to_local(
    serial: &str,
    package: &str,
    rel: &str,
    key: &str,
) -> Result<std::path::PathBuf, String> {
    let dir = sqlite_temp_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建临时目录失败: {}", e))?;
    // 每次都重新拉，保证拿到最新数据
    let file_name = std::path::Path::new(rel)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("db");
    let safe_key: String = key
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let local_db = dir.join(format!("{}_{}", safe_key, file_name));

    exec_out_run_as_cat(serial, package, rel, &local_db)?;

    // -wal / -shm 尽力而为，不存在时静默忽略
    for suffix in ["-wal", "-shm"] {
        let remote = format!("{}{}", rel, suffix);
        let local_extra = dir.join(format!("{}_{}{}", safe_key, file_name, suffix));
        let _ = exec_out_run_as_cat(serial, package, &remote, &local_extra);
    }

    Ok(local_db)
}

fn row_cell_to_string(v: rusqlite::types::ValueRef<'_>) -> String {
    use rusqlite::types::ValueRef;
    match v {
        ValueRef::Null => String::new(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(bytes) => String::from_utf8_lossy(bytes).to_string(),
        ValueRef::Blob(bytes) => format!("<blob {} bytes>", bytes.len()),
    }
}

#[tauri::command]
fn sqlite_list_tables(
    serial: &str,
    package: &str,
    db_path: &str,
) -> Result<Vec<String>, String> {
    let key = format!("list-{}-{}-{}", serial, package, db_path);
    let local = pull_db_to_local(serial, package, db_path, &key)?;
    let conn = rusqlite::Connection::open_with_flags(
        &local,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("打开数据库失败: {}", e))?;
    let mut stmt = conn
        .prepare(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .map_err(|e| format!("准备语句失败: {}", e))?;
    let tables: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| format!("查询失败: {}", e))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(tables)
}

#[tauri::command]
fn sqlite_query_table(
    serial: &str,
    package: &str,
    db_path: &str,
    table: &str,
    limit: u32,
    offset: u32,
    search: Option<String>,
) -> Result<SqliteResult, String> {
    if !is_valid_ident(table) {
        return Err(format!("非法表名: {}", table));
    }
    let limit = limit.clamp(1, 1000);
    let key = format!("query-{}-{}-{}", serial, package, db_path);
    let local = pull_db_to_local(serial, package, db_path, &key)?;
    let conn = rusqlite::Connection::open_with_flags(
        &local,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("打开数据库失败: {}", e))?;

    // 预取一下列名，顺便拿到首列（用作搜索目标列）
    let first_col: Option<String> = {
        let stmt = conn
            .prepare(&format!("SELECT * FROM \"{}\" LIMIT 0", table))
            .map_err(|e| format!("获取列名失败: {}", e))?;
        stmt.column_names().first().map(|s| s.to_string())
    };

    let search_q = search.and_then(|s| {
        let t = s.trim().to_string();
        if t.is_empty() { None } else { Some(t) }
    });

    let (sql, pattern) = match (&search_q, &first_col) {
        (Some(q), Some(col)) => (
            format!(
                "SELECT * FROM \"{}\" WHERE CAST(\"{}\" AS TEXT) LIKE ?1 LIMIT {} OFFSET {}",
                table, col, limit, offset
            ),
            Some(format!("%{}%", q)),
        ),
        _ => (
            format!(
                "SELECT * FROM \"{}\" LIMIT {} OFFSET {}",
                table, limit, offset
            ),
            None,
        ),
    };

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("准备查询失败: {}", e))?;
    let columns: Vec<String> = stmt
        .column_names()
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    let col_count = columns.len();
    let mut rows_iter = match &pattern {
        Some(p) => stmt
            .query([p.as_str()])
            .map_err(|e| format!("执行查询失败: {}", e))?,
        None => stmt.query([]).map_err(|e| format!("执行查询失败: {}", e))?,
    };
    let mut rows: Vec<Vec<String>> = Vec::new();
    while let Some(row) = rows_iter
        .next()
        .map_err(|e| format!("读取行失败: {}", e))?
    {
        let mut out_row = Vec::with_capacity(col_count);
        for i in 0..col_count {
            let v = row
                .get_ref(i)
                .map_err(|e| format!("读取列失败: {}", e))?;
            out_row.push(row_cell_to_string(v));
        }
        rows.push(out_row);
    }
    Ok(SqliteResult { columns, rows })
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
fn start_screen_record(serial: String, state: State<'_, RecordingState>) -> Result<String, String> {
    let mut guard = state.0.lock().map_err(|e| e.to_string())?;

    // 若已有会话，检查子进程是否仍存活：已退出则视为残留，直接回收
    if let Some(session) = guard.as_mut() {
        match session.child.try_wait() {
            Ok(Some(_)) | Err(_) => {
                guard.take();
            }
            Ok(None) => {
                return Err("已有录屏任务正在进行".to_string());
            }
        }
    }

    // 兜底：把设备端可能残留的 screenrecord 先干掉（例如上次进程崩溃留下的孤儿）
    let _ = run_adb_command(&[
        "-s", &serial, "shell", "pkill", "-SIGINT", "screenrecord",
    ]);
    thread::sleep(Duration::from_millis(200));

    // 使用时间戳生成唯一文件名，避免多次录制冲突
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let remote_path = format!("/sdcard/screenrecord_{}.mp4", ts);

    eprintln!(
        "ADB command (spawn): adb -s {} shell screenrecord {}",
        serial, remote_path
    );

    // 以子进程方式启动 screenrecord（该命令会阻塞直到录制结束或收到信号）
    let child = Command::new(adb_path()?)
        .args(&["-s", &serial, "shell", "screenrecord", &remote_path])
        .spawn()
        .map_err(|e| format!("启动录屏失败: {}", e))?;

    *guard = Some(RecordingSession {
        child,
        serial,
        remote_path: remote_path.clone(),
    });

    Ok(remote_path)
}

// 通过 `pidof screenrecord` 查询设备端是否真的还在录屏
#[tauri::command]
fn is_screen_recording(serial: &str) -> Result<bool, String> {
    match run_adb_command(&["-s", serial, "shell", "pidof", "screenrecord"]) {
        Ok(s) => Ok(!s.trim().is_empty()),
        // pidof 找不到进程时返回非零退出码，run_adb_command 视为 Err
        Err(_) => Ok(false),
    }
}

#[tauri::command]
fn cancel_screen_record(state: State<'_, RecordingState>) -> Result<(), String> {
    let session = {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        guard.take()
    };

    if let Some(mut s) = session {
        // 优雅终止设备端 screenrecord
        let _ = run_adb_command(&[
            "-s", &s.serial, "shell", "pkill", "-SIGINT", "screenrecord",
        ]);
        // 本地 adb 子进程也强杀
        let _ = s.child.kill();
        let _ = s.child.wait();
        // 清理设备上可能已生成的临时文件
        let _ = run_adb_command(&["-s", &s.serial, "shell", "rm", "-f", &s.remote_path]);
    }

    Ok(())
}

#[tauri::command]
fn stop_screen_record(save_path: String, state: State<'_, RecordingState>) -> Result<String, String> {
    let session = {
        let mut guard = state.0.lock().map_err(|e| e.to_string())?;
        guard.take().ok_or_else(|| "当前没有录屏任务".to_string())?
    };

    let RecordingSession {
        mut child,
        serial,
        remote_path,
    } = session;

    // 通过 adb 给设备上的 screenrecord 进程发送 SIGINT，让其优雅结束并写完文件头尾
    let _ = run_adb_command(&[
        "-s", &serial, "shell", "pkill", "-SIGINT", "screenrecord",
    ]);

    // 等待本地 adb 子进程退出（最多等 5 秒，超时则强杀）
    // 子进程退出即意味着设备端 screenrecord 已完成 MP4 尾部写入，文件可安全拉取
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
                thread::sleep(Duration::from_millis(30));
            }
            Err(_) => break,
        }
    }

    // 拉取到本地
    run_adb_command(&["-s", &serial, "pull", &remote_path, &save_path])?;

    // 清理远端文件
    let _ = run_adb_command(&["-s", &serial, "shell", "rm", &remote_path]);

    Ok("Screen recording saved successfully".to_string())
}

// Firebase Analytics 埋点调试：先清空 → 再设置目标包名
#[tauri::command]
fn enable_analytics_debug(serial: &str, package: &str) -> Result<String, String> {
    run_adb_command(&[
        "-s", serial, "shell", "setprop", "debug.firebase.analytics.app", ".none.",
    ])?;
    run_adb_command(&[
        "-s", serial, "shell", "setprop", "debug.firebase.analytics.app", package,
    ])?;
    Ok(format!("已开启 Firebase 埋点调试：{}", package))
}

// 导出设备日志到本地文件（adb logcat -d 获取全部缓冲区快照）
// buffers: 可选，形如 "main,system,crash"，默认使用 "all"
// package: 可选包名，若指定则通过 pidof 取 PID，再按 threadtime 格式过滤 PID 列
#[tauri::command]
fn export_logcat(
    serial: &str,
    save_path: &str,
    buffers: Option<&str>,
    package: Option<&str>,
) -> Result<String, String> {
    let buffer_arg = buffers.unwrap_or("all");
    let filter_pkg = package.map(|s| s.trim()).filter(|s| !s.is_empty());

    // 按包名过滤时：通过 `pidof` 拿到所有进程 PID（多进程应用可能有多个）
    let pids: Vec<String> = if let Some(pkg) = filter_pkg {
        let out = run_adb_command(&["-s", serial, "shell", "pidof", pkg]).unwrap_or_default();
        out.split_whitespace().map(|s| s.to_string()).collect()
    } else {
        Vec::new()
    };

    if let Some(pkg) = filter_pkg {
        if pids.is_empty() {
            return Err(format!(
                "未找到应用 {} 的进程，请先在设备上启动该应用后再导出",
                pkg
            ));
        }
    }

    eprintln!(
        "ADB command: adb -s {} logcat -d -b {} -v threadtime (filter pkg={:?}, pids={:?})",
        serial, buffer_arg, filter_pkg, pids
    );
    let output = Command::new(adb_path()?)
        .args(["-s", serial, "logcat", "-d", "-b", buffer_arg, "-v", "threadtime"])
        .output()
        .map_err(|e| format!("执行 adb logcat 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("导出日志失败: {}", stderr));
    }

    // threadtime 格式： "MM-DD HH:MM:SS.mmm  PID  TID LEVEL TAG: MSG"
    // 按空白分割后，索引 2 是 PID
    let bytes: Vec<u8> = if pids.is_empty() {
        output.stdout
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let pid_set: std::collections::HashSet<&str> =
            pids.iter().map(|s| s.as_str()).collect();
        let mut filtered = String::new();
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && pid_set.contains(parts[2]) {
                filtered.push_str(line);
                filtered.push('\n');
            }
        }
        filtered.into_bytes()
    };

    std::fs::write(save_path, &bytes).map_err(|e| format!("写入日志文件失败: {}", e))?;

    Ok(format!("日志已保存（{} 字节）", bytes.len()))
}

#[tauri::command]
fn reboot_device(serial: &str, mode: Option<&str>) -> Result<String, String> {
    match mode {
        Some("recovery") => run_adb_command(&["-s", serial, "reboot", "recovery"]),
        Some("bootloader") => run_adb_command(&["-s", serial, "reboot", "bootloader"]),
        _ => run_adb_command(&["-s", serial, "reboot"]),
    }
}

// 将文本输入到设备当前焦点的输入框（封装 `adb shell input text`）
#[tauri::command]
fn input_text(serial: &str, text: &str) -> Result<String, String> {
    if text.is_empty() {
        return Err("文本不能为空".to_string());
    }
    // `input text` 仅支持 ASCII；非 ASCII（含中文）需要 ADBKeyBoard 等第三方输入法支持
    if !text.is_ascii() {
        return Err(
            "adb shell input text 不支持中文/非 ASCII 字符。\n\
             如需输入中文，请在设备安装 ADBKeyBoard 并切换为默认输入法。"
                .to_string(),
        );
    }

    // 空格需要转成 %s；shell 元字符需转义，否则会被设备端 shell 解释
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            ' ' => escaped.push_str("%s"),
            '"' | '\'' | '`' | '$' | '\\' | '&' | ';' | '|' | '*' | '<' | '>' | '?' | '#'
            | '(' | ')' | '!' | '~' => {
                escaped.push('\\');
                escaped.push(c);
            }
            _ => escaped.push(c),
        }
    }

    run_adb_command(&["-s", serial, "shell", "input", "text", &escaped])?;
    Ok("文本已发送".to_string())
}

#[tauri::command]
fn run_shell_command(serial: &str, command: &str) -> Result<String, String> {
    let args: Vec<&str> = command.split_whitespace().collect();
    let mut cmd_args = vec!["-s", serial, "shell"];
    cmd_args.extend(args);
    run_adb_command(&cmd_args)
}

// 返回 ~/Documents/adbtools，若不存在则创建
#[tauri::command]
fn ensure_default_save_dir() -> Result<String, String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "无法获取用户主目录".to_string())?;
    let dir = std::path::Path::new(&home)
        .join("Documents")
        .join("adbtools");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {}", e))?;
    Ok(dir.to_string_lossy().to_string())
}

// 在系统文件管理器中打开并高亮指定文件
#[tauri::command]
fn reveal_in_folder(path: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", path])
            .spawn()
            .map_err(|e| format!("打开 Finder 失败: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        // /select, 后面直接跟路径（不加引号，Command 会处理转义）
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path))
            .spawn()
            .map_err(|e| format!("打开 Explorer 失败: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        // Linux 绝大多数文件管理器不支持 reveal，退回到打开父目录
        let parent = std::path::Path::new(path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".to_string());
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|e| format!("打开文件管理器失败: {}", e))?;
    }

    Ok(())
}

// 使用系统默认方式打开 URL（macOS open / Windows start / Linux xdg-open）
fn open_url_native(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", url])
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {}", e))?;
    }

    Ok(())
}

#[tauri::command]
fn open_url(url: &str) -> Result<(), String> {
    open_url_native(url)
}

// 后端代理下载图片，返回 data URL（用于绕开 webview 请求头导致的 429 等问题）
#[tauri::command]
fn fetch_image_as_data_url(url: String) -> Result<String, String> {
    use base64::{engine::general_purpose, Engine as _};

    let response = ureq::get(&url)
        .set("User-Agent", "Mozilla/5.0")
        .call()
        .map_err(|e| format!("下载图片失败: {}", e))?;

    let content_type = response
        .header("Content-Type")
        .unwrap_or("image/jpeg")
        .to_string();

    let mut bytes: Vec<u8> = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("读取图片失败: {}", e))?;

    let encoded = general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{};base64,{}", content_type, encoded))
}

// 启动本地回环 HTTP 服务器接收 Google OAuth 回调
// 因为 Google 会把 token 放在 URL fragment (#...) 里，服务器收不到，
// 所以先返回一段 HTML，让浏览器把 fragment 作为 query 重新请求 /callback
#[tauri::command]
fn start_oauth_server(app: AppHandle, state: State<'_, OAuthState>) -> Result<u16, String> {
    // 关闭之前可能残留的 server
    {
        let mut guard = state.0.lock().unwrap();
        if let Some(tx) = guard.take() {
            let _ = tx.send(());
        }
    }
    // 等旧 socket 释放
    thread::sleep(Duration::from_millis(300));

    let listener = TcpListener::bind("127.0.0.1:8765")
        .map_err(|e| format!("端口 8765 占用，请关闭占用程序后重试: {}", e))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("设置非阻塞失败: {}", e))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    let (tx, rx) = channel::<()>();
    *state.0.lock().unwrap() = Some(tx);

    thread::spawn(move || {
        loop {
            // 收到关闭信号立即退出
            match rx.try_recv() {
                Ok(_) | Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }

            let (mut stream, _) = match listener.accept() {
                Ok(s) => s,
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
                Err(_) => break,
            };
            let _ = stream.set_nonblocking(false);

            let mut request_line = String::new();
            {
                let mut reader = BufReader::new(&stream);
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
            }

            let path = request_line
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .to_string();

            if path.starts_with("/callback") {
                let query = path
                    .split_once('?')
                    .map(|(_, q)| q.to_string())
                    .unwrap_or_default();

                let _ = app.emit("oauth_callback", query);

                let body = "<html><head><meta charset=\"utf-8\"></head><body style=\"font-family:sans-serif;text-align:center;padding-top:80px\"><h2>认证完成</h2><p>可以关闭此页面返回应用</p></body></html>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(), body
                );
                let _ = stream.write_all(response.as_bytes());
                break; // 处理完回调后关闭 server
            } else {
                // 根路径：返回 HTML 把 fragment 转成 query 再请求一次
                let body = "<html><body><script>var h=window.location.hash.slice(1);window.location.replace('/callback?'+h);</script></body></html>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(), body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        }
    });

    Ok(port)
}

// 通过 dumpsys window 的 mCurrentFocus 行提取当前最顶层 Activity（package/ComponentName）
#[tauri::command]
fn get_top_activity(serial: &str) -> Result<String, String> {
    let out = run_adb_command(&["-s", serial, "shell", "dumpsys", "window"])?;
    for line in out.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("mCurrentFocus=") {
            // 形如：mCurrentFocus=Window{hash u0 pkg/.Activity}
            // 取最后一个 '{' 与 '}' 之间的内容，再取最后一个空格后的 token
            if let (Some(lb), Some(rb)) = (trimmed.rfind('{'), trimmed.rfind('}')) {
                if rb > lb {
                    let inner = &trimmed[lb + 1..rb];
                    if let Some(token) = inner.split_whitespace().last() {
                        return Ok(token.to_string());
                    }
                }
            }
            return Ok(trimmed.to_string());
        }
    }
    Err("未能在 dumpsys window 中解析 mCurrentFocus".to_string())
}

// ------------------ 快捷开关命令 ------------------

// 将 "1"/"true" 视为 true，其它内容或命令失败时返回 false（不抛错）
fn read_bool_setting(args: &[&str]) -> bool {
    match run_adb_command(args) {
        Ok(out) => {
            let v = out.trim().to_ascii_lowercase();
            v == "1" || v == "true"
        }
        Err(_) => false,
    }
}

#[tauri::command]
fn get_wifi_enabled(serial: &str) -> Result<bool, String> {
    Ok(read_bool_setting(&[
        "-s", serial, "shell", "settings", "get", "global", "wifi_on",
    ]))
}

#[tauri::command]
fn set_wifi_enabled(serial: &str, enabled: bool) -> Result<String, String> {
    let action = if enabled { "enable" } else { "disable" };
    run_adb_command(&["-s", serial, "shell", "svc", "wifi", action])
}

#[tauri::command]
fn open_dev_options(serial: &str) -> Result<String, String> {
    run_adb_command(&[
        "-s", serial, "shell", "am", "start", "-a",
        "android.settings.APPLICATION_DEVELOPMENT_SETTINGS",
    ])
}

#[tauri::command]
fn open_language_settings(serial: &str) -> Result<String, String> {
    run_adb_command(&[
        "-s", serial, "shell", "am", "start", "-a",
        "android.settings.LOCALE_SETTINGS",
    ])
}

#[tauri::command]
fn open_date_settings(serial: &str) -> Result<String, String> {
    run_adb_command(&[
        "-s", serial, "shell", "am", "start", "-a",
        "android.settings.DATE_SETTINGS",
    ])
}

#[tauri::command]
fn get_show_layout_bounds(serial: &str) -> Result<bool, String> {
    Ok(read_bool_setting(&[
        "-s", serial, "shell", "getprop", "debug.layout",
    ]))
}

#[tauri::command]
fn set_show_layout_bounds(serial: &str, enabled: bool) -> Result<String, String> {
    let val = if enabled { "true" } else { "false" };
    run_adb_command(&["-s", serial, "shell", "setprop", "debug.layout", val])?;
    // service call activity 1599295570 让 WindowManager 立即重绘以生效
    let _ = run_adb_command(&[
        "-s", serial, "shell", "service", "call", "activity", "1599295570",
    ]);
    Ok("ok".to_string())
}

#[tauri::command]
fn get_show_touches(serial: &str) -> Result<bool, String> {
    Ok(read_bool_setting(&[
        "-s", serial, "shell", "settings", "get", "system", "show_touches",
    ]))
}

#[tauri::command]
fn set_show_touches(serial: &str, enabled: bool) -> Result<String, String> {
    let val = if enabled { "1" } else { "0" };
    run_adb_command(&[
        "-s", serial, "shell", "settings", "put", "system", "show_touches", val,
    ])
}

#[tauri::command]
fn get_pointer_location(serial: &str) -> Result<bool, String> {
    Ok(read_bool_setting(&[
        "-s", serial, "shell", "settings", "get", "system", "pointer_location",
    ]))
}

#[tauri::command]
fn set_pointer_location(serial: &str, enabled: bool) -> Result<String, String> {
    let val = if enabled { "1" } else { "0" };
    run_adb_command(&[
        "-s", serial, "shell", "settings", "put", "system", "pointer_location", val,
    ])
}

#[tauri::command]
fn get_always_finish_activities(serial: &str) -> Result<bool, String> {
    Ok(read_bool_setting(&[
        "-s", serial, "shell", "settings", "get", "global", "always_finish_activities",
    ]))
}

#[tauri::command]
fn set_always_finish_activities(serial: &str, enabled: bool) -> Result<String, String> {
    let val = if enabled { "1" } else { "0" };
    run_adb_command(&[
        "-s", serial, "shell", "settings", "put", "global", "always_finish_activities", val,
    ])
}

// 暗黑模式：通过 `cmd uimode night yes/no` 切换，读取 `cmd uimode night` 输出形如 "Night mode: yes"
#[tauri::command]
fn get_dark_mode(serial: &str) -> Result<bool, String> {
    match run_adb_command(&["-s", serial, "shell", "cmd", "uimode", "night"]) {
        Ok(out) => {
            let low = out.to_ascii_lowercase();
            // 明确命中 "yes" 才返回 true；auto/no/unknown 都视为 false
            Ok(low.contains("yes"))
        }
        Err(_) => Ok(false),
    }
}

#[tauri::command]
fn set_dark_mode(serial: &str, enabled: bool) -> Result<String, String> {
    let val = if enabled { "yes" } else { "no" };
    run_adb_command(&["-s", serial, "shell", "cmd", "uimode", "night", val])
}

// 导航模式：gestural / threebutton / twobutton
// 读取 `settings get secure navigation_mode`：0=三按键，1=两按键，2=手势
#[tauri::command]
fn get_navigation_mode(serial: &str) -> Result<String, String> {
    let out = run_adb_command(&[
        "-s", serial, "shell", "settings", "get", "secure", "navigation_mode",
    ])
    .unwrap_or_default();
    let mode = match out.trim() {
        "2" => "gestural",
        "1" => "twobutton",
        "0" => "threebutton",
        _ => "unknown",
    };
    Ok(mode.to_string())
}

#[tauri::command]
fn set_navigation_mode(serial: &str, mode: &str) -> Result<String, String> {
    let pkg = match mode {
        "gestural" => "com.android.internal.systemui.navbar.gestural",
        "threebutton" => "com.android.internal.systemui.navbar.threebutton",
        "twobutton" => "com.android.internal.systemui.navbar.twobutton",
        other => return Err(format!("未知的导航模式: {}", other)),
    };
    run_adb_command(&["-s", serial, "shell", "cmd", "overlay", "enable-exclusive", pkg])
}

const ADB_DOWNLOAD_URL: &str =
    "https://developer.android.com/tools/releases/platform-tools?hl=zh-cn";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::menu::{Menu, MenuItem, MenuItemKind};

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(OAuthState(Mutex::new(None)))
        .manage(RecordingState(Mutex::new(None)))
        .setup(|app| {
            use tauri::menu::{Menu as TrayMenu, MenuItem as TrayMenuItem};
            use tauri::tray::{
                MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
            };

            let show_item =
                TrayMenuItem::with_id(app, "tray_show", "显示窗口", true, None::<&str>)?;
            let hide_item =
                TrayMenuItem::with_id(app, "tray_hide", "隐藏窗口", true, None::<&str>)?;
            let quit_item =
                TrayMenuItem::with_id(app, "tray_quit", "退出", true, None::<&str>)?;
            let tray_menu =
                TrayMenu::with_items(app, &[&show_item, &hide_item, &quit_item])?;

            let _tray = TrayIconBuilder::with_id("main")
                .icon(tauri::include_image!("icons/tray.png"))
                .icon_as_template(true)
                .tooltip("ADB Tools")
                .menu(&tray_menu)
                // 左键点击图标 → 切换显示/隐藏窗口；右键弹出菜单
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "tray_show" => {
                        for (_, w) in app.webview_windows() {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "tray_hide" => {
                        for (_, w) in app.webview_windows() {
                            let _ = w.hide();
                        }
                    }
                    "tray_quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        for (_, w) in app.webview_windows() {
                            // 已显示就隐藏，已隐藏就显示并 focus
                            match w.is_visible() {
                                Ok(true) => {
                                    let _ = w.hide();
                                }
                                _ => {
                                    let _ = w.show();
                                    let _ = w.set_focus();
                                }
                            }
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .menu(|handle| {
            // 在系统默认菜单的 Help 子菜单里追加"下载 ADB"；
            // 同时把 dev 模式下 macOS App 菜单里继承自 bin 名的 "AdbTools" 强制改成 "ADB Tools"
            let menu = Menu::default(handle)?;
            let download_item = MenuItem::with_id(
                handle,
                "download_adb",
                "未安装 ADB?",
                true,
                None::<&str>,
            )?;

            const APP_NAME: &str = "ADB Tools";

            let mut is_first_submenu = true;
            for item in menu.items()? {
                if let MenuItemKind::Submenu(sm) = item {
                    // 第一个 submenu 就是 macOS 的 App 菜单
                    if is_first_submenu {
                        is_first_submenu = false;
                        let _ = sm.set_text(APP_NAME);
                        // 把里面形如 "About X / Hide X / Quit X" 的项显式改名
                        if let Ok(children) = sm.items() {
                            for child in children {
                                if let MenuItemKind::Predefined(p) = child {
                                    if let Ok(t) = p.text() {
                                        let new_text = if t.starts_with("About ") {
                                            Some(format!("About {}", APP_NAME))
                                        } else if t.starts_with("Hide ") && !t.starts_with("Hide Others") {
                                            Some(format!("Hide {}", APP_NAME))
                                        } else if t.starts_with("Quit ") {
                                            Some(format!("Quit {}", APP_NAME))
                                        } else {
                                            None
                                        };
                                        if let Some(nt) = new_text {
                                            let _ = p.set_text(&nt);
                                        }
                                    }
                                }
                            }
                        }
                        continue;
                    }
                    if let Ok(text) = sm.text() {
                        if text == "Help" || text == "帮助" {
                            sm.append(&download_item)?;
                        }
                    }
                }
            }

            Ok(menu)
        })
        .on_menu_event(|_app, event| {
            if event.id() == "download_adb" {
                if let Err(e) = open_url_native(ADB_DOWNLOAD_URL) {
                    eprintln!("打开下载页失败: {}", e);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_devices,
            get_device_info,
            connect_device,
            disconnect_device,
            install_apk,
            get_apk_package_name,
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
            export_logcat,
            enable_analytics_debug,
            start_screen_record,
            stop_screen_record,
            cancel_screen_record,
            is_screen_recording,
            reboot_device,
            run_shell_command,
            input_text,
            open_url,
            reveal_in_folder,
            ensure_default_save_dir,
            start_oauth_server,
            fetch_image_as_data_url,
            get_wifi_enabled,
            set_wifi_enabled,
            open_dev_options,
            open_language_settings,
            open_date_settings,
            get_show_layout_bounds,
            set_show_layout_bounds,
            get_show_touches,
            set_show_touches,
            get_pointer_location,
            set_pointer_location,
            get_always_finish_activities,
            set_always_finish_activities,
            get_dark_mode,
            set_dark_mode,
            get_navigation_mode,
            set_navigation_mode,
            get_top_activity,
            preview_remote_file,
            cleanup_preview_temp,
            copy_local_file,
            sqlite_list_tables,
            sqlite_query_table,
            list_runtime_permissions,
            set_permission,
        ])
        // 关闭按钮 → 隐藏窗口而不是退出（macOS 习惯：App 留在 Dock，点击图标恢复）
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                #[cfg(target_os = "macos")]
                {
                    let _ = window.hide();
                    api.prevent_close();
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let _ = (window, api);
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        // 主事件循环：拦截 Dock 图标点击（macOS Reopen），把已隐藏窗口重新唤出
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { has_visible_windows, .. } = event {
                if !has_visible_windows {
                    for (_, window) in app.webview_windows() {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}
