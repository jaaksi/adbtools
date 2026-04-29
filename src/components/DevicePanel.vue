<script setup lang="ts">
import { ref, watch, onMounted, onBeforeUnmount } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { ElMessage, ElMessageBox, ElNotification } from "element-plus";
import { Plus, Refresh, UploadFilled } from "@element-plus/icons-vue";

interface Device {
  serial: string;
  status: string;
  model?: string;
  device_type?: string;
}

interface DeviceInfo {
  serial: string;
  model: string;
  manufacturer: string;
  android_version: string;
  sdk_version: string;
  screen_resolution: string;
  density: string;
  density_dpi: string;
  smallest_width: string;
}

const props = defineProps<{
  devices: Device[];
  selectedDevice: string;
}>();

const emit = defineEmits<{ refresh: [] }>();

const deviceInfo = ref<DeviceInfo | null>(null);
const loading = ref(false);
const connectDialogVisible = ref(false);
const ipAddress = ref("");
const port = ref(5555);

// 拖拽安装 APK
const dragActive = ref(false);
const installing = ref(false);
const installProgress = ref({ current: 0, total: 0, file: "" });
let dragDropUnlisten: (() => void) | null = null;

// 处理版本降级：解析包名 → 询问用户 → 卸载重装
// 返回 "installed" | "cancelled" | "failed"
async function handleVersionDowngrade(
  apkPath: string,
  fileName: string
): Promise<"installed" | "cancelled" | "failed"> {
  // 尝试解析 APK 包名
  let packageName = "";
  try {
    packageName = await invoke<string>("get_apk_package_name", { apkPath });
  } catch (e) {
    console.warn("解析 APK 包名失败:", e);
  }

  const pkgHint = packageName ? `（${packageName}）` : "";

  // 弹确认框前先关闭遮罩，否则 z-index 会把 MessageBox 整个盖住，
  // 用户看不到也点不到，流程会永远卡在这里
  const wasInstalling = installing.value;
  installing.value = false;

  let confirmed = false;
  try {
    await ElMessageBox.confirm(
      `设备上已安装更高版本的 ${fileName}${pkgHint}。\n\n继续需要先卸载该应用，此操作会清空应用数据，是否继续？`,
      "检测到版本降级",
      {
        type: "warning",
        confirmButtonText: "卸载并安装",
        cancelButtonText: "取消",
        distinguishCancelAndClose: true,
        // MessageBox 即将关闭时立刻恢复安装遮罩，避免确认框消失到 loading 出现之间的空窗期
        beforeClose: (_action, _instance, done) => {
          installing.value = wasInstalling;
          done();
        },
      }
    );
    confirmed = true;
  } catch {
    // 用户点取消/关闭/ESC（installing 已在 beforeClose 中恢复）
  }

  if (!confirmed) return "cancelled";

  if (!packageName) {
    ElMessage.error(
      "无法自动卸载：未能解析 APK 包名（aapt 不可用）。请手动卸载后重试。"
    );
    return "failed";
  }

  try {
    await invoke("uninstall_app", {
      serial: props.selectedDevice,
      packageName,
    });
    await invoke("install_apk", {
      serial: props.selectedDevice,
      apkPath,
    });
    return "installed";
  } catch (e) {
    console.error("卸载重装失败:", e);
    return "failed";
  }
}

async function installApkFiles(paths: string[]) {
  if (!props.selectedDevice) {
    ElMessage.warning("请先选择设备");
    return;
  }

  // 只处理 .apk 文件
  const apkFiles = paths.filter((p) => p.toLowerCase().endsWith(".apk"));
  if (apkFiles.length === 0) {
    ElMessage.warning("请拖入 .apk 文件");
    return;
  }

  // 真正要调用 adb install 时才点亮 loading
  installing.value = true;
  installProgress.value = { current: 0, total: apkFiles.length, file: "" };

  // 等两帧，确保 loading 遮罩已经被 WebKit 真正 paint 到屏幕上，
  // 再开始同步派发 adb 请求（Tauri IPC 很快，不 wait 会让遮罩显示滞后）
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve()))
  );

  let successCount = 0;
  const failed: string[] = [];

  try {
    for (let i = 0; i < apkFiles.length; i++) {
      const apkPath = apkFiles[i];
      const fileName = apkPath.split(/[/\\]/).pop() || apkPath;
      installProgress.value.current = i + 1;
      installProgress.value.file = fileName;

      try {
        await invoke("install_apk", {
          serial: props.selectedDevice,
          apkPath,
        });
        successCount++;
      } catch (error) {
        const msg = String(error);
        // 版本降级：已安装的版本高于待安装的，提示用户确认卸载后重装
        if (msg.includes("INSTALL_FAILED_VERSION_DOWNGRADE")) {
          const handled = await handleVersionDowngrade(apkPath, fileName);
          if (handled === "installed") {
            successCount++;
            continue;
          }
          if (handled === "cancelled") {
            failed.push(`${fileName}: 已取消（检测到版本降级）`);
            continue;
          }
          failed.push(`${fileName}: 卸载重装失败`);
          continue;
        }
        failed.push(`${fileName}: ${msg}`);
      }
    }
  } finally {
    // 无论成功/失败/异常，都要关闭 loading
    installing.value = false;
  }

  if (failed.length === 0) {
    ElNotification.success({
      title: "安装完成",
      message: `成功安装 ${successCount} 个 APK`,
    });
  } else {
    ElNotification.warning({
      title: `安装完成（${successCount} 成功 / ${failed.length} 失败）`,
      message: failed.join("\n"),
      duration: 0,
    });
  }
}

async function loadDeviceInfo() {
  if (!props.selectedDevice) return;
  loading.value = true;
  try {
    deviceInfo.value = await invoke("get_device_info", {
      serial: props.selectedDevice,
    });
  } catch (error) {
    console.error("Failed to get device info:", error);
    ElMessage.error("获取设备信息失败");
  } finally {
    loading.value = false;
  }
}

async function connectDevice() {
  if (!ipAddress.value) {
    ElMessage.warning("请输入IP地址");
    return;
  }
  try {
    const result = await invoke("connect_device", {
      ip: ipAddress.value,
      port: port.value,
    });
    ElMessage.success(result as string);
    connectDialogVisible.value = false;
    emit("refresh");
  } catch (error) {
    ElMessage.error(`连接失败: ${error}`);
  }
}

watch(() => props.selectedDevice, loadDeviceInfo, { immediate: true });

onMounted(async () => {
  // 监听 Tauri 窗口原生拖拽事件
  dragDropUnlisten = await getCurrentWebview().onDragDropEvent((event) => {
    const t = event.payload.type;
    if (t === "enter") {
      // 安装过程中忽略新的拖入事件
      if (installing.value) return;
      dragActive.value = true;
    } else if (t === "leave") {
      dragActive.value = false;
    } else if (t === "drop") {
      // 先关掉拖拽提示，loading 由 installApkFiles 在真正执行 adb 前点亮
      dragActive.value = false;
      installApkFiles(event.payload.paths);
    }
    // 注意：有意忽略 "over" 事件 —— 松手瞬间 Tauri 可能在 drop 之后紧跟一个 over，
    //       若响应它会把 dragActive 重新点亮，让用户感觉"松手后还显示拖拽提示"。
  });
});

onBeforeUnmount(() => {
  if (dragDropUnlisten) dragDropUnlisten();
});
</script>

<template>
  <div class="device-panel">
    <div class="panel-header">
      <h2>设备管理</h2>
      <div class="header-actions">
        <el-button type="primary" :icon="Plus" @click="connectDialogVisible = true">
          连接设备
        </el-button>
        <el-button :icon="Refresh" @click="$emit('refresh')">刷新</el-button>
      </div>
    </div>

    <div v-if="!selectedDevice" class="empty-state">
      <el-empty description="请选择或连接一个设备" />
    </div>

    <div v-else-if="deviceInfo" v-loading="loading" class="device-content">
      <el-card class="info-card">
        <template #header>
          <div class="card-header">
            <span>设备信息</span>
            <el-tag type="success">已连接</el-tag>
          </div>
        </template>
        <el-descriptions :column="2" border>
          <el-descriptions-item label="设备型号">
            {{ deviceInfo.model }}
          </el-descriptions-item>
          <el-descriptions-item label="制造商">
            {{ deviceInfo.manufacturer }}
          </el-descriptions-item>
          <el-descriptions-item label="Android 版本">
            {{ deviceInfo.android_version }}
          </el-descriptions-item>
          <el-descriptions-item label="SDK 版本">
            {{ deviceInfo.sdk_version }}
          </el-descriptions-item>
          <el-descriptions-item label="屏幕密度">
            {{ deviceInfo.density || "-" }}
          </el-descriptions-item>
          <el-descriptions-item label="Smallest width dp">
            {{ deviceInfo.smallest_width || "-" }}
          </el-descriptions-item>
          <el-descriptions-item label="densityDpi">
            {{ deviceInfo.density_dpi || "-" }}
          </el-descriptions-item>
          <el-descriptions-item label="屏幕分辨率">
            {{ deviceInfo.screen_resolution }}
          </el-descriptions-item>
        </el-descriptions>
      </el-card>

      <!-- 拖拽安装 APK 提示区 -->
      <el-card class="dropzone-card" shadow="never">
        <div class="dropzone-hint" v-loading="installing">
          <el-icon class="dropzone-icon"><UploadFilled /></el-icon>
          <div class="dropzone-text">
            <p class="dropzone-title">拖拽 APK 文件到此处安装</p>
            <p class="dropzone-desc">
              支持同时拖入多个 .apk 文件，将依次安装到当前设备
            </p>
            <p v-if="installing" class="dropzone-progress">
              正在安装 ({{ installProgress.current }}/{{ installProgress.total }})：{{ installProgress.file }}
            </p>
          </div>
        </div>
      </el-card>
    </div>

    <!-- 拖拽提示（安装过程中不显示） -->
    <Teleport to="body">
      <div v-if="dragActive && selectedDevice && !installing" class="drag-overlay">
        <div class="drag-overlay-inner">
          <el-icon class="drag-overlay-icon"><UploadFilled /></el-icon>
          <p>松开鼠标安装 APK</p>
        </div>
      </div>
    </Teleport>

    <!-- 安装中全屏 loading -->
    <Teleport to="body">
      <div v-if="installing" class="installing-overlay">
        <div class="installing-box">
          <div class="installing-spinner" />
          <p class="installing-text">
            正在安装 APK ({{ installProgress.current }}/{{ installProgress.total }})
          </p>
          <p class="installing-subtext" :title="installProgress.file">
            {{ installProgress.file }}
          </p>
        </div>
      </div>
    </Teleport>

    <el-dialog v-model="connectDialogVisible" title="连接设备" width="400px">
      <el-form label-width="80px">
        <el-form-item label="IP地址">
          <el-input v-model="ipAddress" placeholder="192.168.1.100" />
        </el-form-item>
        <el-form-item label="端口">
          <el-input-number v-model="port" :min="1" :max="65535" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="connectDialogVisible = false">取消</el-button>
        <el-button type="primary" @click="connectDevice">连接</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.device-panel {
  height: 100%;
}

.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 20px;
}

.panel-header h2 {
  margin: 0;
}

.header-actions {
  display: flex;
  gap: 10px;
}

.empty-state {
  display: flex;
  justify-content: center;
  align-items: center;
  height: 400px;
}

.info-card {
  margin-bottom: 20px;
}

.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.dropzone-card {
  margin-top: 8px;
  border: 2px dashed var(--el-border-color);
  background: var(--el-fill-color-lighter);
}

.dropzone-hint {
  display: flex;
  align-items: center;
  gap: 20px;
  padding: 8px 16px;
}

.dropzone-icon {
  font-size: 48px;
  color: var(--el-color-primary);
}

.dropzone-text {
  flex: 1;
}

.dropzone-title {
  margin: 0 0 4px;
  font-size: 16px;
  font-weight: 500;
  color: var(--el-text-color-primary);
}

.dropzone-desc {
  margin: 0;
  font-size: 13px;
  color: var(--el-text-color-secondary);
}

.dropzone-progress {
  margin: 6px 0 0;
  font-size: 13px;
  color: var(--el-color-primary);
}

</style>

<style>
/* Teleport 到 body 的全屏遮罩，用非 scoped 样式 */
.drag-overlay {
  position: fixed;
  inset: 0;
  background: rgba(64, 158, 255, 0.15);
  border: 4px dashed var(--el-color-primary);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 9999;
  pointer-events: none;
  backdrop-filter: blur(2px);
}

.drag-overlay-inner {
  background: #fff;
  padding: 40px 60px;
  border-radius: 12px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.15);
  text-align: center;
}

.drag-overlay-icon {
  font-size: 80px;
  color: var(--el-color-primary);
}

.drag-overlay-inner p {
  margin: 16px 0 0;
  font-size: 18px;
  font-weight: 500;
  color: var(--el-text-color-primary);
}

.installing-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.6);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 99999;
}

.installing-box {
  background: #fff;
  padding: 32px 48px;
  border-radius: 12px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3);
  text-align: center;
  min-width: 360px;
  max-width: 520px;
}

.installing-spinner {
  width: 48px;
  height: 48px;
  margin: 0 auto 16px;
  border: 4px solid #e4e7ed;
  border-top-color: #409eff;
  border-radius: 50%;
  animation: installing-spin 0.8s linear infinite;
}

@keyframes installing-spin {
  to { transform: rotate(360deg); }
}

.installing-text {
  margin: 0;
  font-size: 16px;
  font-weight: 500;
  color: #303133;
}

.installing-subtext {
  margin: 6px 0 0;
  font-size: 13px;
  color: #909399;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
