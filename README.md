# FlowArc

基于 **AUP 2.0（自适应超昼夜循环法）** 的桌面专注助手，用来替代固定倒计时的番茄钟。
支持 **macOS / Windows / Linux**，数据完全存本机，无账号。

> 核心思路：不是“到点就响”，而是 **校准启动 → 弹性专注（软下限 45′ / 个人上限 U / 硬上限 120′）→ 在自然断点落地 → 60 秒快照 → 分型休息**；每天的深度工作预算由睡眠与精力决定，个人上限随你的反馈缓慢自适应。

## 功能（MVP）

| 模块 | 说明 |
| --- | --- |
| 晨间签到 | 睡眠 + 精力 → 当日金/银块预算与上限修正 |
| 校准启动 | 1–5 分钟，写意图与完成标准 |
| 环境色弧 | 灰→蓝→琥珀→红；默认不显示数字，悬停才显示分钟 |
| 弹性落地 | ≥U 且在断点：轻提示；≥U+10：直接提示；≥120：强提示；每块可“再 10 分钟”一次 |
| 落地快照 | 停在哪 / 未写下的想法 / 回来第一步 / 心流·疲劳·完成度；自动捕获前台应用、窗口标题、git 分支与改动摘要，**每项可删** |
| 恢复卡 | 下一块开始前显示“第一步”与上下文 |
| 分型休息 | 疲劳→闭眼/小睡；卡壳→带问题散步；久坐→走动；紧绷→循环叹息；时长随专注时长缩放 |
| 停车场 | 全局快捷键 `Ctrl/⌘+Shift+Space` 一行记下干扰念头 |
| 收工仪式 | 日小结 + 清空停车场 + 末疲劳 |
| 个人上限自适应 | 疲劳≥4 或临界未完成：U−5；连续 3 块跑满且心流≥4、疲劳≤2：U+5；限 60–120 |
| 隐私 | 导出 JSON/CSV、一键删除、上下文保留期、可关闭感知 |

设计依据与完整规划见《AUP 2.0：循证改进方案与配套软件规划》。

## 目录结构

```
crates/flowarc-core/   纯 Rust 核心（状态机、规则引擎、SQLite），无 UI 依赖，可单独测试
src-tauri/             Tauri 2 外壳：命令、托盘、全局快捷键、空闲/前台窗口感知
src/                   TypeScript + Vite 前端（无框架）
.github/workflows/     CI（三平台）与发布流水线
```

## 开发环境

通用：Node ≥ 20、Rust（stable，`rustup`）。

- **macOS**：`xcode-select --install`
- **Windows**：安装 [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)（“使用 C++ 的桌面开发”）；WebView2 在 Win10/11 通常已内置
- **Linux（Debian/Ubuntu）**：
  ```bash
  sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
       librsvg2-dev libxdo-dev libssl-dev libx11-dev libxss-dev build-essential pkg-config
  ```
  其他发行版见 [Tauri 前置依赖](https://tauri.app/start/prerequisites/)。

## 运行与构建

```bash
npm install
npm run tauri dev        # 开发模式（热更新）
npm run tauri build      # 打包：.dmg / .msi+.exe / .deb+.rpm+.AppImage
npm run test:core        # 核心库单元测试（28 个）
```

> 前端依赖 Tauri 后端，不能单独在浏览器里打开。

发布：推送 `v*` 标签，`release.yml` 会在三平台构建并生成草稿 Release（未签名；macOS/Windows 正式分发需要自己的签名证书，见 Tauri 文档）。

## 平台说明与降级

| 能力 | macOS | Windows | Linux |
| --- | --- | --- | --- |
| 空闲检测（自然断点） | 无需权限 | 可用 | X11 可用；Wayland 取决于桌面环境，不可用时降级 |
| 前台应用/窗口标题 | 应用名无需权限；**窗口标题**需在“系统设置 → 隐私与安全 → 辅助功能/屏幕录制”授权 | 可用 | X11 可用；Wayland 通常不可用 |
| 托盘/菜单栏 | 菜单栏显示已专注分钟 | 系统托盘 | 需要 AppIndicator（GNOME 需启用扩展） |
| 全局快捷键 | 可用 | 可用 | X11 可用；Wayland 视合成器而定 |

任何感知能力不可用时，应用自动降级：断点改为界面上的“我现在到了断点”手动按钮，快照里不再有自动上下文，其余流程不受影响。

## 隐私

- 只记录：块的起止、快照文字、你的评分、**每分钟是否活跃**与前台应用名。
- **绝不记录**：按键内容、截图、剪贴板。
- 窗口标题/git 摘要仅在你保留的前提下存本机，默认 30 天后清除，可在设置中关闭或设为 0。
- 数据位置：系统应用数据目录下的 `flowarc.db`（macOS `~/Library/Application Support/app.flowarc.desktop/`，Windows `%APPDATA%\app.flowarc.desktop\`，Linux `~/.local/share/app.flowarc.desktop/`）。

## 当前未实现（对应规划的后续阶段）

- 数据库加密（SQLCipher）：目前是普通 SQLite 文件；建议先开启系统磁盘加密。SQLCipher 在 Windows 需要 OpenSSL，故未放入 MVP。
- “重开上下文”目前是**复制上下文到剪贴板**，尚未自动重新打开应用/文件。
- 系统勿扰/专注模式联动、IDE 插件、浏览器扩展、通知批处理、周复盘、N-of-1 自我实验、被动疲劳信号、手表端、团队状态灯、可选端到端加密同步。
- 修改全局快捷键需编辑设置后重启。
- 休息“卡壳/紧绷”信号目前由休息界面里的按钮手动触发。
- 安装包未签名、未做自动更新。

## 许可

MIT
