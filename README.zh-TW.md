<p align="center"><img src="https://raw.githubusercontent.com/Hybirdss/altium-cli-extended/main/docs/assets/banner.png" alt="altium-cli-extended" width="820"></p>

<div align="center">

# altium-cli-extended (`ace`)

**專為 AI 代理與 CI 打造的原生 Rust 解析器、位元組級安全編輯器與 Altium Designer 自動化橋接工具**

<a href="https://github.com/Hybirdss/altium-cli-extended/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/Hybirdss/altium-cli-extended/ci.yml?branch=main&label=CI" alt="CI"></a>
<a href="https://github.com/Hybirdss/altium-cli-extended/releases"><img src="https://img.shields.io/github/v/release/Hybirdss/altium-cli-extended?label=release" alt="latest release"></a>
<a href="https://github.com/Hybirdss/altium-cli-extended/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg" alt="GPL-3.0-or-later license"></a>

[English](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.md) | [한국어](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.ko.md) | [简体中文](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.zh-CN.md) | **繁體中文** | [日本語](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.ja.md) | [Español](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.es.md) | [हिन्दी](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.hi.md)

[雙後端架構](#原生與-designer-雙後端) · [安裝](#安裝) · [快速開始](#快速開始) · [代理技能](#代理技能) · [工作流程](#工作流程) · [指令群組](#指令群組) · [JSON 契約](#json-契約) · [開發](#開發)

</div>

> **正在使用 KiCad 10？** 請參考姊妹工具 **[kicad-cli-extended (`kce`)](https://github.com/Hybirdss/kicad-cli-extended)**。兩個專案皆採用 GPL-3.0-or-later 授權條款。

Altium Designer 官方並未提供命令列工具——它是一個在 Windows 上以 `X2.EXE` 執行的 GUI 應用程式。**`ace`** (Altium CLI Extended) 透過**雙後端架構**填補了這個空缺：

1. **原生 Rust 引擎（`backend: native`）：** **無需安裝 Altium Designer**，即可在 **Windows、macOS 與 Linux** 上直接讀取、檢視、比對（diff）、檢查並以位元組級安全（byte-safe）方式編輯 `.PrjPcb`、`.SchDoc`、`.PcbDoc`、`.SchLib`、`.PcbLib` 與 `.OutJob` 檔案。
2. **Designer 橋接後端（`backend: designer`）：** 當任務需要呼叫 Altium 引擎本身（如專案編譯、ERC、DRC、產生 Gerber/ODB++/PDF/STEP 等 `.OutJob` 輸出、呼叫任意 Altium 程序或執行 DelphiScript）時，橋接並驅動使用者自己擁有合法授權的 Altium Designer（支援本機 Windows、SSH 遠端 Windows/虛擬機，或 Linux Wine）。

每個指令都會輸出單一 JSON 信封格式，每次失敗皆回傳穩定的錯誤碼與復原提示，且 `ace commands` 與 `ace describe` 完整公開了所有指令的參數與回傳結構，讓 AI 代理與 CI 管線無需猜測即可可靠運作。

---

## 原生與 Designer 雙後端

| 面向 | 原生後端 (`backend: native`) | Designer 後端 (`backend: designer`) |
|---|---|---|
| **外部依賴** | 無（獨立單一執行檔） | 使用者合法授權的 Altium Designer (`X2.EXE`) |
| **支援平台** | Windows、macOS、Linux | 本機 Windows、SSH 遠端 Windows 或 Linux Wine |
| **支援功能** | 讀取全部檔案、匯出 BOM 與網表、設計檢查、版本差異比對、SVG 預覽、位元組級安全地修改參數/規則/變體/封裝 | 專案編譯、ERC、DRC、執行 `.OutJob` 發布任務、呼叫任意 Altium 程序與 DelphiScript |

### Designer 主機模式

* **本機 Windows（預設）：** 呼叫本機已登入桌面工作階段中的 Altium Designer（Windows 10 1809+ 使用 `conhost --headless`）。執行 `ace designer setup` 檢查前置條件，並依提示以桌面使用者身分執行 `ace designer setup --apply runner` 註冊排程工作（一般權限即可，無需系統管理員提權）。若安裝於非預設路徑，請將 `ACE_X2` 指向 `X2.EXE`。
* **SSH 遠端 Windows：** 在 macOS、Linux 或另一台 PC 上執行 `ace`，透過 SSH 驅動遠端 Windows 主機或虛擬機上的 Altium（自動同步專案檔案）。`ace` 對 `~/.ssh/known_hosts_altium` 強制啟用 `StrictHostKeyChecking=yes`，首次連線前需透過可信管道核對伺服器指紋並寫入該檔案。
* **Linux Wine（選用）：** 在 Linux 下透過 Wine 驅動 Altium Designer。

> **位元組級安全寫入與 Designer 說明：**
> * 原生寫入指令（`effect: write`）支援 `--dry-run`，預設保留首個備份檔 `<file>.bak`（除非傳入 `--no-backup`），並在替換原檔案前重新解析驗證。二進位文件修改不會更動未觸及的記錄與資料流。
> * Designer 已知限制：Designer 任務完成後，專案 `.PrjPcb` 及橋接腳本專案（`AltiumCliBridge.PrjScr`、`Job.PrjScr`）會保留在 Altium 的 Projects 面板中，且執行過 DRC 的 PCB 會顯示為已修改。清單變長時可在 Projects 面板中手動關閉，並對未編輯的檔案選擇 **「Don't Save」**。

---

## 安裝

### 預編譯執行檔

從 [GitHub Releases](https://github.com/Hybirdss/altium-cli-extended/releases) 下載對應平台的壓縮檔：

| 作業系統 | 壓縮檔名稱 | 執行檔 |
|---|---|---|
| **Windows x64** | `altium-cli-extended-x86_64-pc-windows-msvc.zip` | `ace.exe` |
| **macOS Apple Silicon** | `altium-cli-extended-aarch64-apple-darwin.tar.gz` | `ace` |
| **macOS Intel** | `altium-cli-extended-x86_64-apple-darwin.tar.gz` | `ace` |
| **Linux x86_64** | `altium-cli-extended-x86_64-unknown-linux-gnu.tar.gz` | `ace` |
| **Linux arm64** | `altium-cli-extended-aarch64-unknown-linux-gnu.tar.gz` | `ace` |

每個壓縮檔均附帶對應的 `<archive>.sha256` 校驗檔（macOS/Linux 使用 `shasum -a 256 -c`，Windows 使用 `Get-FileHash`）。解壓縮並將執行檔加入 `PATH`：

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

*macOS Gatekeeper 提示：* macOS 預編譯執行檔未簽署。驗證 SHA-256 無誤後，若 Gatekeeper 阻擋執行，可執行 `xattr -d com.apple.quarantine /usr/local/bin/ace` 移除隔離屬性。

### 從原始碼編譯（需 Rust 1.89+）

```bash
cargo install --locked --git https://github.com/Hybirdss/altium-cli-extended altium-cli-extended
```

執行 `ace doctor --quick` 可在不連線 Designer 主機的情況下快速檢查原生環境；執行 `ace doctor` 則會同時探測 Designer 主機狀態。

---

## 快速開始

```bash
ace inspect Board.PrjPcb                          # 檢視專案、原理圖、PCB 或元件庫概覽
ace bom Board.PrjPcb --out bom.csv                # 匯出分組 BOM（支援 --variant <name>）
ace netlist Board.PrjPcb --where name=GND         # 查詢指定網路及其連接的接腳
ace check Board.PrjPcb                            # 檢查元件編號、封裝及原理圖與 PCB 一致性
ace diff Board.PrjPcb --rev HEAD~1                # 與上一個 Git 提交比對差異
ace pcb rules Board.PcbDoc --kind Clearance --units mil
ace sch set-param Power.SchDoc R5 MPN RC0603FR-0710KL --dry-run
ace pcblib list Footprints.PcbLib --where mount=smd --fields name,pads
ace pcblib rename Footprints.PcbLib SOIC-8 SOIC-8N --dry-run
```

* **篩選與欄位選擇：** 所有清單指令均支援 `--fields`、`--where`（`key=value`、`key!=value`、`key~regex`、`key^=substring`）與 `--limit`。
* **單位與編碼：** 長度預設使用 `mm`（可透過 `--units mil|raw` 或 `ACE_UNITS` 修改）；舊版字元編碼可透過 `--codepage` 或 `ACE_CODEPAGE` 指定。

---

## 代理技能

儲存庫在 [`skills/altium-cli-extended/`](skills/altium-cli-extended/) 目錄下提供了由執行檔自動產生的 AI 代理技能包：包含核心規則與 12 條觸發式配方的 [`SKILL.md`](skills/altium-cli-extended/SKILL.md)，以及完整指令參考手冊 [`COMMANDS.md`](skills/altium-cli-extended/COMMANDS.md)。

```bash
ace skill install                          # 安裝至 ~/.agents/skills 及偵測到的代理（Claude、Codex、Gemini）
ace skill install --target claude          # 或 codex、gemini、agents、all
ace skill install --dir ./.claude/skills   # 安裝至專案目錄
ace skill status                           # 檢視已安裝副本及是否為最新
ace skill uninstall --target all
```

---

## 工作流程

從你的實際目標出發。以下範例中 `$P` 代表專案檔（`.PrjPcb`），`$B` 代表電路板（`.PcbDoc`），`$S` 代表原理圖紙（`.SchDoc`），`$R` 代表原始碼目錄外的報告目錄。請務必使用**絕對路徑**（以便正確解析類似 `..\Utils\*.SchDoc` 的上層相對路徑圖紙）。

| 你的目標 | 起始指令 | 對應章節 |
|---|---|---|
| 在未安裝 Altium 的情況下審查外包交付專案 | `ace project find <dir>` | [審查外包交付專案](#審查外包交付專案) |
| 比對兩個版本之間的原理圖/PCB/元件庫改動 | `ace diff A.PrjPcb B.PrjPcb --summary` | [比對版本差異](#比對版本差異) |
| 在 CI 中對設計問題進行門檻攔截 | `ace check P.PrjPcb --fail-on error` | [在 CI 中設定設計門檻](#在-ci-中設定設計門檻) |
| 不開啟 Altium 修改數值、組裝變體或設計規則 | `ace sch set-param ... --dry-run` | [免開啟 Altium 套用 ECO 變更](#免開啟-altium-套用-eco-變更) |
| 安全重新命名或複製封裝而不留下失效參照 | `ace pcblib rename ... --dry-run` | [安全重新命名封裝](#安全重新命名封裝) |
| 查詢接腳、網路、層疊結構與元件庫封裝 | `ace sch locate P.PrjPcb U1` | [尋找零件、網路與元件庫封裝](#尋找零件網路與元件庫封裝) |
| 執行完整編譯、ERC、DRC 並匯出 Gerber 等生產檔 | `ace designer ping --timeout 60` | [使用 Designer 準備生產發布](#使用-designer-準備生產發布) |
| 還原錯誤修改或恢復卡住的 Altium | `ace restore F --from first --dry-run` | [還原修改或恢復卡住的 Designer](#還原修改或恢復卡住的-designer) |
| 將 Altium 電路板遷移至 KiCad | `ace check P.PrjPcb --fail-on error` | [從 Altium 遷移至 KiCad](#從-altium-遷移至-kicad) |

### 審查外包交付專案

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

### 比對版本差異

```bash
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --summary
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --by part --ignore-field '^param:ComponentLink'
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --where change=part_added --fields ref
ace diff /absolute/RevA/Board.PcbDoc /absolute/RevB/Board.PcbDoc --geometry
ace diff "$P" --rev HEAD~1 --source pcb
```

### 在 CI 中設定設計門檻

```bash
ace check "$P" --list
ace check "$P" --pcb "$B" --require MPN,Manufacturer --fail-on error > "$R/check.json"
ace diff "$P" --rev HEAD~1 --exit-code
ace check "$P" --pcb "$B" --require MPN,Manufacturer --baseline "$R/check.json" --fail-on error
```

### 免開啟 Altium 套用 ECO 變更

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

### 安全重新命名封裝

```bash
ace sch find "$S" SOIC-8
ace pcb components "$B" --where footprint=SOIC-8 --fields designator,footprint
ace pcblib copy /absolute/Target.PcbLib /absolute/Source.PcbLib SOIC-8 --name SOIC-8N --dry-run
ace pcblib rename /absolute/Target.PcbLib SOIC-8 SOIC-8N --dry-run
ace sch set-footprint "$S" U3 SOIC-8N --dry-run
ace diff /absolute/before/Target.PcbLib /absolute/Target.PcbLib
```

### 尋找零件、網路與元件庫封裝

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

### 使用 Designer 準備生產發布

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

在 PowerShell 中，請使用參數陣列傳遞包含空白的路徑，並明確檢查 `$LASTEXITCODE`：

```powershell
$AceArgs = @('--json', '--read-only')
$Project = 'C:\Designs\My Board\Board.PrjPcb'
& ace.exe @AceArgs check $Project --fail-on error
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
```

### 還原修改或恢復卡住的 Designer

```bash
ace restore "$S" --from first --dry-run
ace restore "$S" --from first
ace designer status
ace designer dialogs
ace designer screenshot --out /tmp/designer.png
ace designer jobs --last 3
```

### 從 Altium 遷移至 KiCad

凍結一份原始專案副本並在兩個專案目錄之外記錄遞迴檔案/雜湊清單，然後匯入至明確指定的目標路徑：

```bash
kce project new /tmp/target --name target
kce --json --project /tmp/target pcb import /absolute/source/Board.PcbDoc --format altium --output /tmp/target/target.kicad_pcb --report-format json --report-file /tmp/import.json
kce --json --project /tmp/target pcb info
```

回讀目標電路板，確認回傳路徑等於 `/tmp/target/target.kicad_pcb`，檢查匯入的封裝數量與雜湊，並重新計算原始目錄清單（含檔案新增與刪除）以確保原始檔未被改動。請注意，成功匯入電路板並不等同於已驗證的完整遷移：原理圖/元件庫轉換與獨立的電氣一致性仍需單獨驗證。

---

## 指令群組

| 指令群組 | 用途 |
|---|---|
| `project` | 專案搜尋、文件清單、參數、組裝變體（及變體編輯）、專案設定 |
| `sch` | 原理圖紙：零件、接腳、網路、標籤、連接埠、階層結構，以及參數/註解/封裝/元件編號編輯 |
| `schlib` | 原理圖符號庫：符號、接腳、參數與描述編輯 |
| `pcb` | PCB 電路板：圖層、層疊（stackup）、零件、網路、規則、類別、圖元、孔、走線長度、SVG；規則/類別/參數編輯 |
| `pcblib` | 封裝庫：封裝、焊盤、3D 模型、SVG；位元組級安全的 `rename`、`set-param`、`remove`、`copy` |
| `outjob` | Output Job 檔案：輸出項、媒介、啟用/停用 |
| `bom`, `netlist`, `check`, `diff` | 跨文件分析：BOM 匯出、網表擷取、設計規則檢查、版本/Git 差異比對 |
| `designer` | 真實 Altium Designer 橋接：安裝設定、狀態、對話方塊偵測、截圖、編譯、ERC、DRC、Output Job、程序與腳本 |
| `raw` | 任意複合文件（OLE/CFB）的底層資料流與記錄檢視 |
| `commands`, `describe`, `skill`, `errors`, `doctor`, `inspect`, `restore` | 指令自探索、回傳結構說明、代理技能管理、環境診斷與備份還原 |

---

## JSON 契約

當透過管線呼叫或傳入 `--json` 時，`stdout` 嚴格輸出單一 JSON 物件：

```json
{"ok":true,"schema":"altium-cli-extended/v1","command":"pcb nets","data":{"count":2,"items":[...]},
 "warnings":[],"meta":{"units":"mm","backend":"native","effect":"read","dry_run":false,...}}
{"ok":false,"schema":"altium-cli-extended/v1","command":"pcb net","error":{"code":"NOT_FOUND","category":"input",
 "message":"no net \"CLKK\"","hint":"see `ace pcb nets` for the net names"}}
```

---

## 開發

```bash
git clone https://github.com/Hybirdss/altium-cli-extended
cd altium-cli-extended
scripts/fetch-fixtures.sh        # 下載測試範例專案至 .fixtures/ (Windows: scripts/fetch-fixtures.ps1)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check-examples.py --ace target/debug/ace
ACE_UPDATE_SKILL=1 cargo test -p altium-cli-extended --test skill   # 指令變更後重新產生 skills/
```

詳見 [ARCHITECTURE.md](ARCHITECTURE.md) 了解各 Crate 的分層設計與二進位格式慣例。

---

## 授權條款

GPL-3.0-or-later，詳見 [LICENSE](LICENSE)。[NOTICE](NOTICE) 致謝了編寫 Altium 記錄格式時參考的來源：KiCad 的 Altium 匯入器（GPL-3.0-or-later）與 `python-altium` 格式筆記。

**商標與授權聲明：** `altium-cli-extended`（`ace`）是獨立的開源專案，與 Altium LLC 或 Renesas Electronics Corporation 無隸屬、背書或贊助關係。「Altium」與「Altium Designer」是 Altium LLC 的註冊商標。原生指令（`backend: native`）使用獨立實作的解析器直接操作使用者設計檔案；`designer` 指令（`backend: designer`）則需要使用者自行提供單獨的、擁有合法授權的 Altium Designer 安裝環境。
