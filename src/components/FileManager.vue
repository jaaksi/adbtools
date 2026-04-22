<script setup lang="ts">
import { ref, watch, computed, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { ElMessage } from "element-plus";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
  ArrowUp,
  ArrowLeft,
  Upload,
  Refresh,
  Download,
  Delete,
  Search,
  Box,
} from "@element-plus/icons-vue";

interface FileInfo {
  name: string;
  path: string;
  is_dir: boolean;
  size?: string;
  permissions?: string;
  modified_time?: string;
}

interface AppInfo {
  package_name: string;
  app_name?: string;
  version_name?: string;
  version_code?: string;
  is_system_app: boolean;
}

const props = defineProps<{
  selectedDevice: string;
}>();

const files = ref<FileInfo[]>([]);
const apps = ref<AppInfo[]>([]);
const allApps = ref<AppInfo[]>([]);
const loading = ref(false);
const currentPath = ref("/data/data");
const pathHistory = ref<string[]>([]);
const isDataDataView = ref(false);
const searchQuery = ref("");

// 从 localStorage 加载搜索记录
onMounted(() => {
  const savedSearch = localStorage.getItem("adb_tool_app_search");
  if (savedSearch) {
    searchQuery.value = savedSearch;
  }
});

// 过滤后的应用列表
const filteredApps = computed(() => {
  const query = searchQuery.value.trim().toLowerCase();
  if (!query) {
    return apps.value;
  }
  return apps.value.filter(app => 
    app.package_name.toLowerCase().includes(query)
  );
});

// 路径分段，用于面包屑导航
const pathSegments = computed(() => {
  if (currentPath.value === "/") {
    return [{ name: "", path: "/" }];
  }
  const parts = currentPath.value.split("/").filter(p => p !== "");
  const segments = [{ name: "", path: "/" }]; // 根目录
  let currentPathBuilder = "";
  for (const part of parts) {
    currentPathBuilder += "/" + part;
    segments.push({ name: part, path: currentPathBuilder });
  }
  return segments;
});

// 点击面包屑导航
function navigateToSegment(index: number) {
  const segment = pathSegments.value[index];
  if (segment.path !== currentPath.value) {
    pathHistory.value.push(currentPath.value);
    // 如果点击的是 /data，跳转到 /data/data 应用列表页（因为 /data 需要 root 权限）
    if (segment.path === "/data") {
      currentPath.value = "/data/data";
    } else {
      currentPath.value = segment.path;
    }
    loadFiles();
  }
}

// 保存搜索记录
watch(searchQuery, (newValue) => {
  localStorage.setItem("adb_tool_app_search", newValue);
});

async function loadFiles() {
  if (!props.selectedDevice) {
    files.value = [];
    apps.value = [];
    allApps.value = [];
    return;
  }
  loading.value = true;
  
  if (currentPath.value === "/data/data") {
    isDataDataView.value = true;
    try {
      // 直接获取所有已安装应用，不测试可调试性
      const result = await invoke("get_installed_apps", {
        serial: props.selectedDevice,
        filter: null,
      });
      allApps.value = result as AppInfo[];
      apps.value = allApps.value;
      files.value = [];
    } catch (error) {
      console.error("Failed to get apps:", error);
      ElMessage.error(`获取应用列表失败：${error}`);
      apps.value = [];
      allApps.value = [];
    } finally {
      loading.value = false;
    }
    return;
  }
  
  isDataDataView.value = false;
  apps.value = [];
  
  try {
    if (currentPath.value.startsWith("/data/data/")) {
      const afterData = currentPath.value.substring(11);
      const slashIdx = afterData.indexOf('/');
      const packageName = slashIdx === -1 ? afterData : afterData.substring(0, slashIdx);
      const subPath = slashIdx === -1 ? "" : afterData.substring(slashIdx + 1);
      console.log("run_as_list_files:", { packageName, subPath, fullPath: currentPath.value });
      files.value = await invoke("run_as_list_files", {
        serial: props.selectedDevice,
        packageName,
        path: subPath,
      });
    } else {
      console.log("Using list_files for path:", currentPath.value);
      files.value = await invoke("list_files", {
        serial: props.selectedDevice,
        path: currentPath.value,
      });
    }
  } catch (error) {
    console.error("Failed to list files:", currentPath.value, error);
    ElMessage.error(`获取文件列表失败：${error}`);
    files.value = [];
  } finally {
    loading.value = false;
  }
}

async function navigateTo(path: string) {
  pathHistory.value.push(currentPath.value);
  
  // 如果是 run-as 路径，需要构建正确的相对路径
  if (currentPath.value.startsWith("/data/data/")) {
    const afterData = currentPath.value.substring(11);
    const slashIdx = afterData.indexOf('/');
    const packageName = slashIdx === -1 ? afterData : afterData.substring(0, slashIdx);
    const currentSubPath = slashIdx === -1 ? "" : afterData.substring(slashIdx + 1);
    
    // 如果 path 是绝对路径（以 / 开头），需要去掉开头的 /
    const cleanPath = path.startsWith('/') ? path.substring(1) : path;
    const newSubPath = currentSubPath ? `${currentSubPath}/${cleanPath}` : cleanPath;
    
    currentPath.value = `/data/data/${packageName}/${newSubPath}`;
  } else {
    currentPath.value = path;
  }
  
  loadFiles();
}

function navigateBack() {
  if (pathHistory.value.length > 0) {
    currentPath.value = pathHistory.value.pop()!;
    loadFiles();
  }
}

function navigateUp() {
  if (currentPath.value === "/") return;
  
  if (currentPath.value === "/data/data") {
    navigateTo("/sdcard");
    return;
  }
  
  if (currentPath.value.startsWith("/data/data/")) {
    const afterData = currentPath.value.substring(11);
    const slashIdx = afterData.indexOf('/');
    if (slashIdx !== -1) {
      const packageName = afterData.substring(0, slashIdx);
      const subPath = afterData.substring(slashIdx + 1);
      const parentSubPath = subPath.split('/').slice(0, -1).join('/');
      currentPath.value = `/data/data/${packageName}${parentSubPath ? '/' + parentSubPath : ''}`;
      navigateTo(currentPath.value);
      return;
    }
  }
  
  const parts = currentPath.value.split("/").filter(p => p !== "");
  if (parts.length <= 1) {
    currentPath.value = "/";
  } else {
    parts.pop();
    currentPath.value = "/" + parts.join("/");
  }
  navigateTo(currentPath.value);
}

async function openAppData(packageName: string) {
  pathHistory.value.push(currentPath.value);
  currentPath.value = `/data/data/${packageName}`;
  loadFiles();
}

async function pushFile() {
  if (!props.selectedDevice) {
    ElMessage.warning("请先选择设备");
    return;
  }

  const file = await open({
    multiple: false,
  });

  if (file) {
    try {
      loading.value = true;
      const fileName = file.split("/").pop() || "file";
      const remotePath = `${currentPath.value}/${fileName}`;
      const result = await invoke("push_file", {
        serial: props.selectedDevice,
        localPath: file,
        remotePath,
      });
      ElMessage.success(result as string);
      loadFiles();
    } catch (error) {
      ElMessage.error(`上传失败：${error}`);
    } finally {
      loading.value = false;
    }
  }
}

async function pullFile(remotePath: string) {
  if (!props.selectedDevice) return;

  const savePath = await save({
    defaultPath: remotePath.split("/").pop(),
  });

  if (savePath) {
    try {
      loading.value = true;
      const result = await invoke("pull_file", {
        serial: props.selectedDevice,
        remotePath,
        localPath: savePath,
      });
      ElMessage.success(result as string);
    } catch (error) {
      ElMessage.error(`下载失败：${error}`);
    } finally {
      loading.value = false;
    }
  }
}

async function deleteFile(path: string) {
  if (!props.selectedDevice) return;

  try {
    await invoke("delete_file", {
      serial: props.selectedDevice,
      remotePath: path,
    });
    ElMessage.success("删除成功");
    loadFiles();
  } catch (error) {
    ElMessage.error(`删除失败：${error}`);
  }
}

watch(() => props.selectedDevice, loadFiles, { immediate: true });
</script>

<template>
  <div class="file-manager">
    <div class="panel-header">
      <h2>文件管理</h2>
      <div class="header-actions">
        <el-button :icon="ArrowUp" @click="navigateUp" :disabled="currentPath === '/'">
          上级目录
        </el-button>
        <el-button :icon="ArrowLeft" @click="navigateBack" :disabled="pathHistory.length === 0">
          返回
        </el-button>
        <el-button type="primary" :icon="Upload" @click="pushFile">
          上传文件
        </el-button>
        <el-button :icon="Refresh" @click="loadFiles">刷新</el-button>
      </div>
    </div>

    <div class="breadcrumb">
      <span class="breadcrumb-label">当前路径：</span>
      <el-breadcrumb separator="/">
        <el-breadcrumb-item 
          v-for="(segment, index) in pathSegments" 
          :key="index"
          @click="navigateToSegment(index)"
          :class="{ 'clickable': true }"
        >
          {{ segment.name || '根目录' }}
        </el-breadcrumb-item>
      </el-breadcrumb>
    </div>

    <div v-if="!selectedDevice" class="empty-state">
      <el-empty description="请先选择设备" />
    </div>

    <div v-else-if="isDataDataView" v-loading="loading" class="apps-container">
      <div class="apps-header">
        <span>已安装应用 ({{ filteredApps.length }} / {{ apps.length }} 个)</span>
        <el-input
          v-model="searchQuery"
          placeholder="搜索应用包名"
          :prefix-icon="Search"
          clearable
          style="width: 300px"
          autocomplete="off"
        />
      </div>
      <el-table
        :data="filteredApps"
        class="fill-table"
        style="width: 100%"
        height="100%"
        @row-click="(row: AppInfo) => openAppData(row.package_name)"
      >
        <el-table-column prop="package_name" label="应用包名" min-width="300">
          <template #default="{ row }">
            <div class="app-item">
              <el-icon><Box /></el-icon>
              <span>{{ row.package_name }}</span>
            </div>
          </template>
        </el-table-column>
        <el-table-column label="操作" width="100" fixed="right">
          <template #default="{ row }">
            <el-button size="small" @click.stop="openAppData(row.package_name)">
              打开
            </el-button>
          </template>
        </el-table-column>
      </el-table>
    </div>

    <el-table
      v-else
      v-loading="loading"
      :data="files"
      class="fill-table"
      style="width: 100%"
      height="100%"
      @row-click="(row: FileInfo) => row.is_dir && navigateTo(row.path)"
    >
      <el-table-column width="50">
        <template #default="{ row }">
          <el-icon :size="20">
            <Folder v-if="row.is_dir" color="#409EFF" />
            <Document v-else color="#909399" />
          </el-icon>
        </template>
      </el-table-column>
      <el-table-column prop="name" label="名称" min-width="200">
        <template #default="{ row }">
          <span :class="{ 'folder-name': row.is_dir }">
            {{ row.name }}
          </span>
        </template>
      </el-table-column>
      <el-table-column prop="size" label="大小" width="120" />
      <el-table-column prop="permissions" label="权限" width="100" />
      <el-table-column prop="modified_time" label="修改时间" width="180" />
      <el-table-column label="操作" width="200" fixed="right">
        <template #default="{ row }">
          <el-button-group>
            <el-button
              v-if="!row.is_dir"
              size="small"
              :icon="Download"
              @click.stop="pullFile(row.path)"
            >
              下载
            </el-button>
            <el-button
              size="small"
              type="danger"
              :icon="Delete"
              @click.stop="deleteFile(row.path)"
            >
              删除
            </el-button>
          </el-button-group>
        </template>
      </el-table-column>
    </el-table>
  </div>
</template>

<style scoped>
.file-manager {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.fill-table {
  flex: 1;
  min-height: 0;
}

.apps-container {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}

.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 15px;
  flex-shrink: 0;
}

.panel-header h2 {
  margin: 0;
}

.header-actions {
  display: flex;
  gap: 10px;
}

.breadcrumb {
  margin-bottom: 15px;
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}

.breadcrumb-label {
  color: #606266;
  font-size: 14px;
  white-space: nowrap;
}

.clickable {
  cursor: pointer;
  color: #409EFF;
}

.clickable:hover {
  color: #66b1ff;
}

.empty-state {
  display: flex;
  justify-content: center;
  align-items: center;
  height: 400px;
}

.folder-name {
  color: #409EFF;
  cursor: pointer;
}

.folder-name:hover {
  text-decoration: underline;
}

.apps-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 10px;
  color: #606266;
  font-size: 14px;
  flex-shrink: 0;
}

.app-item {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
}

.app-item:hover {
  color: #409EFF;
}
</style>
