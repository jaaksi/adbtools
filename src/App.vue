<script setup lang="ts">
import { ref, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Refresh } from "@element-plus/icons-vue";
import DevicePanel from "./components/DevicePanel.vue";
import AppManager from "./components/AppManager.vue";
import FileManager from "./components/FileManager.vue";
import ToolsPanel from "./components/ToolsPanel.vue";
import GoogleAuth from "./components/GoogleAuth.vue";

interface Device {
  serial: string;
  status: string;
  model?: string;
  device_type?: string;
}

const devices = ref<Device[]>([]);
const selectedDevice = ref<string>("");
const activeTab = ref("devices");
const loading = ref(false);

async function refreshDevices() {
  loading.value = true;
  try {
    devices.value = await invoke("get_devices");
    if (devices.value.length > 0 && !selectedDevice.value) {
      selectedDevice.value = devices.value[0].serial;
    }
  } catch (error) {
    console.error("Failed to get devices:", error);
  } finally {
    loading.value = false;
  }
}

onMounted(() => {
  refreshDevices();
});
</script>

<template>
  <div class="app-container">
    <el-container class="full-height">
      <el-aside width="220px" class="sidebar">
        <div class="logo">
          <el-icon size="32" color="#409EFF"><Monitor /></el-icon>
          <span>Adb工具</span>
          <GoogleAuth class="logo-auth" />
        </div>
        
        <div class="device-section">
          <div class="section-header">
            <span>设备列表</span>
            <el-button 
              :icon="Refresh" 
              circle 
              size="small" 
              :loading="loading"
              @click="refreshDevices"
            />
          </div>
          <el-menu
            :default-active="selectedDevice"
            class="device-menu"
            @select="(index: string) => selectedDevice = index"
          >
            <el-menu-item 
              v-for="device in devices" 
              :key="device.serial"
              :index="device.serial"
            >
              <el-icon><Cellphone /></el-icon>
              <template #title>
                <div class="device-item">
                  <span class="device-name">{{ device.model || device.serial }}</span>
                  <el-tag 
                    :type="device.status === 'device' ? 'success' : 'warning'" 
                    size="small"
                  >
                    {{ device.status }}
                  </el-tag>
                </div>
              </template>
            </el-menu-item>
            <el-empty v-if="devices.length === 0" description="未连接设备" :image-size="60" />
          </el-menu>
        </div>

        <el-menu
          :default-active="activeTab"
          class="nav-menu"
          @select="(index: string) => activeTab = index"
        >
          <el-menu-item index="devices">
            <el-icon><Cellphone /></el-icon>
            <span>设备管理</span>
          </el-menu-item>
          <el-menu-item index="apps">
            <el-icon><Box /></el-icon>
            <span>应用管理</span>
          </el-menu-item>
          <el-menu-item index="files">
            <el-icon><Folder /></el-icon>
            <span>文件管理</span>
          </el-menu-item>
          <el-menu-item index="tools">
            <el-icon><Tools /></el-icon>
            <span>实用工具</span>
          </el-menu-item>
        </el-menu>
      </el-aside>

      <el-main class="main-content">
        <div class="main-body">
          <!-- 用 KeepAlive 缓存各面板，避免切 tab 时组件被销毁造成状态丢失（例如录屏中的计时器） -->
          <KeepAlive>
            <DevicePanel
              v-if="activeTab === 'devices'"
              :devices="devices"
              :selected-device="selectedDevice"
              @refresh="refreshDevices"
            />
            <AppManager
              v-else-if="activeTab === 'apps'"
              :selected-device="selectedDevice"
            />
            <FileManager
              v-else-if="activeTab === 'files'"
              :selected-device="selectedDevice"
            />
            <ToolsPanel
              v-else-if="activeTab === 'tools'"
              :selected-device="selectedDevice"
            />
          </KeepAlive>
        </div>
      </el-main>
    </el-container>
  </div>
</template>

<style scoped>
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

html, body {
  margin: 0;
  padding: 0;
  overflow: hidden;
}

.app-container {
  height: 100vh;
  width: 100vw;
  overflow: hidden;
}

.full-height {
  height: 100%;
  overflow: hidden;
}

.full-height :deep(.el-container) {
  overflow: hidden;
}

.sidebar {
  background: #fff;
  color: #303133;
  display: flex;
  flex-direction: column;
  border-right: 1px solid #e4e7ed;
  height: 100%;
  overflow: hidden;
}

.logo {
  height: 60px;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 0 15px;
  font-size: 18px;
  font-weight: bold;
  border-bottom: 1px solid #e4e7ed;
  color: #303133;
  flex-shrink: 0;
}

.device-section {
  padding: 15px;
  border-bottom: 1px solid #e4e7ed;
  overflow-y: auto;
  max-height: 300px;
}

/* 隐藏滚动条 */
.device-section::-webkit-scrollbar {
  display: none;
}

.section-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 10px;
  font-size: 14px;
  color: #606266;
}

.device-menu {
  background: transparent;
  border: none;
}

.device-menu :deep(.el-menu-item) {
  color: #606266;
  height: 60px;
  line-height: normal;
  padding: 10px 15px !important;
}

.device-menu :deep(.el-menu-item.is-active) {
  background: #ecf5ff;
  color: #409eff;
}

.device-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-left: 8px;
}

.device-name {
  font-size: 13px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 120px;
}

.nav-menu {
  background: transparent;
  border: none;
  flex: 1;
  overflow-y: auto;
}

/* 隐藏滚动条 */
.nav-menu::-webkit-scrollbar {
  display: none;
}

.nav-menu :deep(.el-menu-item) {
  color: #606266;
}

.nav-menu :deep(.el-menu-item.is-active) {
  background: #ecf5ff;
  color: #409eff;
}

.nav-menu :deep(.el-menu-item:hover) {
  background: #f5f7fa;
}

.main-content {
  background: #f5f7fa;
  padding: 20px;
  overflow-y: auto;
  height: 100%;
  overflow-x: hidden;
  display: flex;
  flex-direction: column;
}

/* 隐藏滚动条 */
.main-content::-webkit-scrollbar {
  display: none;
}

.logo-auth {
  margin-left: auto;
}

.main-body {
  flex: 1;
  overflow-y: auto;
}

/* 隐藏滚动条 */
.main-body::-webkit-scrollbar {
  display: none;
}
</style>
