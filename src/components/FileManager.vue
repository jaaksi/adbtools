<script setup lang="ts">
import { ref, watch, computed, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { ElMessage } from "element-plus";
import { open, save } from "@tauri-apps/plugin-dialog";
import {
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
import { useFavoritesStore } from "../stores/favorites";

const favStore = useFavoritesStore();

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

interface SqliteView {
  absolutePath: string; // 展示用
  package: string;
  relativePath: string; // run-as 下相对 app data 根的相对路径
  tables: string[];
  currentTable: string;
  columns: string[];
  rows: string[][];
  loadingTables: boolean;
  loadingRows: boolean;
  limit: number;
  offset: number;
  search: string;
  error: string;
}

const sqliteView = ref<SqliteView | null>(null);
const SQLITE_EXTS = ["db", "sqlite", "sqlite3"];

function isSqliteFile(name: string): boolean {
  const ext = name.toLowerCase().split(".").pop() ?? "";
  return SQLITE_EXTS.includes(ext);
}

// 从 currentPath + row 推出 run-as 需要的 (pkg, 相对路径)
function resolvePackageAndRel(rowPath: string): { pkg: string; rel: string } | null {
  const abs = resolveRemotePath(rowPath);
  if (!abs.startsWith("/data/data/")) return null;
  const after = abs.substring(11);
  const slashIdx = after.indexOf("/");
  if (slashIdx === -1) return null;
  const pkg = after.substring(0, slashIdx);
  const rel = after.substring(slashIdx + 1);
  return { pkg, rel };
}

async function openSqliteView(row: FileInfo) {
  const info = resolvePackageAndRel(row.path);
  if (!info) {
    ElMessage.error("只能查看调试应用 data 目录下的数据库");
    return;
  }
  previewTarget.value = row;
  preview.value = null;
  previewError.value = "";
  spSearch.value = "";
  previewVisible.value = true;
  sqliteView.value = {
    absolutePath: resolveRemotePath(row.path),
    package: info.pkg,
    relativePath: info.rel,
    tables: [],
    currentTable: "",
    columns: [],
    rows: [],
    loadingTables: true,
    loadingRows: false,
    limit: 100,
    offset: 0,
    search: "",
    error: "",
  };
  try {
    const tables = await invoke<string[]>("sqlite_list_tables", {
      serial: props.selectedDevice,
      package: info.pkg,
      dbPath: info.rel,
    });
    if (!sqliteView.value) return;
    sqliteView.value.tables = tables;
    if (tables.length > 0) {
      await loadSqliteTable(tables[0]);
    }
  } catch (e) {
    if (sqliteView.value) sqliteView.value.error = String(e);
  } finally {
    if (sqliteView.value) sqliteView.value.loadingTables = false;
  }
}

async function loadSqliteTable(table: string) {
  if (!sqliteView.value) return;
  sqliteView.value.currentTable = table;
  sqliteView.value.offset = 0;
  sqliteView.value.search = "";
  await queryCurrentSqliteTable();
}

function onSqliteSearchChange() {
  if (!sqliteView.value) return;
  sqliteView.value.offset = 0;
  queryCurrentSqliteTable();
}

async function queryCurrentSqliteTable() {
  if (!sqliteView.value || !sqliteView.value.currentTable) return;
  sqliteView.value.loadingRows = true;
  sqliteView.value.error = "";
  try {
    const res = await invoke<{ columns: string[]; rows: string[][] }>(
      "sqlite_query_table",
      {
        serial: props.selectedDevice,
        package: sqliteView.value.package,
        dbPath: sqliteView.value.relativePath,
        table: sqliteView.value.currentTable,
        limit: sqliteView.value.limit,
        offset: sqliteView.value.offset,
        search: sqliteView.value.search || null,
      }
    );
    sqliteView.value.columns = res.columns;
    sqliteView.value.rows = res.rows;
  } catch (e) {
    sqliteView.value.error = String(e);
    sqliteView.value.columns = [];
    sqliteView.value.rows = [];
  } finally {
    sqliteView.value.loadingRows = false;
  }
}

function sqlitePageNext() {
  if (!sqliteView.value) return;
  sqliteView.value.offset += sqliteView.value.limit;
  queryCurrentSqliteTable();
}

function sqlitePagePrev() {
  if (!sqliteView.value) return;
  sqliteView.value.offset = Math.max(0, sqliteView.value.offset - sqliteView.value.limit);
  queryCurrentSqliteTable();
}

// 双击单元格复制字段值
async function onSqliteCellDblClick(
  _row: string[],
  column: { property?: string }
) {
  if (!sqliteView.value || !column?.property) return;
  const idx = parseInt(column.property, 10);
  if (!Number.isInteger(idx)) return;
  const value = _row[idx] ?? "";
  try {
    await navigator.clipboard.writeText(value);
    ElMessage.success(`已复制${value ? "" : "（空值）"}`);
  } catch (e) {
    ElMessage.error(`复制失败: ${e}`);
  }
}

const previewVisible = ref(false);
const previewLoading = ref(false);
const previewTarget = ref<FileInfo | null>(null);
const preview = ref<FilePreview | null>(null);
const previewError = ref("");

async function openPreview(row: FileInfo) {
  // SQLite 数据库走独立流程（设备端 sqlite3）
  if (isSqliteFile(row.name)) {
    await openSqliteView(row);
    return;
  }
  // 打开前先清理上一份临时副本
  if (preview.value?.temp_path) {
    try {
      await invoke("cleanup_preview_temp", { tempPath: preview.value.temp_path });
    } catch (e) {
      console.warn("清理上次临时文件失败:", e);
    }
  }
  sqliteView.value = null;
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
  sqliteView.value = null;
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

// 应用包名搜索的默认值规则：
// - localStorage 有非空值 → 用上次的输入
// - 否则（null 或 ""）→ 回退到全局"最近一次关注"的包名
onMounted(() => {
  const stored = localStorage.getItem("adb_tool_app_search");
  if (stored) {
    searchQuery.value = stored;
  } else if (favStore.lastFavorited) {
    searchQuery.value = favStore.lastFavorited;
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
    const msg = String(error);
    ElMessage.error(`获取文件列表失败：${error}`);
    files.value = [];
    // 路径不存在 → 自动回退一级，避免用户卡在坏路径无法离开
    if (/No such file or directory|does not exist/i.test(msg) && pathHistory.value.length > 0) {
      const prev = pathHistory.value.pop()!;
      currentPath.value = prev;
      // 不再递归 loadFiles，防止上一层同样坏导致死循环
    }
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

      <div
        v-if="sqliteView"
        v-loading="sqliteView.loadingTables"
        class="preview-body preview-sqlite"
      >
        <el-alert
          v-if="sqliteView.error"
          type="error"
          :closable="false"
          show-icon
          :title="sqliteView.error"
        />
        <template v-else>
          <div class="sqlite-toolbar">
            <span class="sqlite-label">表</span>
            <el-select
              v-model="sqliteView.currentTable"
              :disabled="sqliteView.loadingTables || sqliteView.tables.length === 0"
              placeholder="选择表"
              style="width: 260px"
              @change="loadSqliteTable"
            >
              <el-option
                v-for="t in sqliteView.tables"
                :key="t"
                :label="t"
                :value="t"
              />
            </el-select>
            <span class="sqlite-meta">
              共 {{ sqliteView.tables.length }} 张表
            </span>
          </div>
          <div class="sqlite-toolbar">
            <el-input
              v-model="sqliteView.search"
              :placeholder="`按首列${sqliteView.columns[0] ? '「' + sqliteView.columns[0] + '」' : ''}搜索`"
              clearable
              :prefix-icon="Search"
              style="width: 280px"
              @keyup.enter="onSqliteSearchChange"
              @clear="onSqliteSearchChange"
            />
            <el-button
              size="small"
              type="primary"
              :disabled="sqliteView.loadingRows"
              @click="onSqliteSearchChange"
            >
              搜索
            </el-button>
          </div>
          <div class="sqlite-toolbar">
            <el-button
              size="small"
              :disabled="sqliteView.offset === 0 || sqliteView.loadingRows"
              @click="sqlitePagePrev"
            >
              上一页
            </el-button>
            <span class="sqlite-meta">
              第 {{ Math.floor(sqliteView.offset / sqliteView.limit) + 1 }} 页 · 每页
              {{ sqliteView.limit }} 条 · 当前 {{ sqliteView.rows.length }} 行
            </span>
            <el-button
              size="small"
              :disabled="sqliteView.rows.length < sqliteView.limit || sqliteView.loadingRows"
              @click="sqlitePageNext"
            >
              下一页
            </el-button>
          </div>
          <el-table
            v-loading="sqliteView.loadingRows"
            :data="sqliteView.rows"
            size="small"
            stripe
            border
            empty-text="该表没有数据"
            max-height="calc(88vh - 280px)"
            class="sqlite-rows-table"
            @cell-dblclick="onSqliteCellDblClick"
          >
            <el-table-column
              v-for="(col, idx) in sqliteView.columns"
              :key="col"
              :prop="`${idx}`"
              :label="col"
              min-width="140"
              show-overflow-tooltip
            >
              <template #default="{ row }">{{ row[idx] }}</template>
            </el-table-column>
          </el-table>
        </template>
      </div>

      <div v-else v-loading="previewLoading" class="preview-body">
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

.preview-sqlite {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.sqlite-toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.sqlite-label {
  font-size: 13px;
  color: var(--el-text-color-regular);
}

.sqlite-meta {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}

/* 数据行暗示可点击（双击复制） */
:deep(.sqlite-rows-table) .el-table__row td {
  user-select: text;
  cursor: pointer;
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
