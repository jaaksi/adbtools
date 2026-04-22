<script setup lang="ts">
import { ref, watch, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { ElMessage } from "element-plus";
import { SetUp } from "@element-plus/icons-vue";

const props = defineProps<{
  selectedDevice: string;
}>();

// 4 个开关的当前状态
const wifiEnabled = ref(false);
const layoutBounds = ref(false);
const showTouches = ref(false);
const pointerLocation = ref(false);
const dontKeepActivities = ref(false);
const darkMode = ref(false);
const navigationMode = ref<string>("gestural");
const navModeLoading = ref(false);

const navModeOptions = [
  { label: "手势导航", value: "gestural" },
  { label: "三按键", value: "threebutton" },
  { label: "两按键", value: "twobutton" },
];

// 切换过程中的 loading（避免快速连点）
const wifiLoading = ref(false);
const layoutLoading = ref(false);
const touchLoading = ref(false);
const pointerLoading = ref(false);
const dontKeepLoading = ref(false);
const darkModeLoading = ref(false);

// 统一读取当前设备上的所有开关状态
async function loadAllStates() {
  if (!props.selectedDevice) return;
  try {
    const [wifi, layout, touches, pointer, dontKeep, dark, nav] = await Promise.all([
      invoke<boolean>("get_wifi_enabled", { serial: props.selectedDevice }),
      invoke<boolean>("get_show_layout_bounds", { serial: props.selectedDevice }),
      invoke<boolean>("get_show_touches", { serial: props.selectedDevice }),
      invoke<boolean>("get_pointer_location", { serial: props.selectedDevice }),
      invoke<boolean>("get_always_finish_activities", {
        serial: props.selectedDevice,
      }),
      invoke<boolean>("get_dark_mode", { serial: props.selectedDevice }),
      invoke<string>("get_navigation_mode", { serial: props.selectedDevice }),
    ]);
    wifiEnabled.value = wifi;
    layoutBounds.value = layout;
    showTouches.value = touches;
    pointerLocation.value = pointer;
    dontKeepActivities.value = dontKeep;
    darkMode.value = dark;
    // 未知模式时不更新，避免 segmented 跳回某个选项引起误切换
    if (nav === "gestural" || nav === "threebutton" || nav === "twobutton") {
      navigationMode.value = nav;
    }
  } catch (e) {
    console.error("读取开关状态失败:", e);
  }
}

// 通用切换处理：失败时回滚到旧值并报错
async function toggle(
  cmd: string,
  stateRef: { value: boolean },
  loadingRef: { value: boolean },
  newValue: boolean,
  label: string
) {
  if (!props.selectedDevice) {
    ElMessage.warning("请先选择设备");
    stateRef.value = !newValue;
    return;
  }
  const prev = !newValue;
  loadingRef.value = true;
  try {
    await invoke(cmd, { serial: props.selectedDevice, enabled: newValue });
    ElMessage.success(`${label} 已${newValue ? "开启" : "关闭"}`);
  } catch (e) {
    ElMessage.error(`${label} 切换失败: ${e}`);
    stateRef.value = prev;
  } finally {
    loadingRef.value = false;
  }
}

function onWifiChange(v: boolean | string | number) {
  toggle("set_wifi_enabled", wifiEnabled, wifiLoading, v as boolean, "WiFi");
}
function onLayoutChange(v: boolean | string | number) {
  toggle(
    "set_show_layout_bounds",
    layoutBounds,
    layoutLoading,
    v as boolean,
    "显示布局边界"
  );
}
function onTouchesChange(v: boolean | string | number) {
  toggle(
    "set_show_touches",
    showTouches,
    touchLoading,
    v as boolean,
    "显示触摸位置"
  );
}
function onPointerChange(v: boolean | string | number) {
  toggle(
    "set_pointer_location",
    pointerLocation,
    pointerLoading,
    v as boolean,
    "显示指针位置"
  );
}
function onDontKeepChange(v: boolean | string | number) {
  toggle(
    "set_always_finish_activities",
    dontKeepActivities,
    dontKeepLoading,
    v as boolean,
    "不保留活动"
  );
}
function onDarkModeChange(v: boolean | string | number) {
  toggle("set_dark_mode", darkMode, darkModeLoading, v as boolean, "暗黑模式");
}

async function onNavigationModeChange(v: string | number | boolean) {
  if (!props.selectedDevice) {
    ElMessage.warning("请先选择设备");
    return;
  }
  const next = String(v);
  const prev = navigationMode.value;
  navigationMode.value = next;
  navModeLoading.value = true;
  try {
    await invoke("set_navigation_mode", {
      serial: props.selectedDevice,
      mode: next,
    });
    ElMessage.success(
      `已切换为${navModeOptions.find((o) => o.value === next)?.label ?? next}`
    );
  } catch (e) {
    ElMessage.error(`切换导航模式失败: ${e}`);
    navigationMode.value = prev;
  } finally {
    navModeLoading.value = false;
  }
}

async function openDevOptions() {
  if (!props.selectedDevice) {
    ElMessage.warning("请先选择设备");
    return;
  }
  try {
    await invoke("open_dev_options", { serial: props.selectedDevice });
    ElMessage.success("已在设备上打开开发者选项");
  } catch (e) {
    ElMessage.error(`打开开发者选项失败: ${e}`);
  }
}

onMounted(() => {
  loadAllStates();
});

watch(
  () => props.selectedDevice,
  () => loadAllStates()
);
</script>

<template>
  <div class="quick-toggles-panel">
    <div class="panel-header">
      <h2>快捷开关</h2>
      <el-button size="small" @click="loadAllStates" :disabled="!selectedDevice">
        刷新状态
      </el-button>
    </div>

    <div v-if="!selectedDevice" class="empty-state">
      <el-empty description="请先选择设备" />
    </div>

    <template v-else>
      <el-card class="entry-card">
        <div class="entry-row">
          <div>
            <div class="entry-title">开发者选项</div>
            <div class="entry-desc">在设备上直接打开系统「开发者选项」设置页</div>
          </div>
          <el-button type="primary" :icon="SetUp" @click="openDevOptions">
            打开开发者选项
          </el-button>
        </div>
      </el-card>

      <el-row :gutter="20" class="toggle-grid">
        <el-col :span="12">
          <el-card class="toggle-card">
            <div class="toggle-row">
              <div class="toggle-info">
                <div class="toggle-title">WiFi</div>
                <div class="toggle-desc">通过 svc wifi 开启/关闭设备 WiFi</div>
              </div>
              <el-switch
                v-model="wifiEnabled"
                :loading="wifiLoading"
                @change="onWifiChange"
              />
            </div>
          </el-card>
        </el-col>

        <el-col :span="12">
          <el-card class="toggle-card">
            <div class="toggle-row">
              <div class="toggle-info">
                <div class="toggle-title">显示布局边界</div>
                <div class="toggle-desc">在界面上绘制视图边界（debug.layout）</div>
              </div>
              <el-switch
                v-model="layoutBounds"
                :loading="layoutLoading"
                @change="onLayoutChange"
              />
            </div>
          </el-card>
        </el-col>

        <el-col :span="12" style="margin-top: 20px">
          <el-card class="toggle-card">
            <div class="toggle-row">
              <div class="toggle-info">
                <div class="toggle-title">显示触摸位置</div>
                <div class="toggle-desc">触屏时在屏幕上绘制小圆点（show_touches）</div>
              </div>
              <el-switch
                v-model="showTouches"
                :loading="touchLoading"
                @change="onTouchesChange"
              />
            </div>
          </el-card>
        </el-col>

        <el-col :span="12" style="margin-top: 20px">
          <el-card class="toggle-card">
            <div class="toggle-row">
              <div class="toggle-info">
                <div class="toggle-title">显示指针位置</div>
                <div class="toggle-desc">展示触摸轨迹与坐标（pointer_location）</div>
              </div>
              <el-switch
                v-model="pointerLocation"
                :loading="pointerLoading"
                @change="onPointerChange"
              />
            </div>
          </el-card>
        </el-col>

        <el-col :span="12" style="margin-top: 20px">
          <el-card class="toggle-card">
            <div class="toggle-row">
              <div class="toggle-info">
                <div class="toggle-title">不保留活动</div>
                <div class="toggle-desc">
                  用户离开后立即销毁 Activity（always_finish_activities）
                </div>
              </div>
              <el-switch
                v-model="dontKeepActivities"
                :loading="dontKeepLoading"
                @change="onDontKeepChange"
              />
            </div>
          </el-card>
        </el-col>

        <el-col :span="12" style="margin-top: 20px">
          <el-card class="toggle-card">
            <div class="toggle-row">
              <div class="toggle-info">
                <div class="toggle-title">暗黑模式</div>
                <div class="toggle-desc">
                  切换系统深色主题（cmd uimode night yes/no）
                </div>
              </div>
              <el-switch
                v-model="darkMode"
                :loading="darkModeLoading"
                @change="onDarkModeChange"
              />
            </div>
          </el-card>
        </el-col>

        <el-col :span="24" style="margin-top: 20px">
          <el-card class="toggle-card">
            <div class="toggle-row">
              <div class="toggle-info">
                <div class="toggle-title">导航模式</div>
                <div class="toggle-desc">
                  切换系统导航栏样式（cmd overlay enable-exclusive）
                </div>
              </div>
              <el-segmented
                v-model="navigationMode"
                :options="navModeOptions"
                :disabled="navModeLoading"
                @change="onNavigationModeChange"
              />
            </div>
          </el-card>
        </el-col>
      </el-row>
    </template>
  </div>
</template>

<style scoped>
.quick-toggles-panel {
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

.entry-card {
  margin-bottom: 20px;
}

.entry-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.entry-title {
  font-weight: bold;
  font-size: 15px;
  margin-bottom: 4px;
}

.entry-desc,
.toggle-desc {
  color: #909399;
  font-size: 12px;
}

.toggle-card {
  height: 100%;
}

.toggle-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.toggle-info {
  flex: 1;
  min-width: 0;
}

.toggle-title {
  font-weight: bold;
  font-size: 14px;
  margin-bottom: 4px;
}
</style>
