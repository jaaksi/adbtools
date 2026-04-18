<script setup lang="ts">
import { ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { ElMessage } from "element-plus";
import { Plus, Refresh } from "@element-plus/icons-vue";

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
  battery_level: string;
  battery_status: string;
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

async function disconnectDevice() {
  try {
    const result = await invoke("disconnect_device", { address: null });
    ElMessage.success(result as string);
    emit("refresh");
  } catch (error) {
    ElMessage.error(`断开连接失败: ${error}`);
  }
}

async function rebootDevice(mode?: string) {
  if (!props.selectedDevice) {
    ElMessage.warning("请先选择设备");
    return;
  }
  try {
    await invoke("reboot_device", { serial: props.selectedDevice, mode });
    ElMessage.success("重启命令已发送");
    setTimeout(() => emit("refresh"), 3000);
  } catch (error) {
    ElMessage.error(`重启失败: ${error}`);
  }
}

watch(() => props.selectedDevice, loadDeviceInfo, { immediate: true });
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
      <el-row :gutter="20">
        <el-col :span="16">
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
              <el-descriptions-item label="序列号">
                {{ deviceInfo.serial }}
              </el-descriptions-item>
              <el-descriptions-item label="Android 版本">
                {{ deviceInfo.android_version }}
              </el-descriptions-item>
              <el-descriptions-item label="SDK 版本">
                {{ deviceInfo.sdk_version }}
              </el-descriptions-item>
              <el-descriptions-item label="屏幕分辨率">
                {{ deviceInfo.screen_resolution }}
              </el-descriptions-item>
              <el-descriptions-item label="屏幕密度">
                {{ deviceInfo.density }}
              </el-descriptions-item>
              <el-descriptions-item label="电池电量">
                <el-progress
                  :percentage="parseInt(deviceInfo.battery_level) || 0"
                  :status="
                    parseInt(deviceInfo.battery_level) > 20 ? 'success' : 'exception'
                  "
                />
              </el-descriptions-item>
            </el-descriptions>
          </el-card>
        </el-col>

        <el-col :span="8">
          <el-card class="action-card">
            <template #header>
              <span>快捷操作</span>
            </template>
            <div class="action-buttons">
              <el-button type="primary" @click="rebootDevice()">
                <el-icon><RefreshRight /></el-icon>
                重启设备
              </el-button>
              <el-button @click="rebootDevice('recovery')">
                <el-icon><FirstAidKit /></el-icon>
                重启到 Recovery
              </el-button>
              <el-button @click="rebootDevice('bootloader')">
                <el-icon><Cpu /></el-icon>
                重启到 Bootloader
              </el-button>
              <el-divider />
              <el-button type="danger" @click="disconnectDevice">
                <el-icon><CircleClose /></el-icon>
                断开连接
              </el-button>
            </div>
          </el-card>
        </el-col>
      </el-row>
    </div>

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

.action-card {
  height: 100%;
}

.action-buttons {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.action-buttons .el-button {
  justify-content: flex-start;
}
</style>
