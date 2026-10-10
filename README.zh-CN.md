<table align="center"><tr>
<td align="center" width="120"><a href="README.md" hreflang="en">English</a></td>
<td align="center" width="120"><b>简体中文</b></td>
<td align="center" width="120"><a href="README.es.md" hreflang="es">Español</a></td>
<td align="center" width="120"><a href="README.de.md" hreflang="de">Deutsch</a></td>
</tr></table>

# 砺模 CAD

> **砺模，求理解之设计。**

**易用的参数化 CAD，免费开源，并将永远如此。**
在你自己的电脑上设计机械零件、装配体和工程图，可以亲手操作，也可以交给 AI 智能体，
每一个草图和特征都保持可编辑。

[![Bevy 预览版](https://img.shields.io/badge/Bevy-0.20.0--rc.2-blue)](https://github.com/limo-cad/Limo-CAD/releases/tag/bevy-preview-0.2.2-20261004.1)
[![许可证：LGPL 2.1+](https://img.shields.io/badge/license-LGPL%202.1%2B-blue)](LICENSE)
[![Discussions](https://img.shields.io/github/discussions/limo-cad/Limo-CAD?label=discussions)](https://github.com/limo-cad/Limo-CAD/discussions)

**早期预览版（Pre-alpha）· Bevy rc.2 · 应用版本 0.2.2**
· [预览版说明、源码版本与检查结果](https://github.com/limo-cad/Limo-CAD/releases/tag/bevy-preview-0.2.2-20261004.1)
· [安装帮助（英文）](docs/INSTALL.md)

| 平台 | 下载 |
|---|---|
| Windows 11 | [x64 ZIP](https://github.com/limo-cad/Limo-CAD/releases/download/bevy-preview-0.2.2-20261004.1/noBS-CAD-0.2.2-windows-x64.zip) |
| Linux | [Ubuntu 26.04 x64 DEB](https://github.com/limo-cad/Limo-CAD/releases/download/bevy-preview-0.2.2-20261004.1/noBS.CAD_0.2.2_amd64.deb) |

这些包使用源码版本 `9b082687`，尚未包含后续集成修复。Windows ARM64、macOS 和
AppImage 仍待验证。Bevy 浏览器界面尚在开发中。已发布的文件名保留原产品名称。
详情见[迁移状态（英文）](docs/native-transition-status.md)。

Windows 包尚未签名。首次启动时 SmartScreen 可能会发出警告，请选择
**更多信息 → 仍要运行**。请为重要的早期预览项目做好备份。

> **语言说明：** 本页为简体中文。下方链接的文档、示例和知识库目前仅有英文版，
> 以“（英文）”标注或直接指向英文页面；欢迎熟悉相关内容的贡献者帮助审校和翻译。

## 为什么选择砺模 CAD

- **永远免费开源。** 没有付费版，没有功能限制。代码采用
  [LGPL 2.1 或更高版本](LICENSE)，因此始终保持开放。
- **易于上手。** 易用性与可靠性、性能并列为项目的三项优先事项。
  [第一个零件教程](#做出你的第一个零件)只需几分钟。
- **本地运行，属于你自己。** 无需账号、订阅或云服务。整个项目（零件、装配体和工程图）
  保存在一个 `.limo` 文件中。
- **真正的参数化历史。** 约束草图驱动实体特征；修改一个尺寸，下游所有内容随之重建。
- **智能体就绪。** 内置的 MCP 服务器让任何兼容 MCP 的智能体都能构建和编辑模型，
  所生成的内容与你亲手做出的可编辑历史完全一致。
- **开放格式。** 导出 STEP、STL 和 3MF；工程图导出 DXF 和打印/PDF。

## 用砺模 CAD 做出的设计

每个设计都由空白文档通过 MCP 构建，其草图、特征和装配关系依然可以编辑。
**观看**会播放建模录像；**建模循环**是加速后的简短片段。

<table>
<tr>
<td align="center" valign="top" width="33%">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#garden-bench"><img src="docs/assets/showcase/bench.png" alt="带有弧形顶部靠背板条和圆角扶手的花园长椅"></a><br>
<b>花园长椅</b><br>
修改一根靠背板条的尺寸，整个靠背随之更新。框架、扶手和连接关系保持可编辑。
</td>
<td align="center" valign="top" width="33%">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#d-screw-vise"><img src="docs/assets/showcase/vise.png" alt="带有受约束滑动钳口和紧凑 D 形丝杠手柄的台虎钳"></a><br>
<b>台虎钳</b><br>
转动丝杠，钳口随之移动。100 mm 钳口，90 mm 行程，六个 3D 打印零件加标准件。
</td>
<td align="center" valign="top" width="33%">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#vertical-axis-turbine"><img src="docs/assets/showcase/turbine.png" alt="带轴承支撑轴和发电机传动的两级垂直轴涡轮"></a><br>
<b>垂直轴涡轮</b><br>
两级 Savonius 叶轮安装在轴承支撑的轴上，通过 4:1 传动驱动发电机。
</td>
</tr>
<tr>
<td align="center">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#garden-bench"><b>观看</b></a>
· <a href="https://limo-cad.github.io/Limo-CAD/open.html#garden-bench">打开配方</a><br>
<a href="examples/scripts/garden-bench.limo.jsonc">源码</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/bench.nbcad">.nbcad</a>
· <a href="docs/assets/showcase/bench-loop.gif">建模循环</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/bench-build-full.mp4">MP4</a>
</td>
<td align="center">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#d-screw-vise"><b>观看</b></a>
· <a href="https://limo-cad.github.io/Limo-CAD/open.html#d-screw-vise">打开配方</a><br>
<a href="examples/scripts/d-screw-vise.limo.jsonc">源码</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/vise.nbcad">.nbcad</a>
· <a href="docs/assets/showcase/vise-loop.gif">建模循环</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/vise-build-full.mp4">MP4</a>
</td>
<td align="center">
<a href="https://limo-cad.github.io/Limo-CAD/showcase.html#vertical-axis-turbine"><b>观看</b></a>
· <a href="https://limo-cad.github.io/Limo-CAD/open.html#vertical-axis-turbine">打开配方</a><br>
<a href="examples/scripts/vertical-axis-turbine.limo.jsonc">源码</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/turbine.nbcad">.nbcad</a>
· <a href="docs/assets/showcase/turbine-loop.gif">建模循环</a>
· <a href="https://github.com/limo-cad/Limo-CAD/releases/download/showcase-v0.2.0/turbine-build-full.mp4">MP4</a>
</td>
</tr>
</table>

<!-- Print photos: add docs/assets/showcase/<design>-printed.jpg when supplied. -->

配方链接会把源码载入 **脚本（Scripts）**，供你审阅后再运行。若想立即查看完成的设计，
请下载其 `.limo` 文件并使用 **文件 → 打开**。
这些是开发示例；实物配合与承载能力的验证仍未完成。
[设计、工程图与验证（英文）](docs/flagship-examples.md) · [全部配方（英文）](examples/scripts/README.md)

## 做出你的第一个零件

[安装 CAD](docs/INSTALL.md)后，打开 **脚本（Scripts）**，选择
**Sketch, extrude, ease the edges**，然后点击 **Run in new design**。
该教程会建出一个 60 × 30 × 12 mm、顶部边缘带圆角的方块。

完成后，在特征历史中双击拉伸特征，把 **距离（Distance）** 从 **12 改为 18 mm**。
将结果保存为 `first-part.limo`，再重新打开继续编辑。
[分步说明（英文）](docs/INSTALL.md#make-your-first-part)

## 设计、装配、出图

约束草图和参考几何驱动可编辑的实体特征。
在装配体中复用零件，定义关节，并检查运动和干涉。
零件、装配体和工程图都保存在同一个 `.limo` 项目中。

为每个实体指定材料和颜色，然后导出 **3MF** 供切片软件使用。
材料标签和颜色元数据只用于辅助交接；实际打印配置请在切片软件中选择。
**STEP** 保留精确几何，**STL** 提供网格导出。图纸页可导出为 **DXF** 和打印/PDF。
[装配体（英文）](docs/ASSEMBLIES.md) · [工程图与导出覆盖范围（英文）](docs/2D_DRAWINGS.md)

## 与智能体协作

已安装的应用始终提供本地 stdio MCP。带上你喜欢的兼容 MCP 的智能体和模型，
即可构建零件、编辑现有特征、检查装配体或回放演示。无论你亲自使用工具，
还是让智能体来用，CAD 都保持同一份可编辑项目。

[连接你的智能体（英文）](docs/INSTALL.md#connect-an-mcp-agent)，然后试试：

> Use Limo CAD to run the fillet-basics lesson in a new design in the open CAD
> window. Preserve my existing documents. After the final checks pass, change
> the stock extrusion from 12 to 18 mm, inspect the result and keep it open.

（上面的提示词目前以英文给出，因为内置教程名称是英文的。）

智能体是可选的。**脚本（Scripts）** 可以构建内置示例，讲解各章节，
并通过字幕、镜头运动和播放控制展示建模过程。运行前你可以检查并编辑配方。
[MCP 接口（英文）](mcp-server/README.md) · [配方与回放（英文）](docs/native-scripts.md)
· [工程知识库（英文）](knowledge/index.md)

## 一起来建设

欢迎贡献。我们的优先级依次是 **可靠性、性能和易用性**。
带来一个零件、一个可复现的缺陷或一项有针对性的改进吧。
[贡献指南（英文）](CONTRIBUTING.md) · [开发环境搭建（英文）](docs/DEVELOPMENT.md)
· [文档（英文）](docs/INDEX.md)

有问题、想法，或者想展示你的作品？欢迎到
[Discussions](https://github.com/limo-cad/Limo-CAD/discussions) 发起讨论。
如果砺模 CAD 对你有用，点一个 star 能帮助更多人发现它。

我们正朝着引导式设计课程和对话式向导努力，并在开发早期的 **三轴 CAM** 基础功能，
包括刀路生成、毛坯模拟和面向机床的后处理。它还不是生产级安全的 CAM；
请阅读 [CAM 指南与安全限制（英文）](docs/cam/README.md)。强度分析仍是未来的功能。
[项目方向（英文）](docs/goals.md)

## 开源基础

- **[Open CASCADE Technology](https://github.com/Open-Cascade-SAS/OCCT)** — 几何与 CAD 数据交换。
- **[Bevy](https://bevy.org/) 和 [wgpu](https://wgpu.rs/)** — 原生界面与渲染。
- **[Rust](https://rust-lang.org/)** — 建模、装配和配方执行。

同时感谢 [FreeCAD](https://www.freecad.org/) 和更广泛的开源 CAD 社区。

## 许可证

[GNU LGPL 2.1 或更高版本](LICENSE)。可自由使用、检查和改进。

<details>
<summary>第三方声明与 3D 鼠标支持</summary>

依赖项许可证和署名位于[第三方声明](THIRD_PARTY_NOTICES.md)。
图标来源记录在[图标来源](docs/ICON_PROVENANCE.md)。
其他 CAD 项目有各自的许可证；请参阅[贡献指南](CONTRIBUTING.md#license--borrow)。

Bevy 桌面通过原生 HID 输入支持 3Dconnexion SpaceMouse 设备。
砺模 CAD 是独立项目，与 3Dconnexion 没有隶属、认可或认证关系。
3Dconnexion 和 SpaceMouse 是 3Dconnexion 的商标或注册商标。
3D 输入设备开发工具及相关技术由 3Dconnexion 授权提供。© 3Dconnexion 1992–2020。保留所有权利。

</details>
