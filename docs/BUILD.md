# 打包指南(exe / app / AppImage)

## 方式一:GitHub Actions(推荐,零本机依赖)

仓库已带工作流 `.github/workflows/release.yml`(Windows / macOS arm64 / macOS x86_64 / Linux 四路并行):

1. 把仓库推到 GitHub;
2. 打 tag:`git tag v0.1.0 && git push --tags`(或 Actions 页面手动 Run);
3. 完成后在 **Artifacts / Releases(draft)** 下载:
   - Windows:`MCP Bridge-0.1.0-setup.exe`(NSIS 安装包)+ 便携 exe
   - macOS:`.app` / `.dmg`(Apple Silicon + Intel)
   - Linux:AppImage / deb

> Windows 的 exe 依赖系统自带 **WebView2 Runtime**(Win11 内置,Win10 近年版本也基本内置,缺失时安装包会自动引导下载)。

## 方式二:本机构建

| 目标系统 | 前提 | 命令 |
|---|---|---|
| Windows | 本机 Windows + VS Build Tools + WebView2 | `cd app && pnpm tauri build` |
| macOS | Xcode CLT | `cd app && pnpm tauri build` |
| Linux | `webkit2gtk-4.1-dev` 等(见 CI) | `cd app && pnpm tauri build` |

## 方式三:Linux 交叉编译 Windows exe(已在 Ubuntu 22.04 验证 ✅)

产物:`dist/MCP-Bridge-v0.1.0-windows-x64.exe`(PE32+ GUI,单文件 18MB,便携免安装)。

```bash
# 1) zig 作跨平台 C/链接工具链(免安装,解压即用)
curl -LO https://ziglang.org/download/0.14.0/zig-linux-x86_64-0.14.0.tar.xz && tar -xf zig-*.tar.xz

# 2) windres 及 gcc 预处理器:解压 Ubuntu 的 mingw binutils(免 sudo)
curl -LO 'http://archive.ubuntu.com/ubuntu/pool/universe/b/binutils-mingw-w64/binutils-mingw-w64-x86-64_2.38-3ubuntu1+9build1_amd64.deb'
dpkg -x binutils-mingw-w64-*.deb ~/.local/mingw
#    windres 内部调用 gcc 做预处理 → 用 zig cc 做个 shim
printf '#!/bin/sh\nexec /tmp/zig-linux-x86_64-0.14.0/zig cc -target x86_64-windows-gnu "$@"\n' \
  > ~/.local/mingw/usr/bin/x86_64-w64-mingw32-gcc && chmod +x ~/.local/mingw/usr/bin/x86_64-w64-mingw32-gcc

# 3) Rust 目标 + zigbuild
rustup target add x86_64-pc-windows-gnu
cargo install cargo-zigbuild

# 4) 构建(先出前端产物)
cd app && pnpm install && pnpm build
cd src-tauri && export PATH=/tmp/zig-linux-x86_64-0.14.0:$HOME/.local/mingw/usr/bin:$PATH
cargo zigbuild --release --target x86_64-pc-windows-gnu
# → target/x86_64-pc-windows-gnu/release/app.exe
```

关键点：`tauri-winres` 需要 `x86_64-w64-mingw32-windres` 嵌入 exe 图标/清单;webview2 在 gnu 目标下经 lld 链接正常。NSIS 安装包需要 Windows 主机构建(或走 CI)。

### ⚠️ 直接 cargo 构建必须带 default features(custom-protocol)

`src-tauri/Cargo.toml` 的 `[features] default = ["custom-protocol"]` 决定生产/开发模式。若缺失，`cargo build --release` 也会编成**开发模式**——exe 启动去连 `http://localhost:5173`,Windows 上表现为 WebView2 错误页“无法访问此页面 / ERR_CONNECTION_REFUSED”。验证方法：检查 `target/**/release/build/app-*/output` 中**不应**有 `cargo:rustc-cfg=dev`。

### ⚠️ gnu 交叉产物必须随带 WebView2Loader.dll

`webview2-com` 源码里是硬编码区分：

- `target_env = "msvc"` → **静态链接** `WebView2LoaderStatic.lib`(CI/Windows 主机构建 → 真正单文件 exe);
- 非 msvc(我们的 gnu 交叉构建)→ **动态链接** `WebView2Loader.dll`,exe 启动时需要它在旁边，否则报“找不到 WebView2Loader.dll”。

> 注意：装“WebView2 Runtime”治不好这个错——Runtime 装的是浏览器引擎;Loader.dll 是随程序分发的加载器。

处置(已打好包):构建后把 crate 自带的 x64 DLL 拷到 exe 同级:

```bash
cp ~/.cargo/registry/src/*/webview2-com-sys-*/x64/WebView2Loader.dll dist/
# 已产出 dist/MCP-Bridge-v0.1.0-windows-x64.zip = exe + WebView2Loader.dll
```

解压后**两个文件放同一目录**即可运行。要真正单文件 exe,用 CI(msvc 静态链接)或 cargo-xwin 交叉 msvc 目标。

## 产物体积参考

Tauri 单文件 exe 约 **8–15MB**(前端 ~130KB gzip + Rust 核心),对比 Electron 同类 150MB+。冷启动 <1s。
