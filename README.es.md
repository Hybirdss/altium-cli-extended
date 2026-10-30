<p align="center"><img src="https://raw.githubusercontent.com/Hybirdss/altium-cli-extended/main/docs/assets/banner.png" alt="altium-cli-extended" width="820"></p>

<div align="center">

# altium-cli-extended (`ace`)

**Analizador nativo en Rust, editor seguro a nivel de bytes y puente de automatización de Altium Designer para agentes de IA y CI.**

<a href="https://github.com/Hybirdss/altium-cli-extended/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/Hybirdss/altium-cli-extended/ci.yml?branch=main&label=CI" alt="CI"></a>
<a href="https://github.com/Hybirdss/altium-cli-extended/releases"><img src="https://img.shields.io/github/v/release/Hybirdss/altium-cli-extended?label=release" alt="latest release"></a>
<a href="https://github.com/Hybirdss/altium-cli-extended/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg" alt="GPL-3.0-or-later license"></a>

[English](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.md) | [한국어](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.ko.md) | [简体中文](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.zh-CN.md) | [繁體中文](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.zh-TW.md) | [日本語](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.ja.md) | **Español** | [हिन्दी](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.hi.md)

[Backends](#backends-nativo-y-designer) · [Instalación](#instalación) · [Inicio Rápido](#inicio-rápido) · [Skill para Agentes](#skill-para-agentes) · [Flujos de Trabajo](#flujos-de-trabajo) · [Comandos](#grupos-de-comandos) · [Contrato JSON](#contrato-json) · [Desarrollo](#desarrollo)

</div>

> **¿Trabajas con KiCad 10?** Consulta la herramienta complementaria **[kicad-cli-extended (`kce`)](https://github.com/Hybirdss/kicad-cli-extended)**. Ambos proyectos utilizan la licencia GPL-3.0-or-later.

Altium Designer no incluye una CLI oficial: es una aplicación GUI de Windows que se ejecuta como `X2.EXE`. **`ace`** (Altium CLI Extended) cubre ese vacío mediante una **arquitectura de doble backend**:

1. **Motor nativo en Rust (`backend: native`):** Lee, inspecciona, compara (diff), verifica y realiza ediciones seguras a nivel de bytes en archivos `.PrjPcb`, `.SchDoc`, `.PcbDoc`, `.SchLib`, `.PcbLib` y `.OutJob` directamente en **Windows, macOS y Linux**, **sin necesidad de tener Altium Designer instalado**.
2. **Puente Designer (`backend: designer`):** Controla tu propia instalación con licencia de Altium Designer (en Windows local, Windows remoto por SSH o Wine en Linux) cuando una tarea requiere el motor real de Altium (compilación de proyectos, ERC, DRC, trabajos de salida `.OutJob` para Gerber/ODB++/PDF/STEP, procesos o DelphiScript).

Cada comando imprime un único sobre JSON, cada fallo devuelve un código de error estable y una sugerencia de recuperación, y `ace commands` / `ace describe` describen toda la superficie de comandos para que los agentes de IA y los pipelines de CI trabajen sin adivinar.

---

## Backends nativo y Designer

| Aspecto | Nativo (`backend: native`) | Designer (`backend: designer`) |
|---|---|---|
| **Requiere** | Nada (binario autónomo) | Una instalación de Altium Designer con licencia del usuario (`X2.EXE`) |
| **Se ejecuta en** | Windows, macOS, Linux | Windows local, Windows remoto por SSH o Wine en Linux |
| **Capacidades** | Leer todo, BOM, netlist, comprobaciones, diffs, vistas previas SVG, ediciones seguras de parámetros/reglas/variantes/huellas | Compilar, ERC, DRC, trabajos de salida `.OutJob`, cualquier proceso de Altium y DelphiScript |

### Modos de host de Designer

* **Windows local (predeterminado):** Utiliza Altium Designer instalado en la misma máquina dentro de una sesión de escritorio activa (`conhost --headless` en Windows 10 1809+). Ejecuta `ace designer setup` para revisar los requisitos previos y `ace designer setup --apply runner` como el usuario de escritorio para registrar la tarea programada (con privilegios estándar; no requiere consola de administrador). Si Altium está instalado en una ruta no estándar, apunta `ACE_X2` a `X2.EXE`.
* **Windows remoto por SSH:** Ejecuta `ace` en macOS, Linux u otro PC y controla Altium en una máquina o VM Windows accesible por SSH (con sincronización automática de proyectos). `ace` aplica `StrictHostKeyChecking=yes` con `~/.ssh/known_hosts_altium`: verifica la huella del servidor por un canal de confianza y fija la clave antes de la configuración.
* **Wine en Linux (opcional):** Controla Altium Designer bajo Wine.

> **Escrituras nativas seguras y nota sobre Designer:**
> * Las escrituras nativas (`effect: write`) admiten `--dry-run`, conservan la primera copia de seguridad `<file>.bak` (salvo que se indique `--no-backup`) y vuelven a analizar el resultado antes de reemplazar el archivo. Las ediciones en documentos binarios preservan intactos los registros y flujos no modificados.
> * Limitación conocida de Designer: los trabajos de Designer dejan abiertos en el panel Projects el `.PrjPcb` del trabajo y los proyectos de script puente (`AltiumCliBridge.PrjScr`, `Job.PrjScr`), y una placa donde se ejecutó DRC aparece como modificada. Ciérralos manualmente desde el panel Projects cuando crezca la lista y elige **"Don't Save"** para todo lo que no hayas editado.

---

## Instalación

### Binarios precompilados

Descarga el archivo para tu plataforma desde [GitHub Releases](https://github.com/Hybirdss/altium-cli-extended/releases):

| Sistema Operativo | Archivo | Binario |
|---|---|---|
| **Windows x64** | `altium-cli-extended-x86_64-pc-windows-msvc.zip` | `ace.exe` |
| **macOS Apple Silicon** | `altium-cli-extended-aarch64-apple-darwin.tar.gz` | `ace` |
| **macOS Intel** | `altium-cli-extended-x86_64-apple-darwin.tar.gz` | `ace` |
| **Linux x86_64** | `altium-cli-extended-x86_64-unknown-linux-gnu.tar.gz` | `ace` |
| **Linux arm64** | `altium-cli-extended-aarch64-unknown-linux-gnu.tar.gz` | `ace` |

Cada archivo incluye su correspondiente `<archive>.sha256` (`shasum -a 256 -c` en macOS/Linux, `Get-FileHash` en Windows). Descomprímelo y añade el binario a tu `PATH`:

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

*Nota sobre Gatekeeper en macOS:* Los binarios de macOS no están firmados. Tras verificar la suma SHA-256, si Gatekeeper bloquea `ace`, elimina el atributo de cuarentena con `xattr -d com.apple.quarantine /usr/local/bin/ace`.

### Desde el código fuente (Rust 1.89 o superior)

```bash
cargo install --locked --git https://github.com/Hybirdss/altium-cli-extended altium-cli-extended
```

Comprueba el estado nativo sin contactar con un host mediante `ace doctor --quick`, o ejecuta `ace doctor` para realizar también la prueba completa del host Designer.

---

## Inicio rápido

```bash
ace inspect Board.PrjPcb                          # resumen de cualquier proyecto, hoja, placa o librería
ace bom Board.PrjPcb --out bom.csv                # BOM agrupada; --variant <name> para una variante
ace netlist Board.PrjPcb --where name=GND         # una red y sus pines conectados
ace check Board.PrjPcb                            # anotación, huellas, esquemático frente a PCB
ace diff Board.PrjPcb --rev HEAD~1                # frente al commit anterior en un repositorio Git
ace pcb rules Board.PcbDoc --kind Clearance --units mil
ace sch set-param Power.SchDoc R5 MPN RC0603FR-0710KL --dry-run
ace pcblib list Footprints.PcbLib --where mount=smd --fields name,pads
ace pcblib rename Footprints.PcbLib SOIC-8 SOIC-8N --dry-run
```

* **Filtrado y proyección:** Da forma a cualquier lista con `--fields`, `--where` (`key=value`, `key!=value`, `key~regex`, `key^=substring`) y `--limit`.
* **Unidades y codificación:** Las longitudes están en `mm` salvo que se indique `--units mil|raw` o `ACE_UNITS`. La página de códigos por defecto se ajusta con `--codepage` o `ACE_CODEPAGE`.

---

## Skill para agentes

El repositorio incluye un skill para agentes de IA en [`skills/altium-cli-extended/`](skills/altium-cli-extended/): un [`SKILL.md`](skills/altium-cli-extended/SKILL.md) conciso (reglas y doce recetas con frases de activación, cadena de comandos y qué verificar) y la referencia completa [`COMMANDS.md`](skills/altium-cli-extended/COMMANDS.md).

```bash
ace skill install                          # ~/.agents/skills más todos los agentes detectados (Claude, Codex, Gemini)
ace skill install --target claude          # o codex, gemini, agents, all
ace skill install --dir ./.claude/skills   # en un proyecto local
ace skill status                           # copias instaladas y si están actualizadas
ace skill uninstall --target all
```

---

## Flujos de trabajo

Empieza según lo que quieras hacer. En las cadenas siguientes, `$P` es el proyecto (`.PrjPcb`), `$B` la placa (`.PcbDoc`), `$S` una hoja esquemática (`.SchDoc`) y `$R` un directorio de informes fuera del árbol fuente. Utiliza siempre **rutas absolutas** (para resolver correctamente hojas relativas a carpetas superiores como `..\Utils\*.SchDoc`).

| Quieres | Primer comando | Sección |
|---|---|---|
| Revisar un proyecto entregado por un contratista, sin Altium | `ace project find <dir>` | [Revisar la entrega de un contratista](#revisar-la-entrega-de-un-contratista) |
| Ver qué cambió entre dos revisiones | `ace diff A.PrjPcb B.PrjPcb --summary` | [Comparar revisiones](#comparar-revisiones) |
| Detener un build o lanzamiento en CI por problemas de diseño | `ace check P.PrjPcb --fail-on error` | [Validar un diseño en CI](#validar-un-diseño-en-ci) |
| Cambiar un valor, añadir una variante o ajustar una regla | `ace sch set-param ... --dry-run` | [Aplicar un ECO sin abrir Altium](#aplicar-un-eco-sin-abrir-altium) |
| Renombrar o copiar una huella sin referencias rotas | `ace pcblib rename ... --dry-run` | [Renombrar una huella sin referencias obsoletas](#renombrar-una-huella-sin-referencias-obsoletas) |
| Responder "qué hay en este pin o red" | `ace sch locate P.PrjPcb U1` | [Buscar componentes, redes y huellas de librería](#buscar-componentes-redes-y-huellas-de-librería) |
| Generar Gerbers, compilar y ejecutar ERC/DRC | `ace designer ping --timeout 60` | [Preparar un lanzamiento con Designer](#preparar-un-lanzamiento-con-designer) |
| Deshacer una edición o desbloquear Altium | `ace restore F --from first --dry-run` | [Recuperar una edición o desbloquear Designer](#recuperar-una-edición-o-desbloquear-designer) |
| Migrar un diseño hacia KiCad | `ace check P.PrjPcb --fail-on error` | [De Altium a KiCad](#de-altium-a-kicad) |

### Revisar la entrega de un contratista

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

### Comparar revisiones

```bash
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --summary
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --by part --ignore-field '^param:ComponentLink'
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --where change=part_added --fields ref
ace diff /absolute/RevA/Board.PcbDoc /absolute/RevB/Board.PcbDoc --geometry
ace diff "$P" --rev HEAD~1 --source pcb
```

### Validar un diseño en CI

```bash
ace check "$P" --list
ace check "$P" --pcb "$B" --require MPN,Manufacturer --fail-on error > "$R/check.json"
ace diff "$P" --rev HEAD~1 --exit-code
ace check "$P" --pcb "$B" --require MPN,Manufacturer --baseline "$R/check.json" --fail-on error
```

### Aplicar un ECO sin abrir Altium

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

### Renombrar una huella sin referencias obsoletas

```bash
ace sch find "$S" SOIC-8
ace pcb components "$B" --where footprint=SOIC-8 --fields designator,footprint
ace pcblib copy /absolute/Target.PcbLib /absolute/Source.PcbLib SOIC-8 --name SOIC-8N --dry-run
ace pcblib rename /absolute/Target.PcbLib SOIC-8 SOIC-8N --dry-run
ace sch set-footprint "$S" U3 SOIC-8N --dry-run
ace diff /absolute/before/Target.PcbLib /absolute/Target.PcbLib
```

### Buscar componentes, redes y huellas de librería

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

### Preparar un lanzamiento con Designer

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

En PowerShell, utiliza un array de argumentos para rutas con espacios y comprueba `$LASTEXITCODE` explícitamente:

```powershell
$AceArgs = @('--json', '--read-only')
$Project = 'C:\Designs\My Board\Board.PrjPcb'
& ace.exe @AceArgs check $Project --fail-on error
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
```

### Recuperar una edición o desbloquear Designer

```bash
ace restore "$S" --from first --dry-run
ace restore "$S" --from first
ace designer status
ace designer dialogs
ace designer screenshot --out /tmp/designer.png
ace designer jobs --last 3
```

### De Altium a KiCad

Congela una copia del proyecto origen y registra un manifiesto recursivo de archivos/hashes fuera de ambos proyectos. Importa hacia una ruta de destino explícita:

```bash
kce project new /tmp/target --name target
kce --json --project /tmp/target pcb import /absolute/source/Board.PcbDoc --format altium --output /tmp/target/target.kicad_pcb --report-format json --report-file /tmp/import.json
kce --json --project /tmp/target pcb info
```

Vuelve a leer la placa de destino exacta: exige que la ruta devuelta sea `/tmp/target/target.kicad_pcb`, inspecciona las huellas importadas y compara su hash con la placa vacía. Recalcula el manifiesto de origen incluyendo altas y bajas de archivos para confirmar que es idéntico. Una importación exitosa de la placa no equivale a una migración completa verificada: la conversión de símbolos/librerías y la equivalencia eléctrica independiente requieren evidencia adicional.

---

## Grupos de comandos

| Grupo | Propósito |
|---|---|
| `project` | Descubrimiento de proyectos, documentos, parámetros, variantes (y su edición), ajustes |
| `sch` | Hojas esquemáticas: componentes, pines, redes, etiquetas, puertos, jerarquía, edición de parámetros/comentarios/huellas/designadores |
| `schlib` | Librerías de símbolos esquemáticos: símbolos, pines, edición de parámetros y descripciones |
| `pcb` | Placas PCB: capas, stackup, componentes, redes, reglas, clases, primitivas, taladros, longitudes ruteadas, SVG; edición de reglas/clases/parámetros |
| `pcblib` | Librerías de huellas: huellas, pads, modelos 3D, SVG; `rename`, `set-param`, `remove`, `copy` seguros a nivel de bytes |
| `outjob` | Archivos Output Job: salidas, medios, activar/desactivar |
| `bom`, `netlist`, `check`, `diff` | Análisis entre documentos: BOM, netlist, comprobaciones de diseño y diffs entre revisiones/Git |
| `designer` | Puente al Altium Designer real: configuración, estado, diálogos, captura de pantalla, compilación, ERC, DRC, Output Jobs, procesos, scripts |
| `raw` | Inspección de bajo nivel de flujos y registros en cualquier archivo compuesto (OLE/CFB) |
| `commands`, `describe`, `skill`, `errors`, `doctor`, `inspect`, `restore` | Descubrimiento, esquemas/formas de retorno, gestión del skill, diagnóstico y restauración de copias de seguridad |

---

## Contrato JSON

Cuando se usa en una tubería (pipe) o con `--json`, `stdout` emite exactamente un objeto JSON:

```json
{"ok":true,"schema":"altium-cli-extended/v1","command":"pcb nets","data":{"count":2,"items":[...]},
 "warnings":[],"meta":{"units":"mm","backend":"native","effect":"read","dry_run":false,...}}
{"ok":false,"schema":"altium-cli-extended/v1","command":"pcb net","error":{"code":"NOT_FOUND","category":"input",
 "message":"no net \"CLKK\"","hint":"see `ace pcb nets` for the net names"}}
```

---

## Desarrollo

```bash
git clone https://github.com/Hybirdss/altium-cli-extended
cd altium-cli-extended
scripts/fetch-fixtures.sh        # proyectos de ejemplo en .fixtures/ (Windows: scripts/fetch-fixtures.ps1)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check-examples.py --ace target/debug/ace
ACE_UPDATE_SKILL=1 cargo test -p altium-cli-extended --test skill   # regenerar skills/ tras cambios en comandos
```

Consulta [ARCHITECTURE.md](ARCHITECTURE.md) para ver la arquitectura de crates y las convenciones del formato binario.

---

## Licencia

GPL-3.0-or-later, consulta [LICENSE](LICENSE). [NOTICE](NOTICE) acredita las fuentes de referencia para los formatos de registro de Altium: los importadores de Altium de KiCad (GPL-3.0-or-later) y las notas de formato de `python-altium`.

**Aviso sobre Marcas Registradas y Licencias:** `altium-cli-extended` (`ace`) es un proyecto independiente de código abierto y no está afiliado, respaldado ni patrocinado por Altium LLC ni por Renesas Electronics Corporation. "Altium" y "Altium Designer" son marcas registradas de Altium LLC. Los comandos nativos (`backend: native`) operan directamente sobre los archivos de diseño del usuario mediante un analizador independiente; los comandos `designer` (`backend: designer`) requieren una instalación independiente y con licencia válida de Altium Designer proporcionada por el usuario.
