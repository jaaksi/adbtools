<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { ElMessage } from "element-plus";
import { save } from "@tauri-apps/plugin-dialog";
import {
  Camera,
  VideoCamera,
  Delete,
  Right,
} from "@element-plus/icons-vue";

const props = defineProps<{
  selectedDevice: string;
}>();

const shellCommand = ref("");
const shellOutput = ref("");
const shellLoading = ref(false);
const screenshotLoading = ref(false);
const recordLoading = ref(false);
const recordDuration = ref(10);

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
    defaultPath: `screenshot_${Date.now()}.png`,
    filters: [{ name: "PNG", extensions: ["png"] }],
  });

  if (savePath) {
    screenshotLoading.value = true;
    try {
      const result = await invoke("take_screenshot", {
        serial: props.selectedDevice,
        savePath,
      });
      ElMessage.success(result as string);
    } catch (error) {
      ElMessage.error(`截图失败: ${error}`);
    } finally {
      screenshotLoading.value = false;
    }
  }
}

async function startScreenRecord() {
  if (!props.selectedDevice) {
    ElMessage.warning("请先选择设备");
    return;
  }

  const savePath = await save({
    defaultPath: `screenrecord_${Date.now()}.mp4`,
    filters: [{ name: "MP4", extensions: ["mp4"] }],
  });

  if (savePath) {
    recordLoading.value = true;
    try {
      const result = await invoke("start_screen_record", {
        serial: props.selectedDevice,
        savePath,
        duration: recordDuration.value,
      });
      ElMessage.success(result as string);
    } catch (error) {
      ElMessage.error(`录屏失败: ${error}`);
    } finally {
      recordLoading.value = false;
    }
  }
}

function clearOutput() {
  shellOutput.value = "";
}
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
          <p class="tool-desc">录制设备屏幕视频（最大180秒）</p>
          <div class="record-controls">
            <el-input-number
              v-model="recordDuration"
              :min="1"
              :max="180"
              :step="10"
              style="width: 120px; margin-right: 10px"
            />
            <span style="margin-right: 10px">秒</span>
            <el-button
              type="primary"
              :icon="VideoCamera"
              :loading="recordLoading"
              @click="startScreenRecord"
            >
              开始录屏
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
