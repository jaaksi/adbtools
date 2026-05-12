# 我用AI基于 Tauri + Vue 3 写了个 ADB 桌面工具，把命令行的脏活全干了

GitHub 仓库：https://github.com/jaaksi/adbtools （欢迎 Star ⭐）

---

## 一、为什么又造一个 ADB 工具？

如果你是 Android 开发者，下面这些命令你一定不陌生：

```bash
adb devices
adb install -r app-debug.apk
adb shell pm list packages | grep com.xxx
adb shell am start -n com.xxx/.MainActivity
adb shell dumpsys activity activities | grep mResumed
adb shell screencap -p /sdcard/x.png && adb pull /sdcard/x.png
adb shell run-as com.xxx ls /data/data/com.xxx/databases
...
```

每一条都不长，但加起来一天敲个上百次，**手指会先于大脑罢工**。

市面上不是没有 GUI：scrcpy、Vysor、Android Studio 的 Device Explorer……但它们要么只解决一件事（投屏），要么藏在 IDE 里启动巨慢，要么是个 Web 应用还要开 Java 后台。

我想要的是：

- **桌面原生应用**，开机就能用，不要 Electron 那 200MB
- **覆盖日常 95% 的 adb 操作**，不用再切终端
- **无线连接友好**，断了能一键重连
- **能直接看 SQLite**，不用再 `adb pull` 出来再用 DB Browser 打开

找了一圈没有完全合心意的，索性自己用AI写了一个：**ADB Tools**。

---

## 二、长什么样？

### 1. 设备管理（自动发现 USB / 无线设备）

![设备管理](https://github.com/jaaksi/adbtools/raw/master/docs/screenshots/device_manager.webp)

显示型号、状态、分辨率、密度、smallestWidth dp——做适配的同学应该秒懂这有多省事。支持拖拽安装，签名、版本冲突自动弹窗提示卸载重装，再也不用手动 `adb uninstall`。

### 2. 应用管理（装包、卸载、清数据、看包名）

![应用管理](https://github.com/jaaksi/adbtools/raw/master/docs/screenshots/app_manager.webp)

### 3. 文件管理（带应用文件查看器）

![文件管理](https://github.com/jaaksi/adbtools/raw/master/docs/screenshots/file_manager.webp)

非 root 设备会自动 `run-as <pkg>` 进沙盒目录，可以无障碍地浏览应用私有数据。

更香的是**内置文件查看器**——点开就能看，不用 `adb pull` 到本地再找工具：

![文件查看器](https://github.com/jaaksi/adbtools/raw/master/docs/screenshots/file_viewer.webp)

- **`.db`**：直接打开 SQLite，看表结构和数据，**支持按 key 搜索**记录
- **`.xml`**：高亮渲染，**支持按 key 搜索**——调 SharedPreferences 简直降维打击
- **`.txt` / `.json` / `.log`**：直接显示文本
- **图片**：`.png` / `.jpg` / `.webp` 直接预览

举个最常见的场景：**调 A/B 测试开关、看登录 token、确认埋点参数有没有写进 SharedPreferences**。传统流程是：

1. 记住 xml 路径 `/data/data/<pkg>/shared_prefs/xxx.xml`
2. `adb pull` 拉到本地
3. 用编辑器打开
4. `Ctrl + F` 搜 key
5. 看完还得记得删掉本地副本

现在直接在工具里点开 xml，搜索框输入 key 名字，结果即时高亮——**3 秒搞定，本地工作目录不留垃圾**。这个功能我用得最爽。

### 4. 实用工具（截图、录屏、TopActivity、Firebase 调试埋点）

![实用工具](https://github.com/jaaksi/adbtools/raw/master/docs/screenshots/utility_tools.webp)

### 5. 快捷开关（深色模式、布局边界、指针位置、导航模式…）

![快捷开关](https://github.com/jaaksi/adbtools/raw/master/docs/screenshots/quick_switch.webp)

---

## 三、为什么选 Tauri 而不是 Electron？

直接对比下我关心的几个点：

| 维度     | Electron   | Tauri 2                      |
| ------ | ---------- | ---------------------------- |
| 包体积    | 100MB+     | **10MB 左右**                  |
| 内存占用   | 高          | 显著更低                         |
| 启动速度   | 慢          | **几乎瞬开**                     |
| 后端语言   | Node.js    | **Rust**                     |
| 系统 API | 通过 Node 模块 | 通过 `#[tauri::command]` 暴露给前端 |

对一个"经常调起又关闭"的工具类应用，**启动速度和包体积是决定我用不用的关键**。Tauri 用系统 WebView（macOS 是 WKWebView，Windows 是 WebView2），加上 Rust 后端，体积和性能都吊打 Electron。

代价是：**Rust 的学习曲线**。但对这种"前端调命令行"的小工具，Rust 端代码并不复杂；而且对我这种 Rust 零基础的人来说，这点学习成本完全可以交给 AI——下一节细说。

---

## 四、和 AI 协作写这个项目，是什么体验？

我的真实情况：

- **Rust 零基础**：`Result<T, E>`、`&str` 和 `String` 的区别一开始都搞不清
- **Vue 半吊子**：写过简单页面，但碰到 Pinia、组合式 API 经常要查文档
- **Tauri 第一次接触**：打开官方文档看到 `#[tauri::command]` 直接懵

**这个项目从架构、Rust 后端、到 Vue 前端组件，100% 由 AI 写出来。** 我的角色更像产品经理 + QA：提需求、看效果、提 bug、决定要不要这个功能。**一行 Rust 代码都没自己手写过。**

放在两三年前，我 99% 会因为"啃不下 Rust"直接放弃这个想法——这种"想做但不会"的项目，相信每个开发者电脑里都躺着好几个。

但 2026 年的今天，工作流已经完全变了。大致是这样：

> 我："给我一个 Tauri 命令，浏览 Android 设备的目录，非 root 设备要支持 run-as 进沙盒。"
> 
> AI：直接产出可编译的 Rust 代码，连边缘情况（权限错误、目录不可读、应用非 debuggable）都帮我考虑了。
> 
> 我："编译报错了，错误信息贴你了。"
> 
> AI：一眼指出哪个生命周期不对、哪里 borrow 冲突，给出修正版。
> 
> 我："再加一个录屏功能，保存为 mp4。"
> 
> AI：5 分钟，从 Rust 后端到 Vue 前端面板，整套实现给我。

**整个项目的 Rust 代码我能看懂大致逻辑，但让我从零写绝对写不出来。** AI 给我的不是"代码片段"，而是"能直接跑、能上线"的完整实现。

走完这个项目，我对 AI 的认知彻底刷新了几个点：

1. **AI 抹平了技术栈的学习门槛**。不会 Rust 一样能写 Rust 项目，前提是你能把需求讲清楚。
2. **"提需求"的能力比"写代码"更重要**。需求拆得越细、约束越明确，AI 给的代码越接近能直接合并的状态。
3. **审美和判断力才是真正的核心竞争力**。AI 给的方案，你得能判断哪个合理、哪个偷懒、哪个埋坑——这部分 AI 替代不了你。
4. **"我不会某语言"已经不是借口**。被技术栈劝退的项目，现在都可以重新捡起来。

当然，AI 也不是魔法。它不会替你测试、不会替你判断功能该不该加、不会替你打磨细节。这个项目里每一处交互、每一个流程优化，都是我真实用了无数次之后反复迭代的——**那部分必须是你自己的活**。

如果你也有"想做但不会"的想法，**别再被技术栈吓住了：让 AI 当副驾驶，你只管掌方向。**

---

## 五、技术栈一览

| 层级    | 选型                                          |
| ----- | ------------------------------------------- |
| 桌面框架  | Tauri 2                                     |
| 后端    | Rust（`std::process` 调 adb，`rusqlite` 解析 db） |
| 前端    | Vue 3 `<script setup>` + TypeScript         |
| UI 组件 | Element Plus                                |
| 状态管理  | Pinia                                       |
| 构建    | Vite 6                                      |

包大小最终 macOS dmg 约 **10MB**，启动 **<1 秒**。比同类 Electron 应用小一个数量级。

---

## 六、下载和使用

- GitHub：https://github.com/jaaksi/adbtools
- Releases（dmg 下载）：https://github.com/jaaksi/adbtools/releases
- 自己跑：

```bash
git clone git@github.com:jaaksi/adbtools.git
cd adbtools
pnpm install
pnpm tauri dev
```

需要本地装好 Rust 工具链和 adb。详细环境要求见 README。

---

## 七、写在最后

这个项目最初只是想偷懒少敲几条命令，写着写着发现几件事：

- **Rust + Tauri 是写跨平台小工具的真香组合**：Electron 的内存焦虑、原生开发的多平台维护成本，被一并解决
- **"封装 CLI 成 GUI" 是被低估的方向**：很多命令行工具不缺功能，只缺一层 UI；Tauri 让这层 UI 的成本降到很低
- **AI 把"个人开发者"的天花板拉高了一个量级**：以前一个人一个项目，做不了多复杂；现在一个人 + AI，能跑通完整产品的全栈
- **解决自己的问题最有动力**：项目里每个功能都是我自己用爽了之后才合并的——这种循环非常上瘾

如果你也是 Android 开发者，欢迎试用，**也欢迎在 GitHub 提 Issue 告诉我你想要什么功能**。

最后，如果觉得有用，**点个 Star ⭐ 是最大的鼓励**：https://github.com/jaaksi/adbtools

---

**作者**：jaaksi
**协议**：Apache License 2.0
