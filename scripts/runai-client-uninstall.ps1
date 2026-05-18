# runai client uninstall — Windows / PowerShell.
#
# Usage:
#   irm http://<SERVER>:<PORT>/uninstall.ps1 | iex
#
# Reverses runai-client-install.ps1: removes the UserPromptSubmit hook
# entry pointing at ~/.runai-hook.ps1 from settings.json, then deletes
# the hook script. Safe to run if you never installed — both steps are
# no-ops in that case. Backs up the prior settings.json to
# .runai-uninstall-bak.

$HookPath = "$env:USERPROFILE\.runai-hook.ps1"
$SettingsPath = "$env:USERPROFILE\.claude\settings.json"

Write-Host "runai client uninstall (Windows)"
Write-Host ""

function ConvertTo-RunaiHashtable($obj) {
    if ($null -eq $obj) { return $null }
    if ($obj -is [PSCustomObject]) {
        $h = @{}
        foreach ($p in $obj.PSObject.Properties) {
            $h[$p.Name] = ConvertTo-RunaiHashtable $p.Value
        }
        return $h
    }
    if ($obj -is [System.Collections.IEnumerable] -and -not ($obj -is [string])) {
        $arr = @()
        foreach ($item in $obj) { $arr += ,(ConvertTo-RunaiHashtable $item) }
        return ,$arr
    }
    return $obj
}

# 1) Strip the hook entry from settings.json. Idempotent.
if (Test-Path $SettingsPath) {
    Copy-Item $SettingsPath "$SettingsPath.runai-uninstall-bak" -Force
    $raw = Get-Content $SettingsPath -Raw
    if ([string]::IsNullOrWhiteSpace($raw)) { $raw = "{}" }
    try {
        $parsed = $raw | ConvertFrom-Json
    } catch {
        Write-Warning "settings.json was not valid JSON, leaving untouched"
        $parsed = $null
    }
    if ($null -ne $parsed) {
        $data = ConvertTo-RunaiHashtable $parsed
        if ($null -ne $data -and $data.ContainsKey('hooks') -and $data.hooks.ContainsKey('UserPromptSubmit')) {
            $removed = 0
            $newUps = @()
            foreach ($g in $data.hooks.UserPromptSubmit) {
                if ($null -eq $g -or -not $g.ContainsKey('hooks')) { $newUps += ,$g; continue }
                $kept = @()
                foreach ($h in @($g.hooks)) {
                    if ($null -ne $h -and $h.command -like "*\.runai-hook.ps1*") {
                        $removed += 1
                    } else {
                        $kept += ,$h
                    }
                }
                if ($kept.Count -gt 0) {
                    $g.hooks = $kept
                    $newUps += ,$g
                } else {
                    # whole group was ours — drop the wrapper too
                    $removed += 1
                }
            }
            if ($newUps.Count -gt 0) {
                $data.hooks.UserPromptSubmit = $newUps
            } else {
                $data.hooks.Remove('UserPromptSubmit')
            }
            $word = if ($removed -eq 1) { 'entry' } else { 'entries' }
            Write-Host "removed $removed runai hook $word from settings.json"
            $data | ConvertTo-Json -Depth 20 | Set-Content -Path $SettingsPath -Encoding utf8
        } else {
            Write-Host "no runai UserPromptSubmit hook present"
        }
    }
} else {
    Write-Host "no settings.json — nothing to clean"
}

# 2) Delete the hook wrapper itself.
if (Test-Path $HookPath) {
    Remove-Item $HookPath -Force
    Write-Host "removed $HookPath"
} else {
    Write-Host "no $HookPath — already clean"
}

Write-Host ""
Write-Host "done. Claude Code will no longer call runai on UserPromptSubmit."
Write-Host "original settings.json backed up to: $SettingsPath.runai-uninstall-bak"
