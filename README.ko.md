<p align="center"><img src="https://raw.githubusercontent.com/Hybirdss/altium-cli-extended/main/docs/assets/banner.png" alt="altium-cli-extended" width="820"></p>

<div align="center">

# altium-cli-extended (`ace`)

**AI 에이전트와 CI를 위한 네이티브 Rust 파서, 바이트 안전 편집기 및 Altium Designer 자동화 브리지**

<a href="https://github.com/Hybirdss/altium-cli-extended/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/Hybirdss/altium-cli-extended/ci.yml?branch=main&label=CI" alt="CI"></a>
<a href="https://github.com/Hybirdss/altium-cli-extended/releases"><img src="https://img.shields.io/github/v/release/Hybirdss/altium-cli-extended?label=release" alt="latest release"></a>
<a href="https://github.com/Hybirdss/altium-cli-extended/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg" alt="GPL-3.0-or-later license"></a>

[English](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.md) | **한국어** | [简体中文](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.zh-CN.md) | [繁體中文](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.zh-TW.md) | [日本語](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.ja.md) | [Español](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.es.md) | [हिन्दी](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.hi.md)

[백엔드 구조](#네이티브-및-designer-백엔드) · [설치](#설치) · [빠른 시작](#빠른-시작) · [에이전트 스킬](#에이전트-스킬) · [워크플로](#워크플로) · [명령어 그룹](#명령어-그룹) · [JSON 규약](#json-규약) · [개발](#개발)

</div>

> **KiCad 10을 사용하시나요?** 자매 도구인 **[kicad-cli-extended (`kce`)](https://github.com/Hybirdss/kicad-cli-extended)**를 확인해 보세요. 두 프로젝트 모두 GPL-3.0-or-later 라이선스를 따릅니다.

Altium Designer는 공식 CLI를 제공하지 않으며 `X2.EXE`로 실행되는 Windows 전용 GUI 애플리케이션입니다. **`ace`** (Altium CLI Extended)는 이 공백을 메우기 위해 **이중 백엔드 아키텍처**를 제공합니다:

1. **네이티브 Rust 엔진 (`backend: native`):** **Altium Designer가 전혀 설치되어 있지 않아도** **Windows, macOS, Linux**에서 `.PrjPcb`, `.SchDoc`, `.PcbDoc`, `.SchLib`, `.PcbLib`, `.OutJob` 파일을 직접 읽고, 검사하고, 리비전 비교(diff) 및 설계 규칙 검사를 수행하며, 바이트 안전(byte-safe) 편집까지 처리하는 단일 바이너리입니다.
2. **Designer 브리지 (`backend: designer`):** 프로젝트 컴파일, ERC, DRC, Gerber/ODB++/PDF/STEP `.OutJob` 출력, Altium 프로세스 및 DelphiScript 실행처럼 Altium 엔진이 필요한 작업이 있을 때 사용자가 보유한 정품 Altium Designer(로컬 Windows, SSH 원격 Windows/VM, 또는 Linux Wine)를 직접 구동합니다.

모든 명령어는 단일 JSON 봉투(Envelope)를 출력하고, 모든 실패는 고정 에러 코드와 복구 힌트를 반환하며, `ace commands`와 `ace describe`가 전체 명령어 체계를 스스로 설명하므로 AI 에이전트와 CI 파이프라인이 추측 없이 정확하게 작업할 수 있습니다.

---

## 네이티브 및 Designer 백엔드

| 항목 | 네이티브 (`backend: native`) | Designer (`backend: designer`) |
|---|---|---|
| **필요 환경** | 없음 (단독 실행 바이너리) | 사용자 라이선스가 있는 Altium Designer (`X2.EXE`) |
| **지원 OS** | Windows, macOS, Linux | 로컬 Windows, SSH 원격 Windows, 또는 Linux Wine |
| **수행 작업** | 프로젝트/시트/보드/라이브러리 전체 조회, BOM, 넷리스트, 설계 검사, 리비전 diff, SVG 프리뷰, 파라미터/규칙/변형/풋프린트 바이트 안전 편집 | 컴파일, ERC, DRC, `.OutJob` 제조 파일 출력, 모든 Altium 프로세스 및 DelphiScript 실행 |

### Designer 호스트 모드

* **로컬 Windows (기본값):** 동일 머신의 로그인된 데스크톱 세션에 설치된 Altium Designer를 사용합니다(Windows 10 1809+에서는 `conhost --headless` 사용). `ace designer setup`으로 사전 요구사항을 점검하고, 안내에 따라 데스크톱 사용자 권한(관리자 권한 불필요)으로 `ace designer setup --apply runner`를 실행해 예약 작업을 등록하세요. 비표준 경로에 설치된 경우 `ACE_X2` 환경 변수에 `X2.EXE` 경로를 지정합니다.
* **SSH 원격 Windows:** macOS, Linux 또는 다른 PC에서 `ace`를 실행하고 SSH로 연결된 Windows 머신이나 VM의 Altium을 구동합니다(프로젝트 자동 동기화 포함). `ace`는 `~/.ssh/known_hosts_altium`에 대해 `StrictHostKeyChecking=yes`를 강제하므로, 설정 전에 신뢰할 수 있는 채널로 서버 지문을 확인하고 호스트 키를 등록해야 합니다.
* **Linux Wine (선택 사항):** Wine 환경에서 실행되는 Altium Designer를 구동합니다.

> **바이트 안전 쓰기 및 Designer 참고사항:**
> * 네이티브 쓰기(`effect: write`)는 `--dry-run`을 지원하며, `--no-backup`을 주지 않는 한 최초 원본을 `<file>.bak`에 보존하고, 파일을 교체하기 전에 수정된 바이트를 다시 파싱해 무결성을 검증합니다. 바이너리 문서 편집 시 수정하지 않은 레코드와 스트림은 원본 그대로 유지됩니다.
> * 알려진 제한 사항: Designer 작업이 끝나면 `.PrjPcb`와 브리지 스크립트 프로젝트(`AltiumCliBridge.PrjScr`, `Job.PrjScr`)가 Altium의 Projects 패널에 열린 채로 남으며, DRC를 실행한 보드는 수정됨(Modified) 상태로 표시됩니다. 목록이 많아지면 Projects 패널에서 직접 닫고, 편집하지 않은 항목에는 **"Don't Save"**를 선택하세요.

---

## 설치

### 사전 빌드된 바이너리

[GitHub Releases](https://github.com/Hybirdss/altium-cli-extended/releases)에서 운영체제에 맞는 압축 파일을 다운로드하세요:

| 운영체제 | 아카이브 파일 | 바이너리 |
|---|---|---|
| **Windows x64** | `altium-cli-extended-x86_64-pc-windows-msvc.zip` | `ace.exe` |
| **macOS Apple Silicon** | `altium-cli-extended-aarch64-apple-darwin.tar.gz` | `ace` |
| **macOS Intel** | `altium-cli-extended-x86_64-apple-darwin.tar.gz` | `ace` |
| **Linux x86_64** | `altium-cli-extended-x86_64-unknown-linux-gnu.tar.gz` | `ace` |
| **Linux arm64** | `altium-cli-extended-aarch64-unknown-linux-gnu.tar.gz` | `ace` |

각 아카이브에는 체크섬 파일(`<archive>.sha256`)이 함께 제공됩니다(macOS/Linux는 `shasum -a 256 -c`, Windows는 `Get-FileHash`). 압축을 풀고 바이너리를 `PATH`에 추가하세요:

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

*macOS Gatekeeper 참고:* macOS 바이너리는 서명되어 있지 않습니다. 체크섬 확인 후 Gatekeeper가 실행을 차단하면 `xattr -d com.apple.quarantine /usr/local/bin/ace`로 격리 속성을 해제하세요.

### 소스에서 빌드 (Rust 1.89 이상)

```bash
cargo install --locked --git https://github.com/Hybirdss/altium-cli-extended altium-cli-extended
```

호스트 연결 없이 네이티브 준비 상태만 빠르게 점검하려면 `ace doctor --quick`을, Designer 호스트까지 함께 점검하려면 `ace doctor`를 실행하세요.

---

## 빠른 시작

```bash
ace inspect Board.PrjPcb                          # 프로젝트, 시트, 보드, 라이브러리 요약 정보 확인
ace bom Board.PrjPcb --out bom.csv                # 그룹화된 BOM 추출 (변형은 --variant <name>)
ace netlist Board.PrjPcb --where name=GND         # 특정 넷과 연결된 핀 목록 조회
ace check Board.PrjPcb                            # 부품 번호, 풋프린트, 회로도 vs PCB 정합성 검사
ace diff Board.PrjPcb --rev HEAD~1                # 이전 Git 커밋과 비교
ace pcb rules Board.PcbDoc --kind Clearance --units mil
ace sch set-param Power.SchDoc R5 MPN RC0603FR-0710KL --dry-run
ace pcblib list Footprints.PcbLib --where mount=smd --fields name,pads
ace pcblib rename Footprints.PcbLib SOIC-8 SOIC-8N --dry-run
```

* **필터링 및 필드 선택:** 모든 목록 명령어는 `--fields`, `--where` (`key=value`, `key!=value`, `key~regex`, `key^=substring`), `--limit`으로 원하는 행과 열만 추출할 수 있습니다.
* **단위 및 인코딩:** 길이 단위는 기본 `mm`이며 `--units mil|raw` 또는 `ACE_UNITS` 환경 변수로 변경할 수 있습니다. 레거시 문자열 인코딩은 `--codepage` 또는 `ACE_CODEPAGE`로 설정합니다.

---

## 에이전트 스킬

저장소의 [`skills/altium-cli-extended/`](skills/altium-cli-extended/) 디렉터리에는 바이너리에서 자동 생성되는 AI 에이전트 스킬이 포함되어 있습니다: 핵심 규칙과 12가지 트리거 레시피를 담은 [`SKILL.md`](skills/altium-cli-extended/SKILL.md)와 전체 명령어 레퍼런스인 [`COMMANDS.md`](skills/altium-cli-extended/COMMANDS.md)로 구성됩니다.

```bash
ace skill install                          # ~/.agents/skills 및 감지된 모든 에이전트(Claude, Codex, Gemini)에 설치
ace skill install --target claude          # 또는 codex, gemini, agents, all
ace skill install --dir ./.claude/skills   # 프로젝트 내부에 설치
ace skill status                           # 설치된 복사본 및 최신 여부 확인
ace skill uninstall --target all
```

---

## 워크플로

원하는 작업 목표에서 바로 시작하세요. 아래 예제에서 `$P`는 프로젝트(`.PrjPcb`), `$B`는 보드(`.PcbDoc`), `$S`는 회로도 시트(`.SchDoc`), `$R`은 소스 외부의 리포트 디렉터리를 뜻합니다. 모든 경로는 **절대 경로**를 사용하세요(`..\Utils\*.SchDoc` 같은 상위 상대 경로 시트를 올바르게 찾기 위함).

| 원하는 작업 | 첫 번째 명령어 | 섹션 |
|---|---|---|
| Altium 없이 외주 인수인계 프로젝트 검토 | `ace project find <dir>` | [외주 인수인계 프로젝트 검토](#외주-인수인계-프로젝트-검토) |
| 두 리비전 간 변경 사항 비교 | `ace diff A.PrjPcb B.PrjPcb --summary` | [리비전 간 비교](#리비전-간-비교) |
| CI에서 설계 오류 시 빌드 실패 처리 | `ace check P.PrjPcb --fail-on error` | [CI에서 설계 게이트 적용](#ci에서-설계-게이트-적용) |
| Altium을 열지 않고 부품 값/변형/규칙 수정 | `ace sch set-param ... --dry-run` | [Altium 없이 ECO 적용](#altium-없이-eco-적용) |
| 참조 누락 없이 안전하게 풋프린트 이름 변경 | `ace pcblib rename ... --dry-run` | [참조 깨짐 없이 풋프린트 이름 변경](#참조-깨짐-없이-풋프린트-이름-변경) |
| 특정 핀, 넷, 적층 구조, 라이브러리 풋프린트 찾기 | `ace sch locate P.PrjPcb U1` | [부품, 넷, 라이브러리 풋프린트 탐색](#부품-넷-라이브러리-풋프린트-탐색) |
| Gerber 생성, 컴파일, ERC, DRC 실행 | `ace designer ping --timeout 60` | [Designer로 릴리스 패키지 준비](#designer로-릴리스-패키지-준비) |
| 잘못된 편집 복원 또는 멈춘 Altium 복구 | `ace restore F --from first --dry-run` | [편집 복원 또는 멈춘 Designer 복구](#편집-복원-또는-멈춘-designer-복구) |
| Altium 보드를 KiCad로 마이그레이션 | `ace check P.PrjPcb --fail-on error` | [Altium에서 KiCad로 변환](#altium에서-kicad로-변환) |

### 외주 인수인계 프로젝트 검토

프로젝트를 탐색한 뒤 절대 경로를 지정하고, 리포트는 소스 외부에 저장합니다. `ACE_READONLY=1`을 설정하면 파일이나 Designer에 접근하기 전에 모든 쓰기 및 제어 명령이 차단됩니다:

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

* `ace check --list`로 각 발견 사항의 `code` 의미와 해결 방법을 확인하세요.
* BOM을 신뢰하기 전에 `missing_documents`, `incomplete`, 선언된 시트 수와 실제 발견된 시트 수를 비교하고 회로도와 PCB의 부품 수를 대조하세요.

### 리비전 간 비교

```bash
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --summary
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --by part --ignore-field '^param:ComponentLink'
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --where change=part_added --fields ref
ace diff /absolute/RevA/Board.PcbDoc /absolute/RevB/Board.PcbDoc --geometry
ace diff "$P" --rev HEAD~1 --source pcb
```

* 먼저 `--summary`를 확인한 뒤 `--where`와 `--fields`로 필요한 행만 좁혀 보세요.
* 부품은 Designator보다 Unique ID로 먼저 매칭되므로, 재번호 부여(Re-annotation)는 `part_renamed`로 표시되며 동시에 변경된 파라미터 차이도 보존됩니다.
* 회로도에서 `identical: true`가 나와도 보드 변경 여부는 알 수 없으므로 반드시 `.PcbDoc` diff(또는 `--source pcb`)도 함께 실행하세요.

### CI에서 설계 게이트 적용

```bash
ace check "$P" --list
ace check "$P" --pcb "$B" --require MPN,Manufacturer --fail-on error > "$R/check.json"
ace diff "$P" --rev HEAD~1 --exit-code
ace check "$P" --pcb "$B" --require MPN,Manufacturer --baseline "$R/check.json" --fail-on error
```

* `check --fail-on error`와 `diff --exit-code`는 게이트 조건에 걸리면 `stdout`에 정상 JSON을 출력하면서 종료 코드 `9`를 반환합니다.
* `--baseline`을 지정하면 기존에 알려진 경고를 제외하고 **새로 발생한** 위반 사항만 카운트합니다.
* 종료 코드 `10`, `11`, `12`는 Altium 호스트가 없거나 실패했거나 사용 중임을 뜻하며 설계 자체의 결함이 아닙니다.

### Altium 없이 ECO 적용

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

* 각 드라이런에서 `changes[].before`를 확인하세요(`=Value`는 간접 참조 링크이므로 `Value` 파라미터를 수정하거나 `--literal`을 사용해야 합니다).
* 회로도를 수정해도 Altium에서 Update PCB를 실행하기 전까지는 PCB에 이전 코멘트/풋프린트가 남아 있으므로 `comment-mismatch`가 발생하는 것이 정상입니다.

### 참조 깨짐 없이 풋프린트 이름 변경

```bash
ace sch find "$S" SOIC-8
ace pcb components "$B" --where footprint=SOIC-8 --fields designator,footprint
ace pcblib copy /absolute/Target.PcbLib /absolute/Source.PcbLib SOIC-8 --name SOIC-8N --dry-run
ace pcblib rename /absolute/Target.PcbLib SOIC-8 SOIC-8N --dry-run
ace sch set-footprint "$S" U3 SOIC-8N --dry-run
ace diff /absolute/before/Target.PcbLib /absolute/Target.PcbLib
```

* 이름을 바꾸기 전에 해당 풋프린트를 참조하는 모든 곳을 먼저 조회하세요. `pcblib copy`는 **대상(Destination) 라이브러리가 첫 번째 인자**입니다.

### 부품, 넷, 라이브러리 풋프린트 탐색

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

### Designer로 릴리스 패키지 준비

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

* 컴파일 오류, ERC 오류, DRC 위반, 미배선(unrouted) 연결이 하나라도 있으면 출력을 중단하세요.
* PowerShell에서는 공백이 포함된 경로를 배열로 전달하고 `$LASTEXITCODE`를 명시적으로 확인하세요:

```powershell
$AceArgs = @('--json', '--read-only')
$Project = 'C:\Designs\My Board\Board.PrjPcb'
& ace.exe @AceArgs check $Project --fail-on error
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
```

### 편집 복원 또는 멈춘 Designer 복구

```bash
ace restore "$S" --from first --dry-run
ace restore "$S" --from first
ace designer status
ace designer dialogs
ace designer screenshot --out /tmp/designer.png
ace designer jobs --last 3
```

* 네이티브 쓰기는 최초 원본을 `<file>.bak`에 보관하며, `ace restore FILE --from first`는 현재 파일을 `<file>.pre-restore`에 백업한 뒤 원본을 복원합니다.
* Altium이 모달 창 때문에 멈춘 경우 `ace designer reset`을 실행하기 전에 `ace designer dialogs`와 `ace designer screenshot`으로 원인을 먼저 확인하세요.

### Altium에서 KiCad로 변환

원본 프로젝트의 복사본을 동결하고 두 프로젝트 외부에 재귀적 파일/해시 매니페스트를 기록한 뒤, 명시적인 출력 경로로 가져오기를 실행하세요:

```bash
kce project new /tmp/target --name target
kce --json --project /tmp/target pcb import /absolute/source/Board.PcbDoc --format altium --output /tmp/target/target.kicad_pcb --report-format json --report-file /tmp/import.json
kce --json --project /tmp/target pcb info
```

반환된 경로가 `/tmp/target/target.kicad_pcb`와 일치하는지, 가져온 풋프린트 수와 해시가 빈 보드와 다른지, 그리고 원본 매니페스트(파일 추가 및 삭제 포함)가 그대로 보존되었는지 확인하세요. 보드 가져오기가 성공했더라도 심볼/라이브러리 변환과 독립적인 전기적 동등성 검증이 별도로 필요합니다.

---

## 명령어 그룹

| 그룹 | 설명 |
|---|---|
| `project` | 프로젝트 탐색, 문서 목록, 파라미터, 변형(Variant) 조회 및 수정, 설정 |
| `sch` | 회로도 시트: 부품, 핀, 넷, 라벨, 포트, 계층 구조, 파라미터/코멘트/풋프린트/Designator 수정 |
| `schlib` | 회로도 심볼 라이브러리: 심볼, 핀, 파라미터 및 설명 수정 |
| `pcb` | PCB 보드: 레이어, 적층 구조(stackup), 부품, 넷, 규칙, 클래스, 프리미티브, 홀, 배선 길이, SVG; 규칙/클래스/파라미터 수정 |
| `pcblib` | 풋프린트 라이브러리: 풋프린트, 패드, 3D 모델, SVG; 바이트 안전 `rename`, `set-param`, `remove`, `copy` |
| `outjob` | Output Job 파일: 출력 항목, 미디어, 활성화/비활성화 |
| `bom`, `netlist`, `check`, `diff` | 문서 간 교차 분석: BOM 생성, 넷리스트 추출, 설계 정합성 검사, 리비전/Git diff |
| `designer` | 실제 Altium Designer 브리지: 설정, 상태, 대화상자, 스크린샷, 컴파일, ERC, DRC, Output Job, 프로세스, 스크립트 |
| `raw` | 모든 복합 파일(OLE/CFB)의 저수준 스트림 및 레코드 조회 |
| `commands`, `describe`, `skill`, `errors`, `doctor`, `inspect`, `restore` | 명령어 탐색, 스키마/반환 구조 설명, 에이전트 스킬 관리, 환경 진단 및 백업 복원 |

---

## JSON 규약

파이프 연결 시 또는 `--json` 옵션 사용 시 `stdout`에 정확히 하나의 JSON 객체를 출력합니다:

```json
{"ok":true,"schema":"altium-cli-extended/v1","command":"pcb nets","data":{"count":2,"items":[...]},
 "warnings":[],"meta":{"units":"mm","backend":"native","effect":"read","dry_run":false,...}}
{"ok":false,"schema":"altium-cli-extended/v1","command":"pcb net","error":{"code":"NOT_FOUND","category":"input",
 "message":"no net \"CLKK\"","hint":"see `ace pcb nets` for the net names"}}
```

* 목록 명령어는 항상 `data.count`와 `data.items`를 반환합니다.
* 실행 성공 시 종료 코드는 `0`이며, 명시적 `check`/`diff` 게이트 실패 시에는 `ok: true`를 유지한 채 종료 코드 `9`를 반환합니다.
* `ace errors`로 모든 에러 코드, 카테고리, 종료 코드를 확인할 수 있습니다.

---

## 개발

```bash
git clone https://github.com/Hybirdss/altium-cli-extended
cd altium-cli-extended
scripts/fetch-fixtures.sh        # .fixtures/에 샘플 프로젝트 다운로드 (Windows: scripts/fetch-fixtures.ps1)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check-examples.py --ace target/debug/ace
ACE_UPDATE_SKILL=1 cargo test -p altium-cli-extended --test skill   # 명령어 변경 후 skills/ 재생성
```

크레이트 계층 구조와 바이너리 포맷 규칙은 [ARCHITECTURE.md](ARCHITECTURE.md)를 참조하세요.

---

## 라이선스

GPL-3.0-or-later ([LICENSE](LICENSE) 참조). [NOTICE](NOTICE)에는 Altium 레코드 레이아웃 구현에 참고한 KiCad의 Altium 임포터(GPL-3.0-or-later)와 `python-altium` 포맷 노트의 출처가 명시되어 있습니다.

**상표 및 라이선스 고지:** `altium-cli-extended`(`ace`)는 독립적인 오픈소스 프로젝트이며 Altium LLC 또는 Renesas Electronics Corporation과 제휴, 승인 또는 후원 관계가 아닙니다. "Altium" 및 "Altium Designer"는 Altium LLC의 등록 상표입니다. 네이티브 명령어(`backend: native`)는 독자적으로 구현된 파서를 사용해 사용자 설계 파일을 직접 처리하며, `designer` 명령어(`backend: designer`)를 사용하려면 사용자가 별도로 적법한 라이선스를 보유한 Altium Designer가 설치되어 있어야 합니다.
