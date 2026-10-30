<p align="center"><img src="https://raw.githubusercontent.com/Hybirdss/altium-cli-extended/main/docs/assets/banner.png" alt="altium-cli-extended" width="820"></p>

<div align="center">

# altium-cli-extended (`ace`)

**面向 AI 智能体与 CI 的原生 Rust 解析器、字节级安全编辑器与 Altium Designer 自动化桥接工具**

<a href="https://github.com/Hybirdss/altium-cli-extended/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/Hybirdss/altium-cli-extended/ci.yml?branch=main&label=CI" alt="CI"></a>
<a href="https://github.com/Hybirdss/altium-cli-extended/releases"><img src="https://img.shields.io/github/v/release/Hybirdss/altium-cli-extended?label=release" alt="latest release"></a>
<a href="https://github.com/Hybirdss/altium-cli-extended/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg" alt="GPL-3.0-or-later license"></a>

[English](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.md) | [한국어](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.ko.md) | **简体中文** | [繁體中文](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.zh-TW.md) | [日本語](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.ja.md) | [Español](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.es.md) | [हिन्दी](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.hi.md)

[双后端架构](#原生与-designer-双后端) · [安装](#安装) · [快速开始](#快速开始) · [智能体技能](#智能体技能) · [工作流](#工作流) · [命令组](#命令组) · [JSON 契约](#json-契约) · [开发](#开发)

</div>

> **正在使用 KiCad 10？** 请查看姊妹工具 **[kicad-cli-extended (`kce`)](https://github.com/Hybirdss/kicad-cli-extended)**。两个项目均采用 GPL-3.0-or-later 许可证。

Altium Designer 官方没有提供命令行工具——它是一个运行于 Windows 的 GUI 程序（`X2.EXE`）。**`ace`** (Altium CLI Extended) 通过**双后端架构**填补了这一空白：

1. **原生 Rust 引擎（`backend: native`）：** **无需安装 Altium Designer**，即可在 **Windows、macOS 和 Linux** 上直接解析、检查、对比（diff）、校验并以字节级安全（byte-safe）方式编辑 `.PrjPcb`、`.SchDoc`、`.PcbDoc`、`.SchLib`、`.PcbLib` 和 `.OutJob` 文件。
2. **Designer 桥接后端（`backend: designer`）：** 当任务需要调用 Altium 引擎本身（如项目编译、ERC、DRC、生成 Gerber/ODB++/PDF/STEP 等 `.OutJob` 输出、调用任意 Altium 进程或运行 DelphiScript）时，桥接并驱动用户自己拥有合法授权的 Altium Designer（支持本地 Windows、SSH 远程 Windows/虚拟机，或 Linux Wine）。

每条命令都输出统一的 JSON 信封格式，每次失败都带有稳定的错误码与恢复提示，且 `ace commands` 与 `ace describe` 完整公开了所有命令的参数与返回结构，让 AI 智能体与 CI 流水线无需猜测即可可靠工作。

---

## 原生与 Designer 双后端

| 维度 | 原生后端 (`backend: native`) | Designer 后端 (`backend: designer`) |
|---|---|---|
| **外部依赖** | 无（独立单文件二进制） | 用户合法授权的 Altium Designer (`X2.EXE`) |
| **运行平台** | Windows、macOS、Linux | 本地 Windows、SSH 远程 Windows 或 Linux Wine |
| **支持功能** | 读取全部文件、导出 BOM 与网表、设计检查、版本差异对比、SVG 预览、字节级安全地修改参数/规则/变体/封装 | 项目编译、ERC、DRC、执行 `.OutJob` 发布任务、调用任意 Altium 进程与 DelphiScript |

### Designer 主机模式

* **本地 Windows（默认）：** 调用本机已登录桌面会话中的 Altium Designer（Windows 10 1809+ 使用 `conhost --headless`）。运行 `ace designer setup` 检查前置条件，并按提示以桌面用户身份执行 `ace designer setup --apply runner` 注册计划任务（普通权限即可，无需管理员提权）。若安装在非默认路径，请将 `ACE_X2` 指向 `X2.EXE`。
* **SSH 远程 Windows：** 在 macOS、Linux 或另一台 PC 上运行 `ace`，通过 SSH 驱动远程 Windows 主机或虚拟机上的 Altium（自动同步工程文件）。`ace` 对 `~/.ssh/known_hosts_altium` 强制启用 `StrictHostKeyChecking=yes`，首次连接前需通过可信渠道核对服务器指纹并写入该文件。
* **Linux Wine（可选）：** 在 Linux 下通过 Wine 驱动 Altium Designer。

> **字节级安全写入与 Designer 说明：**
> * 原生写入命令（`effect: write`）支持 `--dry-run`，默认保留首个备份文件 `<file>.bak`（除非传入 `--no-backup`），并在替换原文件前重新解析校验。二进制文档修改不会触碰无关记录和流；工程与 OutJob INI 文件写入会保留条目顺序、换行符和 BOM，但会规范化空行并忽略手写注释。
> * Designer 已知限制：Designer 任务执行后，工程 `.PrjPcb` 及桥接脚本工程（`AltiumCliBridge.PrjScr`、`Job.PrjScr`）会保留在 Altium 的 Projects 面板中，且运行过 DRC 的 PCB 会显示为已修改。列表变长时可在 Projects 面板中手动关闭，并对未编辑的文件选择 **“Don't Save”**。

---

## 安装

### 预编译二进制包

从 [GitHub Releases](https://github.com/Hybirdss/altium-cli-extended/releases) 下载对应平台的压缩包：

| 操作系统 | 压缩包名称 | 可执行文件 |
|---|---|---|
| **Windows x64** | `altium-cli-extended-x86_64-pc-windows-msvc.zip` | `ace.exe` |
| **macOS Apple Silicon** | `altium-cli-extended-aarch64-apple-darwin.tar.gz` | `ace` |
| **macOS Intel** | `altium-cli-extended-x86_64-apple-darwin.tar.gz` | `ace` |
| **Linux x86_64** | `altium-cli-extended-x86_64-unknown-linux-gnu.tar.gz` | `ace` |
| **Linux arm64** | `altium-cli-extended-aarch64-unknown-linux-gnu.tar.gz` | `ace` |

每个压缩包均附带对应的 `<archive>.sha256` 校验文件（macOS/Linux 使用 `shasum -a 256 -c`，Windows 使用 `Get-FileHash`）。解压并将二进制加入 `PATH`：

```powershell
# Windows (PowerShell)
Expand-Archive altium-cli-extended-x86_64-pc-windows-msvc.zip -DestinationPath $env:LOCALAPPDATA\altium-cli-extended
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
[Environment]::SetEnvironmentVariable("Path", "$userPath;$env:LOCALAPPDATA\altium-cli-extended", "User")
$env:Path += ";$env:LOCALAPPDATA\altium-cli-extended"
ace.exe doctor --quick
```

```bash
# macOS / Linux
mkdir -p altium-cli-extended && tar xzf altium-cli-extended-*.tar.gz -C altium-cli-extended
sudo mv altium-cli-extended/ace /usr/local/bin/
ace doctor --quick
```

*macOS Gatekeeper 提示：* macOS 预编译二进制未签名。校验 SHA-256 无误后，若 Gatekeeper 拦截运行，可执行 `xattr -d com.apple.quarantine /usr/local/bin/ace` 移除隔离属性。

### 从源码编译（需 Rust 1.89+）

```bash
cargo install --locked --git https://github.com/Hybirdss/altium-cli-extended altium-cli-extended
```

运行 `ace doctor --quick` 可在不连接 Designer 主机的情况下快速检查原生环境；运行 `ace doctor` 则会同时探测 Designer 主机状态。

---

## 快速开始

```bash
ace inspect Board.PrjPcb                          # 查看工程、原理图、PCB 或元件库概览
ace bom Board.PrjPcb --out bom.csv                # 导出分组 BOM（支持 --variant <name>）
ace netlist Board.PrjPcb --where name=GND         # 查询指定网络及其连接的引脚
ace check Board.PrjPcb                            # 检查位号标注、封装及原理图与 PCB 一致性
ace diff Board.PrjPcb --rev HEAD~1                # 与上一 Git 提交对比差异
ace pcb rules Board.PcbDoc --kind Clearance --units mil
ace sch set-param Power.SchDoc R5 MPN RC0603FR-0710KL --dry-run
ace pcblib list Footprints.PcbLib --where mount=smd --fields name,pads
ace pcblib rename Footprints.PcbLib SOIC-8 SOIC-8N --dry-run
```

* **筛选与投影：** 所有列表命令均支持 `--fields`、`--where`（`key=value`、`key!=value`、`key~regex`、`key^=substring`）与 `--limit`。
* **单位与编码：** 长度默认使用 `mm`（可通过 `--units mil|raw` 或 `ACE_UNITS` 修改）；旧版字符编码可通过 `--codepage` 或 `ACE_CODEPAGE` 指定。

---

## 智能体技能

仓库在 [`skills/altium-cli-extended/`](skills/altium-cli-extended/) 目录下提供了由二进制自动生成的 AI 智能体技能包：包含核心规则与 12 条触发式配方的 [`SKILL.md`](skills/altium-cli-extended/SKILL.md)，以及完整命令参考手册 [`COMMANDS.md`](skills/altium-cli-extended/COMMANDS.md)。

```bash
ace skill install                          # 安装至 ~/.agents/skills 及检测到的智能体（Claude、Codex、Gemini）
ace skill install --target claude          # 或 codex、gemini、agents、all
ace skill install --dir ./.claude/skills   # 安装到项目目录
ace skill status                           # 查看已安装副本及是否为最新
ace skill uninstall --target all
```

---

## 工作流

从你的实际需求出发。以下示例中 `$P` 代表工程文件（`.PrjPcb`），`$B` 代表电路板（`.PcbDoc`），`$S` 代表原理图纸（`.SchDoc`），`$R` 代表源码目录外的报告目录。请始终使用**绝对路径**（以便正确解析类似 `..\Utils\*.SchDoc` 的上级相对路径图纸）。

| 你的目标 | 起始命令 | 对应章节 |
|---|---|---|
| 在没有安装 Altium 的情况下审查外包交付工程 | `ace project find <dir>` | [审查外包交付工程](#审查外包交付工程) |
| 对比两个版本之间的原理图/PCB/库改动 | `ace diff A.PrjPcb B.PrjPcb --summary` | [对比版本差异](#对比版本差异) |
| 在 CI 中对设计问题进行门禁拦截 | `ace check P.PrjPcb --fail-on error` | [在 CI 中设置设计门禁](#在-ci-中设置设计门禁) |
| 不打开 Altium 修改阻容值、装配变体或设计规则 | `ace sch set-param ... --dry-run` | [免打开 Altium 应用 ECO 变更](#免打开-altium-应用-eco-变更) |
| 安全重命名或复制封装而不留下失效引用 | `ace pcblib rename ... --dry-run` | [安全重命名封装](#安全重命名封装) |
| 查询引脚、网络、层栈结构与库封装 | `ace sch locate P.PrjPcb U1` | [查找元件、网络与库封装](#查找元件网络与库封装) |
| 运行完整编译、ERC、DRC 并导出 Gerber 等生产文件 | `ace designer ping --timeout 60` | [使用 Designer 准备生产发布](#使用-designer-准备生产发布) |
| 回滚错误修改或恢复卡住的 Altium | `ace restore F --from first --dry-run` | [回滚修改或恢复卡住的 Designer](#回滚修改或恢复卡住的-designer) |
| 将 Altium 电路板迁移至 KiCad | `ace check P.PrjPcb --fail-on error` | [从 Altium 迁移到 KiCad](#从-altium-迁移到-kicad) |

### 审查外包交付工程

先搜索定位工程，设置 `ACE_READONLY=1` 在访问文件或 Designer 前拦截一切写入与控制命令，并将报告输出到工程目录之外：

```bash
ace project find /absolute/handover --depth 6
P=/absolute/handover/Board.PrjPcb
B=/absolute/handover/Board.PcbDoc
R=/absolute/review
mkdir -p "$R"
export ACE_READONLY=1
ace inspect "$P"
ace project documents "$P" --fields rel,kind,exists
ace sch hierarchy "$P"
ace pcb info "$B"
ace bom "$P" --group-by designator --out "$R/parts.csv"
ace bom "$P" --source pcb --limit 1
ace netlist "$P" --out "$R/source.net"
ace check "$P" --severity warning --require MPN
ace pcb svg "$B" --out "$R/board.svg"
```

### 对比版本差异

```bash
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --summary
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --by part --ignore-field '^param:ComponentLink'
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --where change=part_added --fields ref
ace diff /absolute/RevA/Board.PcbDoc /absolute/RevB/Board.PcbDoc --geometry
ace diff "$P" --rev HEAD~1 --source pcb
```

### 在 CI 中设置设计门禁

```bash
ace check "$P" --list
ace check "$P" --pcb "$B" --require MPN,Manufacturer --fail-on error > "$R/check.json"
ace diff "$P" --rev HEAD~1 --exit-code
ace check "$P" --pcb "$B" --require MPN,Manufacturer --baseline "$R/check.json" --fail-on error
```

* `check --fail-on error` 与 `diff --exit-code` 在触发门禁时仍会在 `stdout` 输出完整 JSON 并以退出码 `9` 退出。
* 使用 `--baseline` 可仅拦截相对于基线**新增**的问题。退出码 `10`、`11`、`12` 代表 Designer 主机不可用、失败或繁忙，不代表设计本身不合格。

### 免打开 Altium 应用 ECO 变更

```bash
ace check "$P" --pcb "$B" > "$R/before.json"
unset ACE_READONLY
ace sch locate "$P" R20,BF1
ace sch set-param /absolute/handover/Power.SchDoc R20 Value 22K --dry-run
ace sch set-param /absolute/handover/Power.SchDoc R20 Value 22K
ace project add-variant "$P" NoBuzzer --dry-run
ace project add-variant "$P" NoBuzzer
ace project set-variation "$P" NoBuzzer BF1
ace bom "$P" --variant NoBuzzer --group-by designator --limit 0
ace pcb set-rule "$B" Clearance gap 0.2mm --dry-run
ace pcb set-rule "$B" Clearance gap 0.2mm
ace check "$P" --pcb "$B" --baseline "$R/before.json" --fail-on error
ace diff "$P" --rev HEAD --by part
```

### 安全重命名封装

```bash
ace sch find "$S" SOIC-8
ace pcb components "$B" --where footprint=SOIC-8 --fields designator,footprint
ace pcblib copy /absolute/Target.PcbLib /absolute/Source.PcbLib SOIC-8 --name SOIC-8N --dry-run
ace pcblib rename /absolute/Target.PcbLib SOIC-8 SOIC-8N --dry-run
ace sch set-footprint "$S" U3 SOIC-8N --dry-run
ace diff /absolute/before/Target.PcbLib /absolute/Target.PcbLib
```

### 查找元件、网络与库封装

```bash
ace sch locate "$P" R20,BF1
ace netlist "$P" --pins --where designator=R20
ace sch pins /absolute/handover/Power.SchDoc --component R20 --project "$P" --unconnected --fields pin,name,net
ace pcblib find 0603 /absolute/libraries --pads
ace sch component "$S" R20 --project "$P"
ace pcb lengths "$B" --fields net,routed_length,vias --limit 20
ace pcb info "$B"
ace pcb stackup "$B"
ace pcb rules "$B" --where kind^=Clearance
```

### 使用 Designer 准备生产发布

```bash
ace doctor --quick
ace designer status
ace designer ping --timeout 60
ace designer compile "$P" --timeout 300
ace designer erc "$P" --timeout 300
ace designer drc "$B" --timeout 300
ace designer pcb info "$B"
ace outjob outputs /absolute/Release.OutJob
ace outjob mediums /absolute/Release.OutJob
ace designer outjob "$P" /absolute/Release.OutJob
```

在 PowerShell 中，请使用参数数组传递包含空格的路径，并显式检查 `$LASTEXITCODE`：

```powershell
$AceArgs = @('--json', '--read-only')
$Project = 'C:\Designs\My Board\Board.PrjPcb'
& ace.exe @AceArgs check $Project --fail-on error
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
```

### 回滚修改或恢复卡住的 Designer

```bash
ace restore "$S" --from first --dry-run
ace restore "$S" --from first
ace designer status
ace designer dialogs
ace designer screenshot --out /tmp/designer.png
ace designer jobs --last 3
```

### 从 Altium 迁移到 KiCad

冻结一份源工程副本并在两个工程目录之外记录递归文件/哈希清单，然后导入到显式指定的目标路径：

```bash
kce project new /tmp/target --name target
kce --json --project /tmp/target pcb import /absolute/source/Board.PcbDoc --format altium --output /tmp/target/target.kicad_pcb --report-format json --report-file /tmp/import.json
kce --json --project /tmp/target pcb info
```

回读目标电路板，确认返回路径等于 `/tmp/target/target.kicad_pcb`，检查导入的封装数量与哈希，并重新计算源目录清单（含文件新增与删除）以确保源文件未被改动。请注意，成功导入电路板并不等同于已验证的完整迁移：原理图/元件库转换与独立的电气一致性仍需单独验证。

---

## 命令组

| 命令组 | 用途 |
|---|---|
| `project` | 工程搜索、文档列表、参数、装配变体（及变体编辑）、工程设置 |
| `sch` | 原理图纸：元件、引脚、网络、标签、端口、层次结构，以及参数/注释/封装/位号编辑 |
| `schlib` | 原理图符号库：符号、引脚、参数与描述编辑 |
| `pcb` | PCB 电路板：层、层栈（stackup）、元件、网络、规则、类、图元、孔、走线长度、SVG；规则/类/参数编辑 |
| `pcblib` | 封装库：封装、焊盘、3D 模型、SVG；字节级安全的 `rename`、`set-param`、`remove`、`copy` |
| `outjob` | Output Job 文件：输出项、媒介、启用/禁用 |
| `bom`, `netlist`, `check`, `diff` | 跨文档分析：BOM 导出、网表提取、设计规则检查、版本/Git 差异对比 |
| `designer` | 真实 Altium Designer 桥接：安装配置、状态、弹窗检测、截图、编译、ERC、DRC、Output Job、进程与脚本 |
| `raw` | 任意复合文档（OLE/CFB）的底层流与记录查看 |
| `commands`, `describe`, `skill`, `errors`, `doctor`, `inspect`, `restore` | 命令自发现、返回结构说明、智能体技能管理、环境诊断与备份恢复 |

---

## JSON 契约

当通过管道调用或传入 `--json` 时，`stdout` 严格输出单个 JSON 对象：

```json
{"ok":true,"schema":"altium-cli-extended/v1","command":"pcb nets","data":{"count":2,"items":[...]},
 "warnings":[],"meta":{"units":"mm","backend":"native","effect":"read","dry_run":false,...}}
{"ok":false,"schema":"altium-cli-extended/v1","command":"pcb net","error":{"code":"NOT_FOUND","category":"input",
 "message":"no net \"CLKK\"","hint":"see `ace pcb nets` for the net names"}}
```

---

## 开发

```bash
git clone https://github.com/Hybirdss/altium-cli-extended
cd altium-cli-extended
scripts/fetch-fixtures.sh        # 下载测试样例工程至 .fixtures/ (Windows: scripts/fetch-fixtures.ps1)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check-examples.py --ace target/debug/ace
ACE_UPDATE_SKILL=1 cargo test -p altium-cli-extended --test skill   # 命令变更后重新生成 skills/
```

详见 [ARCHITECTURE.md](ARCHITECTURE.md) 了解各 Crate 的分层设计与二进制格式约定。

---

## 许可证

GPL-3.0-or-later，详见 [LICENSE](LICENSE)。[NOTICE](NOTICE) 致谢了编写 Altium 记录格式时参考的来源：KiCad 的 Altium 导入器（GPL-3.0-or-later）与 `python-altium` 格式文档。

**商标与许可声明：** `altium-cli-extended`（`ace`）是独立的开源项目，与 Altium LLC 或 Renesas Electronics Corporation 无隶属、背书或赞助关系。“Altium”与“Altium Designer”是 Altium LLC 的注册商标。原生命令（`backend: native`）使用独立实现的解析器直接操作用户设计文件；`designer` 命令（`backend: designer`）则需要用户自行提供单独的、拥有合法授权的 Altium Designer 安装环境。
