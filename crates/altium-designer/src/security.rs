//! The desktop runner consumes executable files: its entire tree must be private.
use altium_core::{Error, ErrorCode, Result};
use serde::Deserialize;

pub(crate) const LOCK_ROOT: &str = r#"$ErrorActionPreference = 'Stop'
$root = 'C:\altium-cli-extended'
New-Item -ItemType Directory -Force -Path $root | Out-Null
$sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
& icacls $root /inheritance:r /grant:r "*${sid}:(OI)(CI)F" '*S-1-5-18:(OI)(CI)F' /remove:g '*S-1-5-32-545' '*S-1-5-11' '*S-1-1-0' '*S-1-5-32-544' | Out-Null
if ($LASTEXITCODE) { throw 'cannot protect ace root with icacls' }
foreach ($rule in (Get-Acl -LiteralPath $root).Access) {
  $other = $rule.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value
  if ($other -ne $sid -and $other -ne 'S-1-5-18') {
    & icacls $root /remove "*$other" | Out-Null
    if ($LASTEXITCODE) { throw 'cannot remove foreign ace root ACL' }
  }
}
Get-ChildItem -LiteralPath $root -Force | ForEach-Object {
  & icacls $_.FullName /reset /T /Q | Out-Null
  if ($LASTEXITCODE) { throw 'cannot protect ace descendants with icacls' }
}
"#;

pub(crate) const READ_ACL: &str = r#"$ErrorActionPreference = 'Stop'
$root = 'C:\altium-cli-extended'
$items = @((Get-Item -LiteralPath $root -Force)) + @(Get-ChildItem -LiteralPath $root -Recurse -Force | Where-Object { $_.PSIsContainer -or $_.Extension -in '.cmd','.exe','.pas','.ps1','.PrjScr' })
$rules = @(foreach ($item in $items) {
  if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'ace tree contains a reparse point' }
  foreach ($rule in (Get-Acl -LiteralPath $item.FullName).Access) {
    if ($rule.AccessControlType -eq 'Allow') {
      @{path=$item.FullName; sid=$rule.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value; rights=[long]$rule.FileSystemRights}
    }
  }
})
ConvertTo-Json -InputObject $rules -Compress
"#;

#[derive(Deserialize)]
struct Rule {
    path: String,
    sid: String,
    rights: u64,
}

pub(crate) fn check_acl(json: &str) -> Result<()> {
    let rules: Vec<Rule> = serde_json::from_str(json.trim()).map_err(|e| {
        Error::new(ErrorCode::Unsafe, format!("cannot verify desktop runner ACL: {e}"))
            .hint("run `ace designer setup --apply runner` to protect the runner tree")
    })?;
    // WriteData, AppendData, WriteEA, DeleteChild, WriteAttributes, Delete, WriteDac, WriteOwner.
    let write = 2 | 4 | 16 | 64 | 256 | 65536 | 262144 | 524288;
    for rule in rules {
        if matches!(rule.sid.as_str(), "S-1-5-32-545" | "S-1-5-11" | "S-1-1-0") && rule.rights & write != 0 {
            return Err(Error::new(ErrorCode::Unsafe, format!("desktop runner tree is writable by {}: {}", rule.sid, rule.path))
                .hint("run `ace designer setup --apply runner`; no command was queued"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broad_write_is_refused_but_read_and_private_write_are_allowed() {
        for sid in ["S-1-5-32-545", "S-1-5-11", "S-1-1-0"] {
            for rights in [2, 4, 16, 64, 256, 65536, 262144, 524288, 2032127] {
                let rules = serde_json::json!([{"path":"queue", "sid":sid, "rights":rights}]);
                assert_eq!(check_acl(&rules.to_string()).unwrap_err().code, ErrorCode::Unsafe);
            }
            assert!(check_acl(&serde_json::json!([{"path":"queue", "sid":sid, "rights":131241}]).to_string()).is_ok());
        }
        assert!(check_acl(r#"[{"path":"queue","sid":"S-1-5-18","rights":2032127}]"#).is_ok());
        assert!(check_acl("invalid").is_err());
    }
}
