<script setup lang="ts">
import { ref, computed, watch, onBeforeUnmount } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { ElMessage, ElMessageBox } from "element-plus";
import { save } from "@tauri-apps/plugin-dialog";
import {
  Camera,
  VideoCamera,
  VideoPause,
  Delete,
  Right,
  EditPen,
  Document,
  Search,
  Close,
  CopyDocument,
  Aim,
} from "@element-plus/icons-vue";

interface AppInfo {
  package_name: string;
  app_name?: string | null;
  version_name?: string | null;
  version_code?: string | null;
  is_system_app: boolean;
}

const props = defineProps<{
  selectedDevice: string;
}>();

const shellCommand = ref("");
const shellOutput = ref("");
const shellLoading = ref(false);
const screenshotLoading = ref(false);
const exportLogLoading = ref(false);

// 当前 Activity
const topActivity = ref<string>("");
const topActivityError = ref<string>("");
const topActivityLoading = ref(false);
const topActivityPolling = ref(false);
let topActivityTimer: number | null = null;

async function fetchTopActivity() {
  if (!props.selectedDevice) return;
  topActivityLoading.value = true;
  try {
    topActivity.value = await invoke<string>("get_top_activity", {
      serial: props.selectedDevice,
    });
    topActivityError.value = "";
  } catch (e) {
    topActivityError.value = String(e);
  } finally {
    topActivityLoading.value = false;
  }
}

function stopTopActivityPolling() {
  if (topActivityTimer !== null) {
    clearInterval(topActivityTimer);
    topActivityTimer = null;
  }
}

function onTopActivityPollingChange(v: boolean | string | number) {
  topActivityPolling.value = Boolean(v);
  stopTopActivityPolling();
  if (topActivityPolling.value) {
    fetchTopActivity();
    topActivityTimer = window.setInterval(fetchTopActivity, 1000);
  }
}

async function copyTopActivity() {
  if (!topActivity.value) return;
  try {
    await navigator.clipboard.writeText(topActivity.value);
    ElMessage.success("已复制到剪贴板");
  } catch (e) {
    ElMessage.error(`复制失败: ${e}`);
  }
}

// 切换设备时：清空上一次结果并重置轮询
watch(
  () => props.selectedDevice,
  () => {
    topActivity.value = "";
    topActivityError.value = "";
    if (topActivityPolling.value) {
      // 轮询保持开启，立即用新设备拉一次
      fetchTopActivity();
    }
  }
);

// 日志导出 - 包名过滤
const selectedLogPackage = ref("");
const appPickerVisible = ref(false);
const appPickerLoading = ref(false);
const appList = ref<AppInfo[]>([]);
const appSearchKeyword = ref("");

const filteredAppList = computed(() => {
  const kw = appSearchKeyword.value.trim().toLowerCase();
  if (!kw) return appList.value;
  return appList.value.filter((a) =>
    a.package_name.toLowerCase().includes(kw)
  );
});

// 输入文本
const inputText = ref("");
const inputTextLoading = ref(false);

// 录屏状态
const isRecording = ref(false);
const isSaving = ref(false); // 点击结束后 → pull 文件期间的过渡状态
const recordLoading = ref(false);
const recordStartTime = ref(0);
const recordElapsed = ref(0);
const recordSavePath = ref<string | null>(null);
let recordTimer: number | null = null;

const recordElapsedText = computed(() => {
  const s = recordElapsed.value;
  const mm = String(Math.floor(s / 60)).padStart(2, "0");
  const ss = String(s % 60).padStart(2, "0");
  return `${mm}:${ss}`;
});

// 文件名时间戳：格式 YYYYMMDDHHmmss（例如 20260422175730）
function formatTimestamp(d: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return (
    `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}` +
    `${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}`
  );
}

// 保存目录：用过一次后记住，否则默认 ~/Documents/adbtools
const LAST_SAVE_DIR_KEY = "adbtools:lastSaveDir";

async function resolveSaveDefault(filename: string): Promise<string> {
  let dir = localStorage.getItem(LAST_SAVE_DIR_KEY);
  if (!dir) {
    try {
      dir = await invoke<string>("ensure_default_save_dir");
    } catch (e) {
      console.warn("获取默认保存目录失败:", e);
      return filename;
    }
  }
  const sep = dir.includes("\\") && !dir.includes("/") ? "\\" : "/";
  return `${dir}${sep}${filename}`;
}

function rememberSaveDir(savedPath: string) {
  const i = Math.max(savedPath.lastIndexOf("/"), savedPath.lastIndexOf("\\"));
  if (i > 0) {
    localStorage.setItem(LAST_SAVE_DIR_KEY, savedPath.slice(0, i));
  }
}

async function runShellCommand() {
  if (!props.selectedDevice) {
    ElMessage.warning("请先选择设备");
    return;
  }
  if (!shellCommand.value.trim()) {
    ElMessage.warning("请输入命令");
    return;
  }

  shellLoading.value = true;
  try {
    const result = await invoke("run_shell_command", {
      serial: props.selectedDevice,
      command: shellCommand.value,
    });
    shellOutput.value += `> ${shellCommand.value}\n${result}\n\n`;
  } catch (error) {
    shellOutput.value += `> ${shellCommand.value}\nError: ${error}\n\n`;
  } finally {
    shellLoading.value = false;
  }
}

async function takeScreenshot() {
  if (!props.selectedDevice) {
    ElMessage.warning("请先选择设备");
    return;
  }

  const savePath = await save({
    defaultPath: await resolveSaveDefault(`screenshot_${formatTimestamp()}.png`),
    filters: [{ name: "PNG", extensions: ["png"] }],
  });

  if (savePath) {
    screenshotLoading.value = true;
    try {
      await invoke("take_screenshot", {
        serial: props.selectedDevice,
        savePath,
      });
      rememberSaveDir(savePath);
      ElMessage.success("截图已保存");
      // 自动在文件管理器中定位到保存的文件
      try {
        await invoke("reveal_in_folder", { path: savePath });
      } catch (e) {
        console.error("打开所在目录失败:", e);
      }
    } catch (error) {
      ElMessage.error(`截图失败: ${error}`);
    } finally {
      screenshotLoading.value = false;
    }
  }
}

// 打开应用选择弹窗并加载第三方应用列表
async function openAppPicker() {
  if (!props.selectedDevice) {
    ElMessage.warning("请先选择设备");
    return;
  }
  appPickerVisible.value = true;
  if (appList.value.length === 0) {
    await loadApps();
  }
}

async function loadApps() {
  appPickerLoading.value = true;
  try {
    const apps = await invoke<AppInfo[]>("get_installed_apps", {
      serial: props.selectedDevice,
      filter: "third",
    });
    appList.value = apps;
  } catch (e) {
    ElMessage.error(`获取应用列表失败: ${e}`);
  } finally {
    appPickerLoading.value = false;
  }
}

function selectAppForLog(app: AppInfo) {
  selectedLogPackage.value = app.package_name;
  appPickerVisible.value = false;
}

function clearSelectedLogPackage() {
  selectedLogPackage.value = "";
}

// 导出设备日志（adb logcat -d）到本地文本文件
async function exportLogcat() {
  if (!props.selectedDevice) {
    ElMessage.warning("请先选择设备");
    return;
  }

  const savePath = await save({
    defaultPath: await resolveSaveDefault(`logcat_${formatTimestamp()}.log`),
    filters: [
      { name: "Log", extensions: ["log", "txt"] },
    ],
  });
  if (!savePath) return;

  exportLogLoading.value = true;
  try {
    const result = await invoke<string>("export_logcat", {
      serial: props.selectedDevice,
      savePath,
      buffers: "all",
      package: selectedLogPackage.value || null,
    });
    rememberSaveDir(savePath);
    ElMessage.success(result || "日志已导出");
    try {
      await invoke("reveal_in_folder", { path: savePath });
    } catch (e) {
      console.error("打开所在目录失败:", e);
    }
  } catch (error) {
    ElMessage.error(`导出日志失败: ${error}`);
  } finally {
    exportLogLoading.value = false;
  }
}

async function toggleScreenRecord() {
  if (isRecording.value) {
    await stopScreenRecord();
  } else {
    await startScreenRecord();
  }
}

async function startScreenRecord() {
  if (!props.selectedDevice) {
    ElMessage.warning("请先选择设备");
    return;
  }

  // 开始前先让用户选好保存路径
  const savePath = await save({
    defaultPath: await resolveSaveDefault(`screenrecord_${formatTimestamp()}.mp4`),
    filters: [{ name: "MP4", extensions: ["mp4"] }],
  });
  if (!savePath) return;
  rememberSaveDir(savePath);

  recordLoading.value = true;
  try {
    await doStartRecord(savePath);
  } catch (error) {
    const msg = String(error);
    if (msg.includes("已有录屏任务")) {
      await handleStaleRecording(savePath);
      return;
    }
    ElMessage.error(`启动录屏失败: ${msg}`);
  } finally {
    recordLoading.value = false;
  }
}

// 处理后端报"已有录屏任务"的情况：先查设备端是否真的在录，再决定是否弹窗
async function handleStaleRecording(savePath: string) {
  recordLoading.value = false;

  // 查设备端 pidof screenrecord
  let actuallyRecording = false;
  try {
    actuallyRecording = await invoke<boolean>("is_screen_recording", {
      serial: props.selectedDevice,
    });
  } catch (e) {
    console.warn("查询设备录屏状态失败，按未在录屏处理:", e);
  }

  // 设备端没在录（僵尸会话）→ 静默清理并重启
  if (!actuallyRecording) {
    recordLoading.value = true;
    try {
      await invoke("cancel_screen_record");
      await doStartRecord(savePath);
    } catch (e) {
      ElMessage.error(`启动录屏失败: ${e}`);
    } finally {
      recordLoading.value = false;
    }
    return;
  }

  // 设备端真的在录 → 弹窗让用户选
  let action: "restart" | "stop" | null = null;
  try {
    await ElMessageBox({
      title: "检测到正在进行的录屏任务",
      message: "设备端仍有 screenrecord 进程在运行，请选择如何处理。",
      type: "warning",
      showCancelButton: true,
      confirmButtonText: "取消并重试",
      cancelButtonText: "结束当前录制",
      distinguishCancelAndClose: true, // 区分"点击取消"与"关闭对话框"
    });
    action = "restart";
  } catch (reason) {
    if (reason === "cancel") {
      action = "stop";
    } else {
      // 关闭/ESC → 什么都不做
      return;
    }
  }

  recordLoading.value = true;
  try {
    await invoke("cancel_screen_record");
    if (action === "restart") {
      await doStartRecord(savePath);
    } else {
      ElMessage.success("已结束残留的录屏任务");
    }
  } catch (e) {
    ElMessage.error(`操作失败: ${e}`);
  } finally {
    recordLoading.value = false;
  }
}

async function doStartRecord(savePath: string) {
  await invoke("start_screen_record", { serial: props.selectedDevice });
  recordSavePath.value = savePath;
  recordStartTime.value = Date.now();
  recordElapsed.value = 0;
  isRecording.value = true;

  // 计时器每秒更新一次已录制时长
  recordTimer = window.setInterval(() => {
    recordElapsed.value = Math.floor((Date.now() - recordStartTime.value) / 1000);
    // screenrecord 单段最大 180 秒，超过后设备会自动停止
    if (recordElapsed.value >= 180) {
      ElMessage.info("已达 180 秒上限，正在停止并保存");
      stopScreenRecord();
    }
  }, 1000);

  ElMessage.success("开始录屏");
}

async function stopScreenRecord() {
  if (!isRecording.value) return;

  // 先停止计时，避免多次触发
  if (recordTimer !== null) {
    clearInterval(recordTimer);
    recordTimer = null;
  }

  // 立即进入"保存中"过渡态，组件模板里会渲染全屏 loading 遮罩
  isSaving.value = true;

  const savedPath = recordSavePath.value;
  try {
    await invoke("stop_screen_record", { savePath: savedPath });
    ElMessage.success("录屏已保存");

    // 自动在文件管理器中定位到保存的文件
    if (savedPath) {
      try {
        await invoke("reveal_in_folder", { path: savedPath });
      } catch (e) {
        console.error("打开所在目录失败:", e);
      }
    }
  } catch (error) {
    ElMessage.error(`结束录屏失败: ${error}`);
  } finally {
    isRecording.value = false;
    isSaving.value = false;
    recordSavePath.value = null;
    recordElapsed.value = 0;
  }
}

async function sendInputText() {
  if (!props.selectedDevice) {
    ElMessage.warning("请先选择设备");
    return;
  }
  const text = inputText.value;
  if (!text) {
    ElMessage.warning("请输入要发送的文本");
    return;
  }

  inputTextLoading.value = true;
  try {
    await invoke("input_text", { serial: props.selectedDevice, text });
    ElMessage.success("文本已发送到设备");
    inputText.value = "";
  } catch (error) {
    ElMessage.error(`发送失败: ${error}`);
  } finally {
    inputTextLoading.value = false;
  }
}

function clearOutput() {
  shellOutput.value = "";
}

onBeforeUnmount(() => {
  if (recordTimer !== null) {
    clearInterval(recordTimer);
    recordTimer = null;
  }
  stopTopActivityPolling();
});
</script>

<template>
  <div class="tools-panel">
    <div class="panel-header">
      <h2>实用工具</h2>
    </div>

    <div v-if="!selectedDevice" class="empty-state">
      <el-empty description="请先选择设备" />
    </div>

    <el-row v-else :gutter="20">
      <el-col :span="12">
        <el-card class="tool-card">
          <template #header>
            <div class="card-header">
              <el-icon><Camera /></el-icon>
              <span>截图工具</span>
            </div>
          </template>
          <p class="tool-desc">截取设备当前屏幕并保存到本地</p>
          <el-button
            type="primary"
            :icon="Camera"
            :loading="screenshotLoading"
            @click="takeScreenshot"
          >
            截图
          </el-button>
        </el-card>
      </el-col>

      <el-col :span="12">
        <el-card class="tool-card">
          <template #header>
            <div class="card-header">
              <el-icon><VideoCamera /></el-icon>
              <span>录屏工具</span>
            </div>
          </template>
          <p class="tool-desc">录制设备屏幕视频（单段最长 180 秒）</p>
          <div class="record-controls">
            <el-button
              :type="isRecording ? 'danger' : 'primary'"
              :icon="isRecording ? VideoPause : VideoCamera"
              :loading="recordLoading || isSaving"
              :disabled="isSaving"
              @click="toggleScreenRecord"
            >
              {{ isSaving ? "保存中..." : isRecording ? "结束录屏" : "开始录屏" }}
            </el-button>
            <span v-if="isSaving" class="record-indicator saving">
              正在拉取视频到本地，请稍候
            </span>
            <span v-else-if="isRecording" class="record-indicator">
              <span class="record-dot" />
              录制中 {{ recordElapsedText }}
            </span>
          </div>
        </el-card>
      </el-col>

      <el-col :span="12" style="margin-top: 20px">
        <el-card class="tool-card log-card">
          <template #header>
            <div class="card-header">
              <el-icon><Document /></el-icon>
              <span>日志导出</span>
            </div>
          </template>
          <p class="tool-desc">
            导出设备当前 logcat 全部缓冲区快照到本地文件，可按应用过滤
          </p>
          <div class="log-actions">
            <el-button
              type="primary"
              :icon="Document"
              :loading="exportLogLoading"
              @click="exportLogcat"
            >
              导出日志
            </el-button>
            <el-button :icon="Search" @click="openAppPicker">
              选择应用
            </el-button>
          </div>
          <div class="log-selected-pkg">
            <span class="log-selected-label">已选应用：</span>
            <el-tag
              v-if="selectedLogPackage"
              closable
              type="success"
              @close="clearSelectedLogPackage"
            >
              {{ selectedLogPackage }}
            </el-tag>
            <span v-else class="log-selected-empty">未选择（将导出全部日志）</span>
          </div>
        </el-card>
      </el-col>

      <!-- 应用选择弹窗 -->
      <el-dialog
        v-model="appPickerVisible"
        title="选择要过滤的应用"
        width="520px"
        append-to-body
      >
        <div v-if="selectedLogPackage" class="app-picker-current">
          <span class="log-selected-label">当前已选：</span>
          <el-tag closable type="success" @close="clearSelectedLogPackage">
            {{ selectedLogPackage }}
          </el-tag>
        </div>
        <el-input
          v-model="appSearchKeyword"
          placeholder="输入包名关键字搜索"
          clearable
          :prefix-icon="Search"
        />
        <div
          v-loading="appPickerLoading"
          class="app-picker-list"
          element-loading-text="加载应用列表中..."
        >
          <div v-if="!appPickerLoading && filteredAppList.length === 0" class="app-picker-empty">
            没有匹配的应用
          </div>
          <div
            v-for="app in filteredAppList"
            :key="app.package_name"
            class="app-picker-item"
            :class="{ active: app.package_name === selectedLogPackage }"
            @click="selectAppForLog(app)"
          >
            {{ app.package_name }}
          </div>
        </div>
        <template #footer>
          <el-button :icon="Close" @click="appPickerVisible = false">关闭</el-button>
          <el-button @click="loadApps" :loading="appPickerLoading">刷新</el-button>
        </template>
      </el-dialog>

      <el-col :span="24" style="margin-top: 20px">
        <el-card class="tool-card top-activity-card">
          <template #header>
            <div class="card-header">
              <el-icon><Aim /></el-icon>
              <span>当前 Activity</span>
              <el-switch
                v-model="topActivityPolling"
                inline-prompt
                active-text="轮询"
                inactive-text="关闭"
                @change="onTopActivityPollingChange"
              />
            </div>
          </template>
          <p class="tool-desc">
            读取 dumpsys window 的 mCurrentFocus；开启轮询后每秒刷新一次
          </p>
          <div class="top-activity-row">
            <el-input
              :model-value="topActivity"
              readonly
              placeholder="点击「读取一次」或开启轮询"
            />
            <el-button
              :icon="Aim"
              :loading="topActivityLoading"
              @click="fetchTopActivity"
            >
              读取一次
            </el-button>
            <el-button
              :icon="CopyDocument"
              :disabled="!topActivity"
              @click="copyTopActivity"
            >
              复制
            </el-button>
          </div>
          <p v-if="topActivityError" class="top-activity-error">
            {{ topActivityError }}
          </p>
        </el-card>
      </el-col>

      <el-col :span="24" style="margin-top: 20px">
        <el-card class="tool-card input-text-card">
          <template #header>
            <div class="card-header">
              <el-icon><EditPen /></el-icon>
              <span>输入文本</span>
            </div>
          </template>
          <p class="tool-desc">
            将文本发送到设备当前聚焦的输入框（adb shell input text，仅支持 ASCII）
          </p>
          <div class="input-text-controls">
            <el-input
              v-model="inputText"
              placeholder="输入文本后点击提交或回车"
              clearable
              @keyup.enter="sendInputText"
            />
            <el-button
              type="primary"
              :icon="Right"
              :loading="inputTextLoading"
              @click="sendInputText"
            >
              提交
            </el-button>
          </div>
        </el-card>
      </el-col>

      <el-col :span="24" style="margin-top: 20px">
        <el-card class="shell-card">
          <template #header>
            <div class="card-header">
              <el-icon><Terminal /></el-icon>
              <span>Shell 终端</span>
              <el-button size="small" :icon="Delete" @click="clearOutput">
                清空
              </el-button>
            </div>
          </template>
          <div class="shell-container">
            <pre class="shell-output">{{ shellOutput || "等待输入命令..." }}</pre>
            <div class="shell-input">
              <el-input
                v-model="shellCommand"
                placeholder="输入 ADB Shell 命令，如: ls -la /sdcard"
                @keyup.enter="runShellCommand"
              >
                <template #append>
                  <el-button
                    :icon="Right"
                    :loading="shellLoading"
                    @click="runShellCommand"
                  >
                    执行
                  </el-button>
                </template>
              </el-input>
            </div>
          </div>
        </el-card>
      </el-col>
    </el-row>

    <!-- 录屏保存全屏遮罩 -->
    <Teleport to="body">
      <div v-if="isSaving" class="saving-overlay">
        <div class="saving-box">
          <div class="saving-spinner" />
          <p class="saving-text">正在保存录屏视频到本地...</p>
          <p class="saving-subtext">稍候会自动打开文件所在目录</p>
        </div>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.tools-panel {
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

.empty-state {
  display: flex;
  justify-content: center;
  align-items: center;
  height: 400px;
}

.tool-card {
  height: 200px;
}

.log-card {
  height: auto;
  min-height: 200px;
}

.card-header {
  display: flex;
  align-items: center;
  gap: 8px;
  font-weight: bold;
}

.tool-desc {
  color: #666;
  margin-bottom: 20px;
}

.record-controls {
  display: flex;
  align-items: center;
  gap: 12px;
}

.log-actions {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.log-selected-pkg {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 12px;
  font-size: 13px;
  flex-wrap: wrap;
}

.log-selected-label {
  color: var(--el-text-color-regular);
}

.log-selected-empty {
  color: var(--el-text-color-secondary);
}

.app-picker-current {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 12px;
  font-size: 13px;
}

.app-picker-list {
  margin-top: 12px;
  max-height: 380px;
  overflow-y: auto;
  border: 1px solid var(--el-border-color-lighter);
  border-radius: 4px;
}

.app-picker-item {
  padding: 8px 12px;
  cursor: pointer;
  font-family: "Courier New", monospace;
  font-size: 13px;
  border-bottom: 1px solid var(--el-border-color-lighter);
}

.app-picker-item:last-child {
  border-bottom: none;
}

.app-picker-item:hover {
  background: var(--el-fill-color-light);
}

.app-picker-item.active {
  background: var(--el-color-primary-light-8);
  color: var(--el-color-primary);
  font-weight: 500;
}

.app-picker-empty {
  padding: 24px;
  text-align: center;
  color: var(--el-text-color-secondary);
}

.top-activity-card {
  height: auto;
}

.top-activity-row {
  display: flex;
  align-items: center;
  gap: 10px;
}

.top-activity-row .el-input {
  flex: 1;
  font-family: "Courier New", monospace;
}

.top-activity-error {
  margin: 10px 0 0;
  color: var(--el-color-danger);
  font-size: 12px;
}

.input-text-card {
  height: auto;
}

.input-text-controls {
  display: flex;
  align-items: center;
  gap: 10px;
}

.input-text-controls .el-input {
  flex: 1;
}

.record-indicator {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 14px;
  color: var(--el-color-danger);
  font-variant-numeric: tabular-nums;
}

.record-indicator.saving {
  color: var(--el-color-info);
}

.record-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--el-color-danger);
  animation: record-blink 1s ease-in-out infinite;
}

@keyframes record-blink {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.2; }
}

.shell-card {
  margin-top: 10px;
}

.shell-container {
  background: #1e1e1e;
  border-radius: 8px;
  overflow: hidden;
}

.shell-output {
  height: 300px;
  overflow-y: auto;
  padding: 15px;
  margin: 0;
  color: #4ec9b0;
  font-family: "Courier New", monospace;
  font-size: 13px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-wrap: break-word;
}

.shell-input {
  padding: 10px;
  background: #2d2d2d;
  border-top: 1px solid #3d3d3d;
}

.shell-input :deep(.el-input__wrapper) {
  background: #1e1e1e;
}

.shell-input :deep(.el-input__inner) {
  color: #4ec9b0;
  font-family: "Courier New", monospace;
}
</style>

<style>
/* Teleport 到 body 的全屏遮罩，必须用非 scoped 样式 */
.saving-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.6);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 99999;
}

.saving-box {
  background: #fff;
  padding: 32px 48px;
  border-radius: 12px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.3);
  text-align: center;
  min-width: 320px;
}

.saving-spinner {
  width: 48px;
  height: 48px;
  margin: 0 auto 16px;
  border: 4px solid #e4e7ed;
  border-top-color: #409eff;
  border-radius: 50%;
  animation: saving-spin 0.8s linear infinite;
}

@keyframes saving-spin {
  to { transform: rotate(360deg); }
}

.saving-text {
  margin: 0;
  font-size: 16px;
  font-weight: 500;
  color: #303133;
}

.saving-subtext {
  margin: 6px 0 0;
  font-size: 13px;
  color: #909399;
}
</style>
