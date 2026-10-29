//! Altium's open dialogs, seen from the desktop session (Win32 window enumeration through a
//! PowerShell helper run by [`Host::ps_interactive`]).
//!
//! A modal dialog (license prompt, script error, "file modified outside", ECO confirmation...)
//! disables Altium's main window and stalls every bridge request. [`list`] reports it with its
//! text and buttons so an agent (or the bridge's stall detection) can say exactly what is in the
//! way; [`close`] presses one of its buttons.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use altium_core::{Error, ErrorCode, Result};

use crate::host::Host;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Window {
    pub title: String,
    pub class: String,
    /// Static texts inside the window (the message of a message box).
    #[serde(default)]
    pub texts: Vec<String>,
    /// Button captions (`&` accelerators removed).
    #[serde(default)]
    pub buttons: Vec<String>,
    /// Enabled windows owned by Altium are the ones a modal dialog leaves usable.
    #[serde(default)]
    pub enabled: bool,
    /// Shown by a second X2.EXE that could not hand its command to the running Altium (e.g.
    /// "another instance is busy"): the running Altium is stuck in a script or a dialog.
    #[serde(default)]
    pub launcher: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Report {
    /// Altium is running.
    pub running: bool,
    /// Altium's main window title (shows the license edition and the focused document).
    #[serde(default)]
    pub main_title: String,
    /// The main window is disabled: a modal dialog is waiting for an answer.
    pub blocked: bool,
    /// Visible windows of Altium other than the main window (dialogs, floating panels).
    #[serde(default)]
    pub windows: Vec<Window>,
    /// Button pressed by [`close`], if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressed: Option<String>,
}

impl Report {
    /// The windows that are modal dialogs (enabled while the main window is disabled).
    pub fn modal(&self) -> Vec<&Window> {
        if !self.blocked {
            return vec![];
        }
        self.windows.iter().filter(|w| w.enabled).collect()
    }

    /// A launcher message box only: Altium itself is busy (a script halted in the debugger).
    pub fn launcher_only(&self) -> bool {
        let m = self.modal();
        !m.is_empty() && m.iter().all(|w| w.launcher)
    }

    /// One line per modal dialog: `title: text [buttons]`.
    pub fn summary(&self) -> String {
        self.modal()
            .iter()
            .map(|w| format!("{}: {} [{}]", w.title, w.texts.join(" "), w.buttons.join("|")))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// `$press`: empty = only list; otherwise a `|`-separated preference list of button captions
/// (`*` = WM_CLOSE when no listed button exists).
const DIALOGS_PS1: &str = r#"
Add-Type -TypeDefinition @"
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public class AcW {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h, EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  public static string Text(IntPtr h) { var b = new StringBuilder(1024); GetWindowText(h, b, 1024); return b.ToString(); }
  public static string Cls(IntPtr h) { var b = new StringBuilder(256); GetClassName(h, b, 256); return b.ToString(); }
  public static List<IntPtr> Top(uint pid) {
    var r = new List<IntPtr>();
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); if (p == pid && IsWindowVisible(h)) r.Add(h); return true; }, IntPtr.Zero);
    return r;
  }
  public static List<IntPtr> Kids(IntPtr w) {
    var r = new List<IntPtr>();
    EnumChildWindows(w, (h, l) => { if (IsWindowVisible(h)) r.Add(h); return true; }, IntPtr.Zero);
    return r;
  }
}
"@
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$res = [ordered]@{ running = $false; main_title = ''; blocked = $false; windows = @(); pressed = $null }
$procs = @(Get-Process X2 -ErrorAction SilentlyContinue)
# Several X2.EXE can exist: the Altium instance and short-lived launchers that hand a command
# line (-R...) to it. A launcher that could not deliver shows its own message box.
$main = $null; $mainPid = 0
foreach ($p in $procs) {
  foreach ($h in [AcW]::Top([uint32]$p.Id)) {
    if (-not $main -and [AcW]::Text($h) -match 'Altium Designer') { $main = $h; $mainPid = $p.Id }
  }
}
if ($procs.Count -gt 0) {
  $res.running = $true
  if ($main) { $res.main_title = [AcW]::Text($main); $res.blocked = -not [AcW]::IsWindowEnabled($main) }
  $wins = @()
  foreach ($p in $procs) {
   $launcher = $p.Id -ne $mainPid
   foreach ($h in [AcW]::Top([uint32]$p.Id)) {
    if ($h -eq $main) { continue }
    $title = [AcW]::Text($h); $cls = [AcW]::Cls($h)
    if ($cls -eq 'TApplication' -or ($title -eq '' -and $cls -match 'Tooltip|Shadow|Hint')) { continue }
    $texts = @(); $buttons = @(); $bh = @{}
    foreach ($k in [AcW]::Kids($h)) {
      $kc = [AcW]::Cls($k); $kt = [AcW]::Text($k)
      if ($kt -eq '') { continue }
      if ($kc -match 'Button') { $c = $kt.Replace('&', ''); $buttons += $c; if (-not $bh.ContainsKey($c)) { $bh[$c] = $k } }
      elseif ($kc -match 'Static|Label|Memo|Edit|RichEdit|Text') { $texts += $kt }
    }
    $enabled = [AcW]::IsWindowEnabled($h)
    $modal = $enabled -and ($res.blocked -or $launcher)
    if ($launcher -and $enabled) { $res.blocked = $true }
    $uia = @{}
    if ($modal) {
      # Themed dialogs draw their controls without child windows: read them through UI Automation.
      try {
        $ae = [System.Windows.Automation.AutomationElement]::FromHandle($h)
        $all = $ae.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
        foreach ($e in $all) {
          $n = $e.Current.Name; if (-not $n) { continue }
          $ct = $e.Current.ControlType.ProgrammaticName
          if ($ct -eq 'ControlType.Button') { $c = $n.Replace('&', ''); if (-not $bh.ContainsKey($c) -and -not $uia.ContainsKey($c)) { $buttons += $c; $uia[$c] = $e } }
          elseif ($ct -match 'Text|Edit|Document|Pane' -and $texts -notcontains $n -and $n -ne $title -and $n -notmatch '^[A-Za-z0-9]+\.[A-Za-z0-9.]+$') { $texts += $n }
        }
      } catch {}
    }
    $w = [ordered]@{ title = $title; class = $cls; texts = $texts; buttons = $buttons; enabled = $enabled; launcher = $launcher }
    $wins += ,$w
    if ($press -ne '' -and $modal -and -not $res.pressed) {
      foreach ($want in $press.Split('|')) {
        if ($want -eq '*') { [void][AcW]::PostMessage($h, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero); $res.pressed = "close:$title"; break }
        $hit = $bh.Keys | Where-Object { $_ -ieq $want } | Select-Object -First 1
        if ($hit) { [void][AcW]::PostMessage($bh[$hit], 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero); $res.pressed = "$hit"; break }
        $hit = $uia.Keys | Where-Object { $_ -ieq $want } | Select-Object -First 1
        if ($hit) {
          try { $uia[$hit].GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke(); $res.pressed = "$hit"; break } catch {}
        }
      }
    }
   }
  }
  $res.windows = $wins
}
[IO.File]::WriteAllText($out, ($res | ConvertTo-Json -Depth 5 -Compress), (New-Object Text.UTF8Encoding $false))
"#;

fn run(host: &dyn Host, press: &str) -> Result<Report> {
    let script = format!("$press = '{}'\r\n{DIALOGS_PS1}", press.replace('\'', "''"));
    let raw = host.ps_interactive(&script, Duration::from_secs(60))?;
    parse(&raw)
}

pub fn parse(raw: &[u8]) -> Result<Report> {
    let text = crate::bridge::decode_text(raw);
    let mut v: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|e| Error::new(ErrorCode::DesignerFailed, format!("dialog helper wrote invalid JSON: {e}")))?;
    // ConvertTo-Json writes a one-element array as an object and an empty one as null.
    if let Some(w) = v.get_mut("windows") {
        if w.is_object() {
            *w = serde_json::Value::Array(vec![w.take()]);
        } else if w.is_null() {
            *w = serde_json::Value::Array(vec![]);
        }
        for win in w.as_array_mut().into_iter().flatten() {
            for k in ["texts", "buttons"] {
                if let Some(x) = win.get_mut(k) {
                    if x.is_string() {
                        *x = serde_json::Value::Array(vec![x.take()]);
                    } else if x.is_null() {
                        *x = serde_json::Value::Array(vec![]);
                    }
                }
            }
        }
    }
    serde_json::from_value(v).map_err(|e| Error::new(ErrorCode::DesignerFailed, format!("dialog helper: {e}")))
}

/// What Altium shows right now.
pub fn list(host: &dyn Host) -> Result<Report> {
    run(host, "")
}

/// Press a button of the modal dialog: the first of `buttons` that exists (case-insensitive);
/// `*` closes the window instead. Nothing happens when no modal dialog is open.
pub fn close(host: &dyn Host, buttons: &[&str]) -> Result<Report> {
    run(host, &buttons.join("|"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_powershell_shapes() {
        let r = parse(br#"{"running":true,"main_title":"Home - Altium Designer Professional","blocked":true,"windows":{"title":"Error","class":"TMessageForm","texts":"Undeclared identifier: X","buttons":"OK","enabled":true},"pressed":null}"#).unwrap();
        assert!(r.blocked);
        assert_eq!(r.windows.len(), 1);
        assert_eq!(r.windows[0].buttons, vec!["OK"]);
        assert_eq!(r.summary(), "Error: Undeclared identifier: X [OK]");
        let r = parse(br#"{"running":false,"main_title":"","blocked":false,"windows":null,"pressed":null}"#).unwrap();
        assert!(r.windows.is_empty() && r.modal().is_empty());
    }
}
