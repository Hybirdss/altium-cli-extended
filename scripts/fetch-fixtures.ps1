# Download real Altium projects used as test fixtures into .fixtures/samples (gitignored).
# Windows counterpart of fetch-fixtures.sh. Tests that need them skip when the directory is absent.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$samples = Join-Path $root ".fixtures/samples"
New-Item -ItemType Directory -Force -Path $samples | Out-Null
foreach ($r in @("ryansturmer/BeagleBoneCape", "SalimTerryLi/AirPi_PCB", "SalimTerryLi/PilotPi_PCB", "mrKuzmich/quasar_arm_pcb")) {
    $dir = Join-Path $samples (Split-Path -Leaf $r)
    if (-not (Test-Path $dir)) {
        git clone -q --depth 1 "https://github.com/$r.git" $dir
    }
}
Write-Output "fixtures in $samples"
