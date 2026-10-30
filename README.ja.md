<p align="center"><img src="https://raw.githubusercontent.com/Hybirdss/altium-cli-extended/main/docs/assets/banner.png" alt="altium-cli-extended" width="820"></p>

<div align="center">

# altium-cli-extended (`ace`)

**AIエージェントとCIのための、ネイティブRustパーサー・バイトセーフエディタ・Altium Designer自動化ブリッジ**

<a href="https://github.com/Hybirdss/altium-cli-extended/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/Hybirdss/altium-cli-extended/ci.yml?branch=main&label=CI" alt="CI"></a>
<a href="https://github.com/Hybirdss/altium-cli-extended/releases"><img src="https://img.shields.io/github/v/release/Hybirdss/altium-cli-extended?label=release" alt="latest release"></a>
<a href="https://github.com/Hybirdss/altium-cli-extended/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg" alt="GPL-3.0-or-later license"></a>

[English](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.md) | [한국어](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.ko.md) | [简体中文](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.zh-CN.md) | [繁體中文](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.zh-TW.md) | **日本語** | [Español](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.es.md) | [हिन्दी](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.hi.md)

[バックエンド構造](#ネイティブとdesignerの2つのバックエンド) · [インストール](#インストール) · [クイックスタート](#クイックスタート) · [エージェントスキル](#エージェントスキル) · [ワークフロー](#ワークフロー) · [コマンド群](#コマンドグループ) · [JSON契約](#json契約) · [開発](#開発)

</div>

> **KiCad 10をお使いですか？** 姉妹ツールである **[kicad-cli-extended (`kce`)](https://github.com/Hybirdss/kicad-cli-extended)** をご覧ください。両プロジェクトとも GPL-3.0-or-later ライセンスで提供されています。

Altium Designer には公式の CLI が存在せず、Windows 上で `X2.EXE` として動作する GUI アプリケーションです。**`ace`** (Altium CLI Extended) は、**デュアルバックエンド・アーキテクチャ**によってこのギャップを埋めます：

1. **ネイティブ Rust エンジン（`backend: native`）：** **Altium Designer のインストール不要**で、**Windows、macOS、Linux** 上で `.PrjPcb`、`.SchDoc`、`.PcbDoc`、`.SchLib`、`.PcbLib`、`.OutJob` ファイルを直接読み取り・検査・差分比較（diff）・チェックし、バイトセーフ（byte-safe）な編集まで行う単一バイナリです。
2. **Designer ブリッジ（`backend: designer`）：** プロジェクトのコンパイル、ERC、DRC、ガーバー/ODB++/PDF/STEP 等の `.OutJob` 出力、Altium プロセスや DelphiScript の実行など、Altium 本体のエンジンが必要な処理において、ユーザー自身が正規ライセンスを保有する Altium Designer（ローカル Windows、SSH 経由のリモート Windows/VM、または Linux 上の Wine）を駆動します。

すべてのコマンドは単一の JSON エンベロープを出力し、エラー時には安定したエラーコードと復旧ヒントを返し、`ace commands` と `ace describe` が全コマンド体系を自己記述するため、AIエージェントやCIパイプラインが推測に頼らず確実に作業できます。

---

## ネイティブとDesignerの2つのバックエンド

| 項目 | ネイティブ (`backend: native`) | Designer (`backend: designer`) |
|---|---|---|
| **必要な環境** | なし（単一バイナリで完結） | 正規ライセンスの Altium Designer (`X2.EXE`) |
| **動作OS** | Windows、macOS、Linux | ローカル Windows、SSH 経由のリモート Windows、または Linux Wine |
| **主な機能** | 全ファイルの読み取り、BOM、ネットリスト、設計チェック、リビジョン diff、SVG プレビュー、パラメータ/ルール/バリアント/フットプリントのバイトセーフ編集 | コンパイル、ERC、DRC、`.OutJob` 製造出力、任意の Altium プロセスおよび DelphiScript 実行 |

### Designer ホストモード

* **ローカル Windows（デフォルト）：** 同一マシンのログイン済みデスクトップセッションにある Altium Designer を使用します（Windows 10 1809+ では `conhost --headless` を使用）。`ace designer setup` で前提条件を確認し、案内されたらデスクトップユーザーとして `ace designer setup --apply runner` を実行してスケジュールタスクを登録します（管理者権限は不要）。非標準パスにインストールされている場合は `ACE_X2` に `X2.EXE` のパスを指定します。
* **SSH 経由のリモート Windows：** macOS、Linux、または別の PC で `ace` を実行し、SSH 接続可能な Windows マシンや VM 上の Altium を駆動します（プロジェクトは自動同期されます）。`ace` は `~/.ssh/known_hosts_altium` に対して `StrictHostKeyChecking=yes` を強制するため、セットアップ前に信頼できる経路でサーバーのフィンガープリントを確認し、ホスト鍵を登録してください。
* **Linux 上の Wine（オプション）：** Wine 環境下の Altium Designer を駆動します。

> **バイトセーフ書き込みと Designer の注意事項：**
> * ネイティブ書き込み（`effect: write`）は `--dry-run` に対応し、`--no-backup` を指定しない限り最初の `<file>.bak` を保持し、ファイルを置き換える前に変更後のバイト列を再パースして検証します。バイナリドキュメントの編集では、変更対象外のレコードやストリームは一切変更されません。
> * 既知の制限事項：Designer ジョブを実行すると、`.PrjPcb` およびブリッジ用スクリプトプロジェクト（`AltiumCliBridge.PrjScr`、`Job.PrjScr`）が Altium の Projects パネルに開いたまま残り、DRC を実行した基板は変更済み（Modified）として表示されます。リストが増えた場合は Projects パネルから手動で閉じ、編集していないファイルには **「Don't Save」** を選択してください。

---

## インストール

### ビルド済みバイナリ

[GitHub Releases](https://github.com/Hybirdss/altium-cli-extended/releases) からお使いの OS 向けのアーカイブをダウンロードします：

| OS | アーカイブ名 | バイナリ |
|---|---|---|
| **Windows x64** | `altium-cli-extended-x86_64-pc-windows-msvc.zip` | `ace.exe` |
| **macOS Apple Silicon** | `altium-cli-extended-aarch64-apple-darwin.tar.gz` | `ace` |
| **macOS Intel** | `altium-cli-extended-x86_64-apple-darwin.tar.gz` | `ace` |
| **Linux x86_64** | `altium-cli-extended-x86_64-unknown-linux-gnu.tar.gz` | `ace` |
| **Linux arm64** | `altium-cli-extended-aarch64-unknown-linux-gnu.tar.gz` | `ace` |

各アーカイブには `<archive>.sha256` チェックサムファイルが付属しています（macOS/Linux は `shasum -a 256 -c`、Windows は `Get-FileHash`）。展開してバイナリを `PATH` に配置してください：

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

*macOS Gatekeeper に関する注意：* macOS バイナリは未署名です。チェックサムを確認後、Gatekeeper にブロックされた場合は `xattr -d com.apple.quarantine /usr/local/bin/ace` を実行してください。

### ソースからビルド（Rust 1.89 以降）

```bash
cargo install --locked --git https://github.com/Hybirdss/altium-cli-extended altium-cli-extended
```

Designer ホストに接続せずネイティブ機能の準備状態だけを確認するには `ace doctor --quick` を、Designer ホストも含めて確認するには `ace doctor` を実行します。

---

## クイックスタート

```bash
ace inspect Board.PrjPcb                          # プロジェクト、シート、基板、ライブラリの概要表示
ace bom Board.PrjPcb --out bom.csv                # グループ化された BOM 出力（バリアントは --variant <name>）
ace netlist Board.PrjPcb --where name=GND         # 特定のネットと接続ピンを表示
ace check Board.PrjPcb                            # アノテーション、フットプリント、回路図 vs PCB の整合性検査
ace diff Board.PrjPcb --rev HEAD~1                # 直前の Git コミットとの差分比較
ace pcb rules Board.PcbDoc --kind Clearance --units mil
ace sch set-param Power.SchDoc R5 MPN RC0603FR-0710KL --dry-run
ace pcblib list Footprints.PcbLib --where mount=smd --fields name,pads
ace pcblib rename Footprints.PcbLib SOIC-8 SOIC-8N --dry-run
```

* **フィルタとフィールド選択：** すべてのリスト出力は `--fields`、`--where`（`key=value`、`key!=value`、`key~regex`、`key^=substring`）、`--limit` で整形できます。
* **単位と文字コード：** 長さの単位はデフォルトで `mm` です（`--units mil|raw` または `ACE_UNITS` で変更可能）。レガシー文字コードは `--codepage` または `ACE_CODEPAGE` で設定します。

---

## エージェントスキル

リポジトリの [`skills/altium-cli-extended/`](skills/altium-cli-extended/) には、バイナリから自動生成された AI エージェント用スキルが含まれています：基本ルールと12個のトリガーレシピをまとめた [`SKILL.md`](skills/altium-cli-extended/SKILL.md) と、全コマンドリファレンス [`COMMANDS.md`](skills/altium-cli-extended/COMMANDS.md) です。

```bash
ace skill install                          # ~/.agents/skills および検出されたエージェント（Claude, Codex, Gemini）にインストール
ace skill install --target claude          # または codex, gemini, agents, all
ace skill install --dir ./.claude/skills   # プロジェクトローカルにインストール
ace skill status                           # インストール済みコピーと最新状態の確認
ace skill uninstall --target all
```

---

## ワークフロー

目的から選んで開始できます。以下のコマンド例で `$P` はプロジェクト（`.PrjPcb`）、`$B` は基板（`.PcbDoc`）、`$S` は回路図シート（`.SchDoc`）、`$R` はソース外のレポート出力先ディレクトリです。常に**絶対パス**を使用してください（`..\Utils\*.SchDoc` のような親相対パスのシートを解決するため）。

| やりたいこと | 最初のコマンド | セクション |
|---|---|---|
| Altium なしで外部委託先の納品プロジェクトをレビュー | `ace project find <dir>` | [外部委託先の納品物をレビューする](#外部委託先の納品物をレビューする) |
| 2つのリビジョン間の変更点を確認 | `ace diff A.PrjPcb B.PrjPcb --summary` | [リビジョン間を比較する](#リビジョン間を比較する) |
| CI で設計エラー時にビルドを失敗させる | `ace check P.PrjPcb --fail-on error` | [CIで設計ゲートをかける](#ciで設計ゲートをかける) |
| Altium を開かずに定数変更・バリアント追加・ルール変更 | `ace sch set-param ... --dry-run` | [Altiumを開かずにECOを適用する](#altiumを開かずにecoを適用する) |
| 参照切れを起こさずにフットプリントをリネーム | `ace pcblib rename ... --dry-run` | [参照を壊さずにフットプリント名を変更する](#参照を壊さずにフットプリント名を変更する) |
| ピン、ネット、層構成、ライブラリフットプリントを検索 | `ace sch locate P.PrjPcb U1` | [部品・ネット・フットプリントを検索する](#部品ネットフットプリントを検索する) |
| ガーバー生成、コンパイル、ERC、DRC を実行 | `ace designer ping --timeout 60` | [Designerでリリースを準備する](#designerでリリースを準備する) |
| 誤った編集の復元やハングした Altium の復旧 | `ace restore F --from first --dry-run` | [編集の復元とハングしたDesignerの復旧](#編集の復元とハングしたdesignerの復旧) |
| Altium 基板を KiCad へ移行 | `ace check P.PrjPcb --fail-on error` | [AltiumからKiCadへの移行](#altiumからkicadへの移行) |

### 外部委託先の納品物をレビューする

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

### リビジョン間を比較する

```bash
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --summary
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --by part --ignore-field '^param:ComponentLink'
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --where change=part_added --fields ref
ace diff /absolute/RevA/Board.PcbDoc /absolute/RevB/Board.PcbDoc --geometry
ace diff "$P" --rev HEAD~1 --source pcb
```

### CIで設計ゲートをかける

```bash
ace check "$P" --list
ace check "$P" --pcb "$B" --require MPN,Manufacturer --fail-on error > "$R/check.json"
ace diff "$P" --rev HEAD~1 --exit-code
ace check "$P" --pcb "$B" --require MPN,Manufacturer --baseline "$R/check.json" --fail-on error
```

### Altiumを開かずにECOを適用する

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

### 参照を壊さずにフットプリント名を変更する

```bash
ace sch find "$S" SOIC-8
ace pcb components "$B" --where footprint=SOIC-8 --fields designator,footprint
ace pcblib copy /absolute/Target.PcbLib /absolute/Source.PcbLib SOIC-8 --name SOIC-8N --dry-run
ace pcblib rename /absolute/Target.PcbLib SOIC-8 SOIC-8N --dry-run
ace sch set-footprint "$S" U3 SOIC-8N --dry-run
ace diff /absolute/before/Target.PcbLib /absolute/Target.PcbLib
```

### 部品・ネット・フットプリントを検索する

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

### Designerでリリースを準備する

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

PowerShell では空白を含むパスを引数配列で渡し、`$LASTEXITCODE` を明示的に確認してください：

```powershell
$AceArgs = @('--json', '--read-only')
$Project = 'C:\Designs\My Board\Board.PrjPcb'
& ace.exe @AceArgs check $Project --fail-on error
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
```

### 編集の復元とハングしたDesignerの復旧

```bash
ace restore "$S" --from first --dry-run
ace restore "$S" --from first
ace designer status
ace designer dialogs
ace designer screenshot --out /tmp/designer.png
ace designer jobs --last 3
```

### AltiumからKiCadへの移行

ソースのコピーを凍結し、両プロジェクトの外部に再帰的なファイル/ハッシュマニフェストを記録した上で、明示的な出力パスを指定してインポートします：

```bash
kce project new /tmp/target --name target
kce --json --project /tmp/target pcb import /absolute/source/Board.PcbDoc --format altium --output /tmp/target/target.kicad_pcb --report-format json --report-file /tmp/import.json
kce --json --project /tmp/target pcb info
```

ターゲット基板をリードバックし、報告されたパスが `/tmp/target/target.kicad_pcb` と一致すること、インポートされたフットプリントとハッシュが空基板と異なること、およびソースマニフェスト（ファイルの追加・削除を含む）が完全に同一であることを確認してください。基板インポートの成功は、検証済みの完全な移行を意味するものではありません（シンボル/ライブラリ変換および独立した電気的等価性の検証が別途必要です）。

---

## コマンドグループ

| グループ | 用途 |
|---|---|
| `project` | プロジェクト検索、ドキュメント一覧、パラメータ、バリアント（および編集）、設定 |
| `sch` | 回路図シート：部品、ピン、ネット、ラベル、ポート、階層構造、パラメータ/コメント/フットプリント/デジグネータ編集 |
| `schlib` | 回路図シンボルライブラリ：シンボル、ピン、パラメータおよび説明の編集 |
| `pcb` | PCB 基板：レイヤー、層構成（stackup）、部品、ネット、ルール、クラス、プリミティブ、穴、配線長、SVG；ルール/クラス/パラメータ編集 |
| `pcblib` | フットプリントライブラリ：フットプリント、パッド、3Dモデル、SVG；バイトセーフな `rename`、`set-param`、`remove`、`copy` |
| `outjob` | Output Job ファイル：出力項目、メディア、有効化/無効化 |
| `bom`, `netlist`, `check`, `diff` | ドキュメント横断解析：BOM 生成、ネットリスト抽出、設計チェック、リビジョン/Git diff |
| `designer` | 実 Altium Designer ブリッジ：セットアップ、ステータス、ダイアログ検出、スクリーンショット、コンパイル、ERC、DRC、Output Job、プロセス、スクリプト |
| `raw` | 任意の複合ファイル（OLE/CFB）のストリームおよびレコードの低レベル検査 |
| `commands`, `describe`, `skill`, `errors`, `doctor`, `inspect`, `restore` | コマンド検索、スキーマ/戻り値構造の説明、エージェントスキル管理、環境診断、バックアップ復元 |

---

## JSON契約

パイプ接続時または `--json` 指定時、`stdout` には厳密に1つの JSON オブジェクトが出力されます：

```json
{"ok":true,"schema":"altium-cli-extended/v1","command":"pcb nets","data":{"count":2,"items":[...]},
 "warnings":[],"meta":{"units":"mm","backend":"native","effect":"read","dry_run":false,...}}
{"ok":false,"schema":"altium-cli-extended/v1","command":"pcb net","error":{"code":"NOT_FOUND","category":"input",
 "message":"no net \"CLKK\"","hint":"see `ace pcb nets` for the net names"}}
```

---

## 開発

```bash
git clone https://github.com/Hybirdss/altium-cli-extended
cd altium-cli-extended
scripts/fetch-fixtures.sh        # サンプルプロジェクトを .fixtures/ に取得 (Windows: scripts/fetch-fixtures.ps1)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check-examples.py --ace target/debug/ace
ACE_UPDATE_SKILL=1 cargo test -p altium-cli-extended --test skill   # コマンド変更後に skills/ を再生成
```

クレート構成とバイナリフォーマット規約については [ARCHITECTURE.md](ARCHITECTURE.md) を参照してください。

---

## ライセンス

GPL-3.0-or-later（[LICENSE](LICENSE) を参照）。[NOTICE](NOTICE) には、Altium レコードレイアウトの実装にあたり参照した KiCad の Altium インポーター（GPL-3.0-or-later）および `python-altium` のフォーマットノートのクレジットが記載されています。

**商標およびライセンスに関する免責事項：** `altium-cli-extended`（`ace`）は独立したオープンソースプロジェクトであり、Altium LLC または Renesas Electronics Corporation と提携、承認、または後援されたものではありません。「Altium」および「Altium Designer」は Altium LLC の登録商標です。ネイティブコマンド（`backend: native`）は独自のパーサーを使用してユーザーの設計ファイルを直接操作します。`designer` コマンド（`backend: designer`）を使用するには、ユーザーが別途正規のライセンスを保有する Altium Designer 環境が必要です。
