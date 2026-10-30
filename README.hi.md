<p align="center"><img src="https://raw.githubusercontent.com/Hybirdss/altium-cli-extended/main/docs/assets/banner.png" alt="altium-cli-extended" width="820"></p>

<div align="center">

# altium-cli-extended (`ace`)

**AI एजेंटों और CI के लिए नेटिव Rust पार्सर, बाइट-सेफ़ एडिटर और Altium Designer ऑटोमेशन ब्रिज।**

<a href="https://github.com/Hybirdss/altium-cli-extended/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/Hybirdss/altium-cli-extended/ci.yml?branch=main&label=CI" alt="CI"></a>
<a href="https://github.com/Hybirdss/altium-cli-extended/releases"><img src="https://img.shields.io/github/v/release/Hybirdss/altium-cli-extended?label=release" alt="latest release"></a>
<a href="https://github.com/Hybirdss/altium-cli-extended/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg" alt="GPL-3.0-or-later license"></a>

[English](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.md) | [한국어](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.ko.md) | [简体中文](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.zh-CN.md) | [繁體中文](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.zh-TW.md) | [日本語](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.ja.md) | [Español](https://github.com/Hybirdss/altium-cli-extended/blob/main/README.es.md) | **हिन्दी**

[बैकएंड आर्किटेक्चर](#नेटिव-और-designer-बैकएंड) · [इंस्टॉलेशन](#इंस्टॉलेशन) · [क्विकस्टार्ट](#क्विकस्टार्ट) · [एजेंट स्किल](#एजेंट-स्किल) · [वर्कफ़्लो](#वर्कफ़्लो) · [कमांड समूह](#कमांड-समूह) · [JSON अनुबंध](#json-अनुबंध) · [विकास (Development)](#विकास-development)

</div>

> **क्या आप KiCad 10 पर काम कर रहे हैं?** सहयोगी टूल **[kicad-cli-extended (`kce`)](https://github.com/Hybirdss/kicad-cli-extended)** देखें। दोनों प्रोजेक्ट GPL-3.0-or-later लाइसेंस का उपयोग करते हैं।

Altium Designer कोई आधिकारिक CLI प्रदान नहीं करता — यह एक Windows GUI अनुप्रयोग है जो `X2.EXE` के रूप में चलता है। **`ace`** (Altium CLI Extended) एक **दोहरे बैकएंड आर्किटेक्चर (dual-backend architecture)** के साथ इस कमी को पूरा करता है:

1. **नेटिव Rust इंजन (`backend: native`):** **बिना किसी Altium Designer इंस्टॉलेशन के**, सीधे **Windows, macOS और Linux** पर `.PrjPcb`, `.SchDoc`, `.PcbDoc`, `.SchLib`, `.PcbLib` और `.OutJob` फ़ाइलों को पढ़ता है, जाँचता है, रिवीजन तुलना (diff) करता है और बाइट-सेफ़ (byte-safe) एडिटिंग करता है।
2. **Designer ब्रिज (`backend: designer`):** जब किसी कार्य के लिए Altium के अपने इंजन की आवश्यकता होती है (जैसे प्रोजेक्ट कम्पाइल, ERC, DRC, Gerber/ODB++/PDF/STEP `.OutJob` एक्सपोर्ट, कोई भी Altium प्रोसेस या DelphiScript), तो यह उपयोगकर्ता के अपने वैध लाइसेंस वाले Altium Designer (लोकल Windows, SSH पर रिमोट Windows/VM, या Linux पर Wine) को संचालित करता है।

प्रत्येक कमांड एक JSON एनवलप (envelope) प्रिंट करता है, प्रत्येक विफलता में एक स्थिर त्रुटि कोड और समाधान संकेत (`hint`) होता है, और `ace commands` व `ace describe` पूरे कमांड सरफ़ेस का वर्णन करते हैं ताकि AI एजेंट और CI पाइपलाइन बिना किसी अनुमान के काम कर सकें।

---

## नेटिव और Designer बैकएंड

| पहलू | नेटिव (`backend: native`) | Designer (`backend: designer`) |
|---|---|---|
| **आवश्यकता** | कुछ नहीं (स्वतंत्र सिंगल बाइनरी) | उपयोगकर्ता-लाइसेंस प्राप्त Altium Designer (`X2.EXE`) |
| **किस पर चलता है** | Windows, macOS, Linux | लोकल Windows, SSH पर रिमोट Windows, या Linux पर Wine |
| **क्षमताएँ** | सभी फ़ाइलें पढ़ना, BOM, नेटलिस्ट, डिज़ाइन जाँच, रिवीजन डिफ, SVG प्रीव्यू, बाइट-सेफ़ पैरामीटर/रूल/वेरिएंट/फुटप्रिंट एडिट | प्रोजेक्ट कम्पाइल, ERC, DRC, `.OutJob` आउटपुट, कोई भी Altium प्रोसेस और DelphiScript |

### Designer होस्ट मोड

* **लोकल Windows (डिफ़ॉल्ट):** उसी मशीन पर लॉग-इन डेस्कटॉप सेशन में इंस्टॉल किए गए Altium Designer का उपयोग करता है (Windows 10 1809+ पर `conhost --headless`)। पूर्व-आवश्यकताओं की जाँच के लिए `ace designer setup` चलाएँ और डेस्कटॉप उपयोगकर्ता के रूप में `ace designer setup --apply runner` से शेड्यूल्ड टास्क रजिस्टर करें (एडमिन अधिकारों की आवश्यकता नहीं)। यदि Altium गैर-डिफ़ॉल्ट पथ पर है, तो `ACE_X2` को `X2.EXE` पर सेट करें।
* **SSH के माध्यम से रिमोट Windows:** macOS, Linux या किसी अन्य PC पर `ace` चलाएँ और SSH के माध्यम से Windows मशीन या VM पर Altium को नियंत्रित करें (प्रोजेक्ट फ़ाइलें स्वचालित रूप से सिंक होती हैं)। `ace` `~/.ssh/known_hosts_altium` के साथ `StrictHostKeyChecking=yes` लागू करता है — सेटअप से पहले विश्वसनीय माध्यम से सर्वर फ़िंगरप्रिंट सत्यापित करें और कुंजी पिन करें।
* **Linux पर Wine (वैकल्पिक):** Wine के अंतर्गत Altium Designer को चलाता है।

> **बाइट-सेफ़ नेटिव राइट्स और Designer नोट:**
> * नेटिव राइट्स (`effect: write`) `--dry-run` का समर्थन करते हैं, पहली `<file>.bak` फ़ाइल सुरक्षित रखते हैं (जब तक `--no-backup` न दिया जाए), और फ़ाइल बदलने से पहले परिणाम को पुनः पार्स करके सत्यापित करते हैं।
> * ज्ञात सीमा: Designer जॉब्स Altium के Projects पैनल में `.PrjPcb` और ब्रिज स्क्रिप्ट प्रोजेक्ट्स (`AltiumCliBridge.PrjScr`, `Job.PrjScr`) को खुला छोड़ देते हैं। जब सूची बड़ी हो जाए तो उन्हें Projects पैनल से बंद करें और जिन फ़ाइलों को आपने एडिट नहीं किया है उनके लिए **"Don't Save"** चुनें।

---

## इंस्टॉलेशन

### प्रीबिल्ट बाइनरी (Prebuilt binaries)

[GitHub Releases](https://github.com/Hybirdss/altium-cli-extended/releases) से अपने ऑपरेटिंग सिस्टम के लिए आर्काइव डाउनलोड करें:

| ऑपरेटिंग सिस्टम | आर्काइव | बाइनरी |
|---|---|---|
| **Windows x64** | `altium-cli-extended-x86_64-pc-windows-msvc.zip` | `ace.exe` |
| **macOS Apple Silicon** | `altium-cli-extended-aarch64-apple-darwin.tar.gz` | `ace` |
| **macOS Intel** | `altium-cli-extended-x86_64-apple-darwin.tar.gz` | `ace` |
| **Linux x86_64** | `altium-cli-extended-x86_64-unknown-linux-gnu.tar.gz` | `ace` |
| **Linux arm64** | `altium-cli-extended-aarch64-unknown-linux-gnu.tar.gz` | `ace` |

प्रत्येक आर्काइव के साथ एक `<archive>.sha256` फ़ाइल होती है। इसे अनपैक करें और बाइनरी को अपने `PATH` में रखें:

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

*macOS Gatekeeper नोट:* macOS बाइनरी अनसाइंड हैं। चेकसम सत्यापित करने के बाद यदि Gatekeeper इसे रोकता है, तो `xattr -d com.apple.quarantine /usr/local/bin/ace` चलाएँ।

### सोर्स से बिल्ड करें (Rust 1.89 या नया)

```bash
cargo install --locked --git https://github.com/Hybirdss/altium-cli-extended altium-cli-extended
```

बिना होस्ट से संपर्क किए नेटिव तत्परता जाँचने के लिए `ace doctor --quick` चलाएँ, या पूर्ण Designer जाँच के लिए `ace doctor` चलाएँ।

---

## क्विकस्टार्ट

```bash
ace inspect Board.PrjPcb                          # प्रोजेक्ट, शीट, बोर्ड या लाइब्रेरी का सारांश
ace bom Board.PrjPcb --out bom.csv                # समूहीकृत BOM; वेरिएंट के लिए --variant <name>
ace netlist Board.PrjPcb --where name=GND         # एक नेट और उससे जुड़े पिन्स
ace check Board.PrjPcb                            # एनोटेशन, फुटप्रिंट्स, स्कीमैटिक बनाम PCB जाँच
ace diff Board.PrjPcb --rev HEAD~1                # पिछले Git कमिट से तुलना
ace pcb rules Board.PcbDoc --kind Clearance --units mil
ace sch set-param Power.SchDoc R5 MPN RC0603FR-0710KL --dry-run
ace pcblib list Footprints.PcbLib --where mount=smd --fields name,pads
ace pcblib rename Footprints.PcbLib SOIC-8 SOIC-8N --dry-run
```

* **फ़िल्टरिंग:** किसी भी सूची को `--fields`, `--where` (`key=value`, `key!=value`, `key~regex`, `key^=substring`), और `--limit` से फ़िल्टर करें।
* **इकाइयाँ और एन्कोडिंग:** लंबाई डिफ़ॉल्ट रूप से `mm` में होती है (`--units mil|raw` या `ACE_UNITS` से बदलें); कोडपेज `--codepage` या `ACE_CODEPAGE` से सेट करें।

---

## एजेंट स्किल

रिपॉजिटरी में [`skills/altium-cli-extended/`](skills/altium-cli-extended/) के अंतर्गत जनरेटेड AI एजेंट स्किल शामिल है: एक संक्षिप्त [`SKILL.md`](skills/altium-cli-extended/SKILL.md) (नियम और 12 ट्रिगर-आधारित रेसिपी) और पूरा [`COMMANDS.md`](skills/altium-cli-extended/COMMANDS.md) संदर्भ।

```bash
ace skill install                          # ~/.agents/skills तथा पाए गए सभी एजेंट (Claude, Codex, Gemini)
ace skill install --target claude          # या codex, gemini, agents, all
ace skill install --dir ./.claude/skills   # किसी प्रोजेक्ट में लोकल इंस्टॉल
ace skill status                           # इंस्टॉल की गई कॉपियों की स्थिति
ace skill uninstall --target all
```

---

## वर्कफ़्लो

आप जो करना चाहते हैं वहाँ से शुरू करें। नीचे दिए गए उदाहरणों में `$P` प्रोजेक्ट (`.PrjPcb`), `$B` बोर्ड (`.PcbDoc`), `$S` स्कीमैटिक शीट (`.SchDoc`), और `$R` सोर्स के बाहर की रिपोर्ट डायरेक्टरी है। हमेशा **पूर्ण पथ (absolute paths)** का उपयोग करें।

| आप क्या करना चाहते हैं | पहला कमांड | अनुभाग |
|---|---|---|
| बिना Altium के कॉन्ट्रैक्टर द्वारा सौंपे गए प्रोजेक्ट की समीक्षा | `ace project find <dir>` | [कॉन्ट्रैक्टर हैंडओवर की समीक्षा](#कॉन्ट्रैक्टर-हैंडओवर-की-समीक्षा) |
| दो रिवीजन के बीच बदलाव देखना | `ace diff A.PrjPcb B.PrjPcb --summary` | [रिवीजन की तुलना करना](#रिवीजन-की-तुलना-करना) |
| CI में डिज़ाइन त्रुटियों पर बिल्ड या रिलीज़ रोकना | `ace check P.PrjPcb --fail-on error` | [CI में डिज़ाइन गेट लगाना](#ci-में-डिज़ाइन-गेट-लगाना) |
| बिना Altium खोले वैल्यू बदलना, वेरिएंट जोड़ना या नियम बदलना | `ace sch set-param ... --dry-run` | [बिना Altium खोले ECO लागू करना](#बिना-altium-खोले-eco-लागू-करना) |
| बिना टूटे रेफरेंस के फुटप्रिंट का नाम बदलना | `ace pcblib rename ... --dry-run` | [फुटप्रिंट का सुरक्षित नाम बदलना](#फुटप्रिंट-का-सुरक्षित-नाम-बदलना) |
| किसी पिन, नेट, स्टैकअप या लाइब्रेरी फुटप्रिंट को खोजना | `ace sch locate P.PrjPcb U1` | [पुर्ज़े, नेट्स और लाइब्रेरी फुटप्रिंट खोजना](#पुर्ज़े-नेट्स-और-लाइब्रेरी-फुटप्रिंट-खोजना) |
| Gerber बनाना, कम्पाइल, ERC, DRC चलाना | `ace designer ping --timeout 60` | [Designer के साथ रिलीज़ तैयार करना](#designer-के-साथ-रिलीज़-तैयार-करना) |
| गलत एडिट को पूर्ववत करना या अटके हुए Altium को ठीक करना | `ace restore F --from first --dry-run` | [एडिट रीस्टोर करना या Designer को अनस्टिक करना](#एडिट-रीस्टोर-करना-या-designer-को-अनस्टिक-करना) |
| Altium डिज़ाइन को KiCad में ले जाना | `ace check P.PrjPcb --fail-on error` | [Altium से KiCad में माइग्रेशन](#altium-से-kicad-में-माइग्रेशन) |

### कॉन्ट्रैक्टर हैंडओवर की समीक्षा

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

### रिवीजन की तुलना करना

```bash
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --summary
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --by part --ignore-field '^param:ComponentLink'
ace diff /absolute/RevA/Board.PrjPcb /absolute/RevB/Board.PrjPcb --where change=part_added --fields ref
ace diff /absolute/RevA/Board.PcbDoc /absolute/RevB/Board.PcbDoc --geometry
ace diff "$P" --rev HEAD~1 --source pcb
```

### CI में डिज़ाइन गेट लगाना

```bash
ace check "$P" --list
ace check "$P" --pcb "$B" --require MPN,Manufacturer --fail-on error > "$R/check.json"
ace diff "$P" --rev HEAD~1 --exit-code
ace check "$P" --pcb "$B" --require MPN,Manufacturer --baseline "$R/check.json" --fail-on error
```

### बिना Altium खोले ECO लागू करना

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

### फुटप्रिंट का सुरक्षित नाम बदलना

```bash
ace sch find "$S" SOIC-8
ace pcb components "$B" --where footprint=SOIC-8 --fields designator,footprint
ace pcblib copy /absolute/Target.PcbLib /absolute/Source.PcbLib SOIC-8 --name SOIC-8N --dry-run
ace pcblib rename /absolute/Target.PcbLib SOIC-8 SOIC-8N --dry-run
ace sch set-footprint "$S" U3 SOIC-8N --dry-run
ace diff /absolute/before/Target.PcbLib /absolute/Target.PcbLib
```

### पुर्ज़े, नेट्स और लाइब्रेरी फुटप्रिंट खोजना

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

### Designer के साथ रिलीज़ तैयार करना

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

PowerShell पर स्पेस वाले पथों के लिए आर्ग्यूमेंट ऐरे का उपयोग करें और `$LASTEXITCODE` की स्पष्ट जाँच करें:

```powershell
$AceArgs = @('--json', '--read-only')
$Project = 'C:\Designs\My Board\Board.PrjPcb'
& ace.exe @AceArgs check $Project --fail-on error
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
```

### एडिट रीस्टोर करना या Designer को अनस्टिक करना

```bash
ace restore "$S" --from first --dry-run
ace restore "$S" --from first
ace designer status
ace designer dialogs
ace designer screenshot --out /tmp/designer.png
ace designer jobs --last 3
```

### Altium से KiCad में माइग्रेशन

स्रोत की एक प्रति फ़्रीज़ करें और दोनों प्रोजेक्ट्स के बाहर एक रिकर्सिव फ़ाइल/हैश मैनिफ़ेस्ट रिकॉर्ड करें। स्पष्ट टार्गेट पथ में इम्पोर्ट करें:

```bash
kce project new /tmp/target --name target
kce --json --project /tmp/target pcb import /absolute/source/Board.PcbDoc --format altium --output /tmp/target/target.kicad_pcb --report-format json --report-file /tmp/import.json
kce --json --project /tmp/target pcb info
```

सटीक टार्गेट बोर्ड को रीड-बैक करें: सुनिश्चित करें कि रिपोर्ट किया गया पथ `/tmp/target/target.kicad_pcb` है, इम्पोर्ट किए गए फुटप्रिंट्स की जाँच करें और इसके हैश की तुलना खाली बोर्ड से करें। फ़ाइल जोड़ने और हटाने सहित स्रोत मैनिफ़ेस्ट की पुनः गणना करें ताकि यह सुनिश्चित हो कि मूल फ़ाइलें अपरिवर्तित हैं। केवल बोर्ड इम्पोर्ट सफल होना एक सत्यापित पूर्ण माइग्रेशन नहीं है (सिंबल/लाइब्रेरी रूपांतरण और स्वतंत्र इलेक्ट्रिकल समानता के लिए अलग प्रमाण आवश्यक हैं)।

---

## कमांड समूह

| समूह | उद्देश्य |
|---|---|
| `project` | प्रोजेक्ट खोज, दस्तावेज़, पैरामीटर, वेरिएंट्स (और वेरिएंट एडिट), सेटिंग्स |
| `sch` | स्कीमैटिक शीट्स: कंपोनेंट्स, पिन्स, नेट्स, लेबल्स, पोर्ट्स, पदानुक्रम (hierarchy), पैरामीटर/कमेंट/फुटप्रिंट/डेज़िग्नेटर एडिट |
| `schlib` | स्कीमैटिक सिंबल लाइब्रेरी: सिंबल, पिन, पैरामीटर और विवरण एडिट |
| `pcb` | PCB बोर्ड: लेयर्स, स्टैकअप, कंपोनेंट्स, नेट्स, नियम, क्लासेस, प्रिमिटिव्स, होल्स, राउटेड लंबाई, SVG; नियम/क्लास/पैरामीटर एडिट |
| `pcblib` | फुटप्रिंट लाइब्रेरी: फुटप्रिंट्स, पैड्स, 3D मॉडल, SVG; बाइट-सेफ़ `rename`, `set-param`, `remove`, `copy` |
| `outjob` | Output Job फ़ाइलें: आउटपुट, मीडियम, सक्षम/अक्षम करना |
| `bom`, `netlist`, `check`, `diff` | क्रॉस-डॉक्यूमेंट विश्लेषण: BOM, नेटलिस्ट, डिज़ाइन जाँच और रिवीजन/Git डिफ |
| `designer` | वास्तविक Altium Designer ब्रिज: सेटअप, स्टेटस, डायलॉग्स, स्क्रीनशॉट, कम्पाइल, ERC, DRC, Output Jobs, प्रोसेस, स्क्रिप्ट्स |
| `raw` | किसी भी कंपाउंड फ़ाइल (OLE/CFB) के स्ट्रीम या रिकॉर्ड का निरीक्षण |
| `commands`, `describe`, `skill`, `errors`, `doctor`, `inspect`, `restore` | खोज, स्कीमा/आकार वर्णन, एजेंट स्किल प्रबंधन, निदान और बैकअप रीस्टोर |

---

## JSON अनुबंध

पाइप किए जाने पर या `--json` के साथ, `stdout` पर ठीक एक JSON ऑब्जेक्ट आउटपुट होता है:

```json
{"ok":true,"schema":"altium-cli-extended/v1","command":"pcb nets","data":{"count":2,"items":[...]},
 "warnings":[],"meta":{"units":"mm","backend":"native","effect":"read","dry_run":false,...}}
{"ok":false,"schema":"altium-cli-extended/v1","command":"pcb net","error":{"code":"NOT_FOUND","category":"input",
 "message":"no net \"CLKK\"","hint":"see `ace pcb nets` for the net names"}}
```

---

## विकास (Development)

```bash
git clone https://github.com/Hybirdss/altium-cli-extended
cd altium-cli-extended
scripts/fetch-fixtures.sh        # .fixtures/ में सैंपल प्रोजेक्ट्स डाउनलोड करें (Windows: scripts/fetch-fixtures.ps1)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check-examples.py --ace target/debug/ace
ACE_UPDATE_SKILL=1 cargo test -p altium-cli-extended --test skill   # कमांड बदलने के बाद skills/ को रीजनरेट करें
```

क्रेट लेयरिंग और बाइनरी फ़ॉर्मेट नियमों के लिए [ARCHITECTURE.md](ARCHITECTURE.md) देखें।

---

## लाइसेंस

GPL-3.0-or-later, [LICENSE](LICENSE) देखें। [NOTICE](NOTICE) उन स्रोतों को श्रेय देता है जिनके आधार पर Altium रिकॉर्ड लेआउट लिखे गए: KiCad के Altium इम्पोर्टर्स (GPL-3.0-or-later) और `python-altium` के फ़ॉर्मेट नोट्स।

**ट्रेडमार्क और लाइसेंस अस्वीकरण:** `altium-cli-extended` (`ace`) एक स्वतंत्र ओपन-सोर्स प्रोजेक्ट है और Altium LLC या Renesas Electronics Corporation से संबद्ध, समर्थित या प्रायोजित नहीं है। "Altium" और "Altium Designer" Altium LLC के पंजीकृत ट्रेडमार्क हैं। नेटिव कमांड (`backend: native`) एक स्वतंत्र पार्सर का उपयोग करके सीधे उपयोगकर्ता की डिज़ाइन फ़ाइलों पर काम करते हैं; `designer` कमांड (`backend: designer`) के लिए उपयोगकर्ता द्वारा प्रदत्त Altium Designer के एक अलग, वैध रूप से लाइसेंस प्राप्त इंस्टॉलेशन की आवश्यकता होती है।
