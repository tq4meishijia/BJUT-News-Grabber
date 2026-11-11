# Assets · 图标资源补齐说明

> **当前状态：此目录只有本说明文件，没有任何二进制图标。** 这是**有意为之**——本机没有
> Windows SDK / VS2022 / 图像工具链，无法生成经过验证的 `.ico` / `.png`。
> 为了不让 `CampusNotice.csproj` 因引用不存在的图标而编译失败，项目里：
> - 没有设置 `<ApplicationIcon>`；
> - `Package.appxmanifest` 中的 `Logo` / `Square44x44Logo` 等仅作为 MSIX 打包时的占位路径；
> - `TrayIconService` 在找不到 `.ico` 时回退到系统自带的应用程序图标（`LoadIcon(NULL, IDI_APPLICATION)`），
>   不会崩溃，只是图标不美观；
> - `NavigationView` 的 PaneHeader 品牌位用 `FontIcon`（`IconRinger`）代替位图，不依赖 `.ico`。

首次构建前请按下面的规格补齐二进制资源；补齐后建议把
`<ApplicationIcon>Assets\AppIcon.ico</ApplicationIcon>` 加回 `CampusNotice.csproj`。

---

## 1. 源图

仓库根目录下的 `public/icon.svg`（品牌蓝 `#2B6CFF`、铃铛 + 日历、圆角底）为唯一源图。
**不要重新设计**；WinUI 3 客户端与现有 Web 版共用同一品牌资产。

## 2. 需要产出的文件

| 文件 | 规格 | 用途 |
| --- | --- | --- |
| `AppIcon.ico` | 多尺寸单文件 ICO：16 / 20 / 24 / 32 / 48 / 256 px，32bpp BGRA + Alpha | 应用程序图标、系统托盘、任务栏、Alt+Tab |
| `TrayLight.ico` | 16 / 20 / 24 / 32 px，彩色版 | 浅色任务栏托盘图标 |
| `TrayDark.ico` | 16 / 20 / 24 / 32 px，**单色白**版（深色任务栏） | 深色任务栏托盘图标 |
| `StoreLogo.png` | 50×50 | MSIX 清单 `Properties/Logo` |
| `Square44x44Logo.png` | 44×44 | MSIX 磁贴 / 跳列表图标 |
| `Square150x150Logo.png` | 150×150 | MSIX 磁贴 |
| `Wide310x150Logo.png` | 310×150 | MSIX 宽磁贴 |
| `SplashScreen.png` | 620×300 | MSIX 启动屏 |

另有缩放标注：视觉规范 §9.3 要求位图资源备 `@125%` / `@150%` 版本，避免高 DPI 下铃铛模糊。
WinUI 3 的 `BitmapIcon` / `Image` 支持 `Assets\foo.scale-125.png`、`foo.scale-150.png` 命名约定；
若用 ICO（自带多尺寸），系统会自动选最接近的尺寸，可不再单独出 scale 版本，但**必须包含 20 / 24 / 32 px**
（125% → 20×20、150% → 24×24 的常见整数倍）。

## 3. 可执行的转换方案（三选一）

### 方案 A · ImageMagick（推荐，命令行可复现）

```bash
# 需要 ImageMagick 7（magick 命令）。从仓库根目录执行。
mkdir -p winui3/CampusNotice/Assets

# ① 多尺寸彩色 ICO
magick -background none public/icon.svg \
  -define icon:auto-resize=256,48,32,24,20,16 \
  winui3/CampusNotice/Assets/AppIcon.ico

# ② 单色白版（深色任务栏）：先把不透明像素全部替换为白色
magick -background none public/icon.svg -resize 256x256 \
  -alpha extract -negate -fill white -colorize 0,100 \
  \( -clone 0 -background none public/icon.svg -resize 256x256 -alpha off -fill white -colorize 100 \) \
  -delete 0 -compose CopyOpacity -composite \
  -define icon:auto-resize=48,32,24,20,16 \
  winui3/CampusNotice/Assets/TrayDark.ico

cp winui3/CampusNotice/Assets/AppIcon.ico winui3/CampusNotice/Assets/TrayLight.ico

# ③ MSIX 用 PNG
for s in 50 44 150; do magick -background none public/icon.svg -resize ${s}x${s} \
  winui3/CampusNotice/Assets/Square${s}x${s}Logo.png; done
magick -background none public/icon.svg -resize 50x50  winui3/CampusNotice/Assets/StoreLogo.png
magick -background none public/icon.svg -resize 310x150 winui3/CampusNotice/Assets/Wide310x150Logo.png
magick -background none public/icon.svg -resize 620x300 winui3/CampusNotice/Assets/SplashScreen.png
```
> `Square150x150Logo.png` 命名与 `Package.appxmanifest` 一致；`for` 循环生成的 `Square50x50Logo.png` /
> `Square44x44Logo.png` 中，50×50 的那份请重命名为 `StoreLogo.png`（或直接以②覆盖）。

### 方案 B · Inkscape + png2ico / icotool

```bash
inkscape public/icon.svg -w 256 -h 256 -o /tmp/icon-256.png
for s in 16 20 24 32 48; do inkscape public/icon.svg -w $s -h $s -o /tmp/icon-$s.png; done
# Debian/Ubuntu: icotool (icoutils)；Windows: png2ico
icotool -c -o winui3/CampusNotice/Assets/AppIcon.ico \
  /tmp/icon-16.png /tmp/icon-20.png /tmp/icon-24.png /tmp/icon-32.png /tmp/icon-48.png /tmp/icon-256.png
```

### 方案 C · Visual Studio / 手工

用 VS2022 的「图像清单」或 [icoconvert.com](https://icoconvert.com) 从 `public/icon.svg`
导出上述尺寸 ICO；PNG 用任意图像编辑器按尺寸导出。图标内容保持单色铃铛+日历即可，
**不要加投影、渐变、彩色整块背景**（视觉规范 §11）。

## 4. 深色单色版要点

- 深色任务栏要求**纯白 + 透明**的单色字形，不要蓝底方块；
- 保留铃铛轮廓与日历格线，去掉蓝色圆角底；
- 与浅色版共用同一几何轮廓，仅换前景色，保证切换主题时图标「不跳动」。

## 5. 补齐后需要同步的两处

1. `CampusNotice.csproj`：加入
   ```xml
   <ApplicationIcon>Assets\AppIcon.ico</ApplicationIcon>
   ```
   并把 `Assets\*.ico` / `Assets\*.png` 标为 `<Content Include="Assets\**\*.ico;Assets\**\*.png"><CopyToOutputDirectory>PreserveNewest</CopyToOutputDirectory></Content>`。
2. `Services/TrayIconService.cs`：`ResolveIconPath()` 会优先找 `Assets\TrayDark.ico`（深色） /
   `Assets\TrayLight.ico`（浅色），找不到才回退系统图标；补齐后无需改代码。

## 6. 硬约束复核

补齐图标时**不得**新增任何启动类入口的图标、徽标或说明（视觉规范 §11 第 12 条、UX 规范 §6.7）。跳转列表与托盘菜单的图标只复用四页图标与系统样式。
