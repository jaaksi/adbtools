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
  CopyDocument,
  FolderOpened,
  Close,
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
const fileSearchQuery = ref("");

// 普通目录下的文件按名称过滤
const filteredFiles = computed(() => {
  const q = fileSearchQuery.value.trim().toLowerCase();
  if (!q) return files.value;
  return files.value.filter((f) => f.name.toLowerCase().includes(q));
});

// ---- 文件预览浮层 ----
interface FilePreview {
  kind: "text" | "image" | "binary";
  mime: string;
  size: number;
  text: string | null;
  data_url: string | null;
  temp_path: string;
}

const previewVisible = ref(false);
const previewLoading = ref(false);
const previewTarget = ref<FileInfo | null>(null);
const preview = ref<FilePreview | null>(null);
const previewError = ref("");

async function openPreview(row: FileInfo) {
  // 打开前先清理上一份临时副本
  if (preview.value?.temp_path) {
    try {
      await invoke("cleanup_preview_temp", { tempPath: preview.value.temp_path });
    } catch (e) {
      console.warn("清理上次临时文件失败:", e);
    }
  }
  previewTarget.value = row;
  preview.value = null;
  previewError.value = "";
  spSearch.value = "";
  previewVisible.value = true;
  previewLoading.value = true;
  try {
    preview.value = await invoke<FilePreview>("preview_remote_file", {
      serial: props.selectedDevice,
      remotePath: resolveRemotePath(row.path),
    });
  } catch (e) {
    previewError.value = String(e);
  } finally {
    previewLoading.value = false;
  }
}

async function onPreviewClosed() {
  if (preview.value?.temp_path) {
    try {
      await invoke("cleanup_preview_temp", { tempPath: preview.value.temp_path });
    } catch (e) {
      console.warn("关闭浮层清理临时文件失败:", e);
    }
  }
  preview.value = null;
  previewTarget.value = null;
  previewError.value = "";
}

async function copyPreviewText() {
  if (!preview.value?.text) return;
  try {
    await navigator.clipboard.writeText(preview.value.text);
    ElMessage.success("已复制到剪贴板");
  } catch (e) {
    ElMessage.error(`复制失败: ${e}`);
  }
}

async function downloadFromPreview() {
  if (!preview.value || !previewTarget.value) return;
  const savePath = await save({
    defaultPath: previewTarget.value.name,
  });
  if (!savePath) return;
  try {
    await invoke("copy_local_file", {
      src: preview.value.temp_path,
      dest: savePath,
    });
    ElMessage.success("已保存到本地");
    try {
      await invoke("reveal_in_folder", { path: savePath });
    } catch (e) {
      console.error("定位文件失败:", e);
    }
  } catch (e) {
    ElMessage.error(`保存失败: ${e}`);
  }
}

async function revealPreviewTemp() {
  if (!preview.value?.temp_path) return;
  try {
    await invoke("reveal_in_folder", { path: preview.value.temp_path });
  } catch (e) {
    ElMessage.error(`定位临时文件失败: ${e}`);
  }
}

function formatPreviewSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(2)} MB`;
}

// ---- SharedPreferences XML 解析（<map>...</map>） ----
interface SpEntry {
  key: string;
  value: string;
  type: string;
}

const spSearch = ref("");

const spEntries = computed<SpEntry[] | null>(() => {
  if (!preview.value || preview.value.kind !== "text" || !preview.value.text) {
    return null;
  }
  const text = preview.value.text.trim();
  // 简单识别：内容是 Android SP 的 <map> 文档（可能带 XML 声明）
  if (!/^<\?xml[^>]*\?>/.test(text) && !text.startsWith("<map")) {
    return null;
  }
  if (!/<map[\s>]/i.test(text)) return null;

  try {
    const doc = new DOMParser().parseFromString(text, "text/xml");
    const parserError = doc.querySelector("parsererror");
    if (parserError) return null;
    const map = doc.querySelector("map");
    if (!map) return null;

    const entries: SpEntry[] = [];
    for (const el of Array.from(map.children)) {
      const tag = el.tagName.toLowerCase();
      const key = el.getAttribute("name") ?? "";
      let value = "";
      let type = tag;
      switch (tag) {
        case "string":
          value = el.textContent ?? "";
          break;
        case "boolean":
        case "int":
        case "long":
        case "float":
          value = el.getAttribute("value") ?? "";
          break;
        case "set":
          value = Array.from(el.children)
            .map((c) => c.textContent ?? "")
            .join(", ");
          type = "set<string>";
          break;
        default:
          value = el.outerHTML;
      }
      entries.push({ key, value, type });
    }
    return entries;
  } catch {
    return null;
  }
});

const filteredSpEntries = computed(() => {
  if (!spEntries.value) return [];
  const q = spSearch.value.trim().toLowerCase();
  if (!q) return spEntries.value;
  return spEntries.value.filter(
    (e) =>
      e.key.toLowerCase().includes(q) ||
      e.value.toLowerCase().includes(q)
  );
});

async function copySpValue(row: SpEntry) {
  try {
    await navigator.clipboard.writeText(row.value);
    ElMessage.success(`已复制 ${row.key}`);
  } catch (e) {
    ElMessage.error(`复制失败: ${e}`);
  }
}

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
  // 切换目录时清空文件名搜索，避免上一级的查询影响当前目录
  fileSearchQuery.value = "";
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

// /data/data/<pkg>/ 下 run_as_list_files 返回的是相对路径（相对 app data 目录），
// adb pull / delete / preview 需要绝对路径，这里根据 currentPath 补齐前缀
function resolveRemotePath(path: string): string {
  if (path.startsWith("/")) return path;
  if (!currentPath.value.startsWith("/data/data/")) return path;
  const afterData = currentPath.value.substring(11); // "<pkg>" 或 "<pkg>/<sub>"
  const slashIdx = afterData.indexOf("/");
  const pkg = slashIdx === -1 ? afterData : afterData.substring(0, slashIdx);
  return `/data/data/${pkg}/${path}`;
}

async function navigateTo(path: string) {
  pathHistory.value.push(currentPath.value);

  if (currentPath.value.startsWith("/data/data/")) {
    // run-as 视图下：path 已经是相对 app data 根目录的完整相对路径
    // （parse_file_list 会把 base_path 拼进去，例如当前在 "files"，子项 "actions" → "files/actions"）
    // 所以这里直接用 path 覆盖子路径，不再与 currentSubPath 拼接，避免路径重复
    const afterData = currentPath.value.substring(11);
    const slashIdx = afterData.indexOf("/");
    const packageName = slashIdx === -1 ? afterData : afterData.substring(0, slashIdx);
    const cleanPath = path.startsWith("/") ? path.substring(1) : path;
    currentPath.value = `/data/data/${packageName}/${cleanPath}`;
  } else if (path === "/data") {
    // /data 在非 root 设备无法直接 ls，统一跳到应用列表（/data/data）
    currentPath.value = "/data/data";
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

  const absolutePath = resolveRemotePath(remotePath);
  const savePath = await save({
    defaultPath: absolutePath.split("/").pop(),
  });

  if (savePath) {
    try {
      loading.value = true;
      const result = await invoke("pull_file", {
        serial: props.selectedDevice,
        remotePath: absolutePath,
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
      remotePath: resolveRemotePath(path),
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
      <div class="breadcrumb-segments">
        <template v-for="(segment, index) in pathSegments" :key="index">
          <span v-if="index > 0" class="breadcrumb-sep">/</span>
          <span
            class="breadcrumb-seg"
            :class="{ 'is-current': index === pathSegments.length - 1 }"
            @click="navigateToSegment(index)"
          >
            {{ segment.name || '根目录' }}
          </span>
        </template>
      </div>
    </div>

    <div v-if="!isDataDataView" class="file-search-row">
      <el-input
        v-model="fileSearchQuery"
        placeholder="搜索当前目录下的文件名"
        :prefix-icon="Search"
        clearable
        style="max-width: 320px"
      />
      <span v-if="fileSearchQuery" class="file-search-meta">
        匹配 {{ filteredFiles.length }} / {{ files.length }} 项
      </span>
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
      :data="filteredFiles"
      class="fill-table"
      style="width: 100%"
      height="100%"
      row-class-name="file-row-clickable"
      @row-click="(row: FileInfo) => row.is_dir ? navigateTo(row.path) : openPreview(row)"
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

    <!-- 文件预览浮层 -->
    <el-dialog
      v-model="previewVisible"
      :title="previewTarget ? `查看 · ${previewTarget.name}` : '查看'"
      width="760px"
      top="6vh"
      class="preview-dialog"
      append-to-body
      destroy-on-close
      @closed="onPreviewClosed"
    >
      <div v-if="previewTarget" class="preview-path">{{ previewTarget.path }}</div>

      <div v-loading="previewLoading" class="preview-body">
        <el-alert
          v-if="previewError"
          type="error"
          :closable="false"
          show-icon
          :title="previewError"
        />
        <template v-else-if="preview">
          <div class="preview-meta">
            {{ preview.mime }} · {{ formatPreviewSize(preview.size) }}
          </div>
          <div v-if="preview.kind === 'text' && spEntries" class="preview-sp">
            <el-input
              v-model="spSearch"
              placeholder="搜索 key 或 value"
              clearable
              class="sp-search"
              :prefix-icon="Search"
            />
            <div class="sp-meta">
              共 {{ spEntries.length }} 项
              <template v-if="spSearch">
                · 匹配 {{ filteredSpEntries.length }} 项
              </template>
            </div>
            <el-table
              :data="filteredSpEntries"
              size="small"
              stripe
              class="preview-sp-table"
              max-height="calc(88vh - 260px)"
              empty-text="没有匹配的条目"
              @row-click="copySpValue"
            >
              <el-table-column prop="key" label="Key" min-width="160" show-overflow-tooltip />
              <el-table-column prop="value" label="Value" min-width="220" show-overflow-tooltip />
              <el-table-column prop="type" label="Type" width="110" />
            </el-table>
          </div>
          <pre
            v-else-if="preview.kind === 'text'"
            class="preview-text"
          >{{ preview.text }}</pre>
          <div v-else-if="preview.kind === 'image'" class="preview-image-wrap">
            <img :src="preview.data_url || ''" class="preview-image" />
          </div>
          <el-empty
            v-else
            description="此文件为二进制，不支持在线预览"
          />
        </template>
      </div>

      <template #footer>
        <el-button
          v-if="preview?.kind === 'text'"
          :icon="CopyDocument"
          @click="copyPreviewText"
        >
          复制全文
        </el-button>
        <el-button
          :icon="Download"
          :disabled="!preview"
          @click="downloadFromPreview"
        >
          下载到本地
        </el-button>
        <el-button
          :icon="FolderOpened"
          :disabled="!preview"
          @click="revealPreviewTemp"
        >
          在访达中显示
        </el-button>
        <el-button :icon="Close" @click="previewVisible = false">关闭</el-button>
      </template>
    </el-dialog>
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

:deep(.file-row-clickable) {
  cursor: pointer;
}

.preview-path {
  font-family: "Courier New", monospace;
  font-size: 12px;
  color: var(--el-text-color-secondary);
  margin-bottom: 8px;
  word-break: break-all;
}

.preview-meta {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  margin-bottom: 8px;
}

.preview-body {
  min-height: 120px;
}

.sp-search {
  margin-bottom: 8px;
}

.sp-meta {
  font-size: 12px;
  color: var(--el-text-color-secondary);
  margin-bottom: 8px;
}

.preview-text {
  margin: 0;
  max-height: 60vh;
  overflow: auto;
  padding: 12px;
  background: #1e1e1e;
  color: #d4d4d4;
  font-family: "Courier New", monospace;
  font-size: 13px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-all;
  border-radius: 4px;
}

.preview-image-wrap {
  text-align: center;
  max-height: 60vh;
  overflow: auto;
  background: repeating-conic-gradient(#f5f5f5 0 25%, #fff 0 50%) 0 0 / 20px 20px;
  padding: 12px;
  border-radius: 4px;
}

.preview-image {
  max-width: 100%;
  max-height: 60vh;
  object-fit: contain;
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

.file-search-row {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
  flex-shrink: 0;
}

.file-search-meta {
  font-size: 12px;
  color: var(--el-text-color-secondary);
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

.breadcrumb-segments {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 4px;
  font-size: 14px;
}

.breadcrumb-seg {
  cursor: pointer;
  color: #303133;
  white-space: nowrap;
}

.breadcrumb-seg:hover {
  color: #409EFF;
}

.breadcrumb-seg.is-current {
  color: #909399;
  cursor: default;
}

.breadcrumb-seg.is-current:hover {
  color: #909399;
}

.breadcrumb-sep {
  color: #c0c4cc;
  user-select: none;
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

<style>
/* 预览浮层（append-to-body 会把 dialog 挪出组件，scoped 样式够不到，这里用全局样式） */
.preview-dialog.el-dialog {
  display: flex;
  flex-direction: column;
  max-height: 88vh;
  overflow: hidden;
}

.preview-dialog .el-dialog__header {
  flex-shrink: 0;
}

/* body 自身不滚；内部由 el-table 或 <pre> 自己处理滚动 */
.preview-dialog .el-dialog__body {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.preview-dialog .el-dialog__footer {
  flex-shrink: 0;
  border-top: 1px solid var(--el-border-color-lighter);
}

</style>
