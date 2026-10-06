$ErrorActionPreference = 'Stop'
$root = Join-Path ([IO.Path]::GetTempPath()) ('bloom-hook-test-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($root) | Out-Null
$script = Join-Path $PSScriptRoot 'codex-hook.ps1'
$utf8 = New-Object Text.UTF8Encoding($false)
$OutputEncoding = $utf8

function Assert($condition, $message) { if (-not $condition) { throw $message } }
function Send-Event($name, $tool = '', $command = '', $turn = 'turn-1', $id = 'test-chat') {
    $data = @{ session_id = $id; hook_event_name = $name; cwd = 'C:\repo\bloom'; turn_id = $turn; tool_name = $tool; tool_input = @{ command = $command; description = 'Check requested access' }; prompt = 'PRIVATE PROMPT'; tool_response = 'PRIVATE RESPONSE' }
    if ($name -ne 'PermissionRequest') { $data.tool_input.Remove('description') }
    $output = $data | ConvertTo-Json -Depth 10 -Compress | & powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $script -StateRoot $root
    Assert ($LASTEXITCODE -eq 0) 'Hook must exit successfully'
    Assert (-not $output) 'Hook must not return permission decisions'
    Get-Content -LiteralPath (Join-Path $root "$id.json") -Raw | ConvertFrom-Json
}

try {
    Assert ((Send-Event 'UserPromptSubmit').status -eq 'running') 'A new prompt starts work'
    Assert ((Send-Event 'PermissionRequest' 'Bash' 'PRIVATE COMMAND').status -eq 'awaiting_approval') 'Approval is reported'
    Assert ((Send-Event 'PreToolUse' 'other-tool' 'other-command').status -eq 'awaiting_approval') 'Parallel tools must not erase pending approvals'
    Assert ((Send-Event 'PostToolUse' 'other-tool' 'other-command').status -eq 'awaiting_approval') 'Unrelated result must not erase pending approvals'
    Assert ((Send-Event 'PostToolUse' 'Bash' 'PRIVATE COMMAND').status -eq 'running') 'Matching result clears approval'
    Send-Event 'PermissionRequest' 'Bash' 'same-command' | Out-Null
    Send-Event 'PermissionRequest' 'Bash' 'same-command' | Out-Null
    Assert ((Send-Event 'PostToolUse' 'Bash' 'same-command').status -eq 'awaiting_approval') 'One result must not clear two matching approval requests'
    Assert ((Send-Event 'PostToolUse' 'Bash' 'same-command').status -eq 'running') 'Second result clears the remaining request'
    $state = Get-Content -LiteralPath (Join-Path $root 'test-chat.json') -Raw
    Assert ($state -notmatch 'PRIVATE') 'No prompt, command or response should be persisted'
    Assert ((Send-Event 'Stop').status -eq 'completed') 'Stop finishes a turn'
    Assert ((Send-Event 'UserPromptSubmit' '' '' 'turn-2').status -eq 'running') 'New turn resumes work'
    Assert ((Send-Event 'Stop' '' '' 'turn-1').status -eq 'running') 'Late stop must not end the new turn'
    Assert ((Send-Event 'Interrupt' '' '' 'turn-2').status -eq 'interrupted') 'Interrupt is distinct from completion'
    Assert ((Send-Event 'SessionEnd' '' '' 'turn-2').status -eq 'closed') 'Session end clears the session'
    Assert ((Send-Event 'UserPromptSubmit' '' '' 'other-turn' 'other-chat').session_id -eq 'other-chat') 'Chats are independent'

    $codex = Join-Path $root 'config'
    [IO.Directory]::CreateDirectory($codex) | Out-Null
    $file = Join-Path $codex 'hooks.json'
    & powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $script -Install -CodexRoot $codex
    Assert ($LASTEXITCODE -eq 0) 'Fresh install must succeed'
    Assert ((Get-Content -LiteralPath $file -Raw | ConvertFrom-Json).hooks.PermissionRequest.Count -eq 1) 'Fresh install includes approvals'
    $original = '{"custom":{"keep":true},"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"existing-hook","timeout":5}]}],"Stop":[{"hooks":[{"type":"command","command":"other-hook"}]}]}}'
    [IO.File]::WriteAllText($file, $original, $utf8)
    & powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $script -Install -CodexRoot $codex
    Assert ($LASTEXITCODE -eq 0) 'Install must succeed'
    & powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $script -Install -CodexRoot $codex
    Assert ($LASTEXITCODE -eq 0) 'Repeated installation must succeed'
    $config = Get-Content -LiteralPath $file -Raw | ConvertFrom-Json
    Assert ($config.custom.keep -eq $true) 'Unknown configuration fields are preserved'
    Assert ($config.hooks.PreToolUse.Count -eq 2) 'Install is idempotent and keeps existing groups'
    Assert ($config.hooks.PreToolUse[0].matcher -eq 'Bash') 'Existing matchers are preserved'
    Assert ($config.hooks.PermissionRequest.Count -eq 1) 'Permission reporting is installed'
    & powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $script -Uninstall -CodexRoot $codex
    Assert ($LASTEXITCODE -eq 0) 'Uninstall must succeed'
    $config = Get-Content -LiteralPath $file -Raw | ConvertFrom-Json
    Assert ($config.hooks.PreToolUse.Count -eq 1) 'Uninstall removes only our hook'
    Assert ($config.hooks.PreToolUse[0].hooks[0].command -eq 'existing-hook') 'Existing hooks survive uninstall'
    Assert ($config.hooks.Stop[0].hooks[0].command -eq 'other-hook') 'Other lifecycle hooks survive uninstall'
    [IO.File]::WriteAllText($file, '{broken', $utf8)
    $ErrorActionPreference = 'Continue'
    & powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $script -Install -CodexRoot $codex 2>$null
    $ErrorActionPreference = 'Stop'
    Assert ($LASTEXITCODE -ne 0) 'Invalid config must fail without replacing it'
    Assert ([IO.File]::ReadAllText($file) -eq '{broken') 'Invalid configuration is preserved'
    Write-Output 'Codex hook lifecycle, approval, privacy and configuration tests passed.'
} finally {
    $resolved = [IO.Path]::GetFullPath($root)
    $tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if ($resolved.StartsWith($tempRoot, [StringComparison]::OrdinalIgnoreCase) -and (Split-Path $resolved -Leaf) -like 'bloom-hook-test-*') {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
