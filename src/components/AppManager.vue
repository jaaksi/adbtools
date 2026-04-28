<script setup lang="ts">
import { ref, watch, computed } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { ElMessage, ElMessageBox } from "element-plus";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Search,
  Plus,
  Refresh,
  VideoPlay,
  VideoPause,
  Delete,
  Close,
  Key,
} from "@element-plus/icons-vue";

interface AppInfo {
  package_name: string;
  app_name?: string;
  version_name?: string;
  version_code?: string;
  is_system_app: boolean;
}

interface PermissionInfo {
  name: string;
  granted: boolean;
  flags: string;
}

const props = defineProps<{
  selectedDevice: string;
}>();

const apps = ref<AppInfo[]>([]);
const loading = ref(false);
const filter = ref("third");
const searchQuery = ref("");

async function loadApps() {
  if (!props.selectedDevice) {
    apps.value = [];
    return;
  }
  loading.value = true;
  try {
    const filterType = filter.value === "all" ? null : filter.value;
    apps.value = await invoke("get_installed_apps", {
      serial: props.selectedDevice,
      filter: filterType,
    });
  } catch (error) {
    console.error("Failed to get apps:", error);
    ElMessage.error("获取应用列表失败");
  } finally {
    loading.value = false;
  }
}

async function installApk() {
  if (!props.selectedDevice) {
    ElMessage.warning("请先选择设备");
    return;
  }

  const file = await open({
    multiple: false,
    filters: [{ name: "APK", extensions: ["apk"] }],
  });

  if (file) {
    try {
      loading.value = true;
      const result = await invoke("install_apk", {
        serial: props.selectedDevice,
        apkPath: file,
      });
      ElMessage.success(result as string);
      loadApps();
    } catch (error) {
      ElMessage.error(`安装失败: ${error}`);
    } finally {
      loading.value = false;
    }
  }
}

async function uninstallApp(packageName: string) {
  if (!props.selectedDevice) return;

  try {
    await ElMessageBox.confirm(
      `确定要卸载 ${packageName} 吗？`,
      "确认卸载",
      { type: "warning" }
    );

    loading.value = true;
    const result = await invoke("uninstall_app", {
      serial: props.selectedDevice,
      packageName,
    });
    ElMessage.success(result as string);
    loadApps();
  } catch (error: any) {
    if (error !== "cancel") {
      ElMessage.error(`卸载失败: ${error}`);
    }
  } finally {
    loading.value = false;
  }
}

async function startApp(packageName: string) {
  if (!props.selectedDevice) return;

  try {
    await invoke("start_app", {
      serial: props.selectedDevice,
      packageName,
    });
    ElMessage.success("应用已启动");
  } catch (error) {
    ElMessage.error(`启动失败: ${error}`);
  }
}

async function stopApp(packageName: string) {
  if (!props.selectedDevice) return;

  try {
    await invoke("stop_app", {
      serial: props.selectedDevice,
      packageName,
    });
    ElMessage.success("应用已停止");
  } catch (error) {
    ElMessage.error(`停止失败: ${error}`);
  }
}

async function clearAppData(packageName: string) {
  if (!props.selectedDevice) return;

  try {
    await ElMessageBox.confirm(
      `确定要清除 ${packageName} 的数据吗？`,
      "确认清除数据",
      { type: "warning" }
    );

    await invoke("clear_app_data", {
      serial: props.selectedDevice,
      packageName,
    });
    ElMessage.success("数据已清除");
  } catch (error: any) {
    if (error !== "cancel") {
      ElMessage.error(`清除失败: ${error}`);
    }
  }
}

const filteredApps = computed(() => {
  if (!searchQuery.value) return apps.value;
  return apps.value.filter((app) =>
    app.package_name.toLowerCase().includes(searchQuery.value.toLowerCase())
  );
});

// 权限弹窗
const permDialogVisible = ref(false);
const permTargetPackage = ref("");
const permissions = ref<PermissionInfo[]>([]);
const permLoading = ref(false);
const permSearch = ref("");

const filteredPermissions = computed(() => {
  const q = permSearch.value.trim().toLowerCase();
  if (!q) return permissions.value;
  return permissions.value.filter((p) => p.name.toLowerCase().includes(q));
});

async function openPermissions(pkg: string) {
  permTargetPackage.value = pkg;
  permDialogVisible.value = true;
  permSearch.value = "";
  await loadPermissions();
}

async function loadPermissions() {
  if (!props.selectedDevice || !permTargetPackage.value) return;
  permLoading.value = true;
  try {
    permissions.value = await invoke<PermissionInfo[]>(
      "list_runtime_permissions",
      { serial: props.selectedDevice, package: permTargetPackage.value }
    );
  } catch (e) {
    ElMessage.error(`获取权限失败: ${e}`);
    permissions.value = [];
  } finally {
    permLoading.value = false;
  }
}

async function togglePermission(perm: PermissionInfo, granted: boolean) {
  const prev = perm.granted;
  perm.granted = granted;
  try {
    await invoke("set_permission", {
      serial: props.selectedDevice,
      package: permTargetPackage.value,
      permission: perm.name,
      granted,
    });
    ElMessage.success(`${perm.name.split(".").pop()} ${granted ? "已授予" : "已撤销"}`);
  } catch (e) {
    ElMessage.error(`${granted ? "授予" : "撤销"}失败: ${e}`);
    perm.granted = prev;
  }
}

async function bulkSetPermissions(granted: boolean) {
  if (!permTargetPackage.value) return;
  const targets = filteredPermissions.value.filter((p) => p.granted !== granted);
  if (targets.length === 0) {
    ElMessage.info(granted ? "已全部处于授予状态" : "已全部处于撤销状态");
    return;
  }
  permLoading.value = true;
  let ok = 0,
    fail = 0;
  for (const p of targets) {
    try {
      await invoke("set_permission", {
        serial: props.selectedDevice,
        package: permTargetPackage.value,
        permission: p.name,
        granted,
      });
      p.granted = granted;
      ok++;
    } catch (e) {
      console.error(e);
      fail++;
    }
  }
  permLoading.value = false;
  if (fail === 0) {
    ElMessage.success(`已${granted ? "授予" : "撤销"} ${ok} 项权限`);
  } else {
    ElMessage.warning(`${granted ? "授予" : "撤销"} ${ok} 项成功, ${fail} 项失败`);
  }
}

watch(() => props.selectedDevice, loadApps, { immediate: true });
watch(filter, loadApps);
</script>

<template>
  <div class="app-manager">
    <div class="panel-header">
      <h2>应用管理</h2>
      <div class="header-actions">
        <el-input
          v-model="searchQuery"
          placeholder="搜索应用包名"
          style="width: 200px"
          :prefix-icon="Search"
          clearable
        />
        <el-select v-model="filter" style="width: 120px">
          <el-option label="全部应用" value="all" />
          <el-option label="系统应用" value="system" />
          <el-option label="第三方应用" value="third" />
        </el-select>
        <el-button type="primary" :icon="Plus" @click="installApk">
          安装 APK
        </el-button>
        <el-button :icon="Refresh" @click="loadApps">刷新</el-button>
      </div>
    </div>

    <div v-if="!selectedDevice" class="empty-state">
      <el-empty description="请先选择设备" />
    </div>

    <el-table
      v-else
      v-loading="loading"
      :data="filteredApps"
      class="fill-table"
      style="width: 100%"
      height="100%"
    >
      <el-table-column prop="package_name" label="包名" min-width="250" />
      <el-table-column label="类型" width="100">
        <template #default="{ row }">
          <el-tag :type="row.is_system_app ? 'danger' : 'success'" size="small">
            {{ row.is_system_app ? "系统" : "第三方" }}
          </el-tag>
        </template>
      </el-table-column>
      <el-table-column label="操作" width="360" fixed="right">
        <template #default="{ row }">
          <el-button-group>
            <el-button size="small" :icon="VideoPlay" @click="startApp(row.package_name)">
              启动
            </el-button>
            <el-button size="small" :icon="VideoPause" @click="stopApp(row.package_name)">
              停止
            </el-button>
            <el-button size="small" :icon="Key" @click="openPermissions(row.package_name)">
              权限
            </el-button>
            <el-button size="small" :icon="Delete" @click="clearAppData(row.package_name)">
              清除
            </el-button>
            <el-button
              size="small"
              type="danger"
              :icon="Close"
              @click="uninstallApp(row.package_name)"
            >
              卸载
            </el-button>
          </el-button-group>
        </template>
      </el-table-column>
    </el-table>

    <!-- 权限管理弹窗 -->
    <el-dialog
      v-model="permDialogVisible"
      :title="`权限 · ${permTargetPackage}`"
      width="720px"
      append-to-body
    >
      <div class="perm-toolbar">
        <el-input
          v-model="permSearch"
          placeholder="搜索权限名"
          :prefix-icon="Search"
          clearable
          style="width: 260px"
        />
        <span class="perm-meta">
          共 {{ permissions.length }} 项
          <template v-if="permSearch">· 匹配 {{ filteredPermissions.length }} 项</template>
        </span>
        <el-button size="small" @click="loadPermissions" :loading="permLoading">
          刷新
        </el-button>
        <el-button size="small" type="success" @click="bulkSetPermissions(true)">
          全部授予
        </el-button>
        <el-button size="small" type="danger" @click="bulkSetPermissions(false)">
          全部撤销
        </el-button>
      </div>
      <el-table
        v-loading="permLoading"
        :data="filteredPermissions"
        size="small"
        stripe
        max-height="60vh"
        empty-text="未检测到 runtime 权限（或应用不存在）"
      >
        <el-table-column prop="name" label="权限" min-width="320" show-overflow-tooltip />
        <el-table-column label="状态" width="140">
          <template #default="{ row }">
            <el-switch
              :model-value="row.granted"
              active-text="已授予"
              inactive-text="已撤销"
              inline-prompt
              @change="(v: boolean) => togglePermission(row, v)"
            />
          </template>
        </el-table-column>
        <el-table-column prop="flags" label="Flags" show-overflow-tooltip />
      </el-table>
      <template #footer>
        <el-button @click="permDialogVisible = false">关闭</el-button>
      </template>
    </el-dialog>
  </div>
</template>

<style scoped>
.app-manager {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.perm-toolbar {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 12px;
  flex-wrap: wrap;
}

.perm-meta {
  font-size: 12px;
  color: var(--el-text-color-secondary);
}

.fill-table {
  flex: 1;
  min-height: 0;
}

.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 20px;
  flex-shrink: 0;
}

.panel-header h2 {
  margin: 0;
}

.header-actions {
  display: flex;
  gap: 10px;
  align-items: center;
}

.empty-state {
  display: flex;
  justify-content: center;
  align-items: center;
  height: 400px;
}
</style>
