param(
    [switch]$Install,
    [switch]$Uninstall,
    [string]$StateRoot = (Join-Path $env:APPDATA 'bloom\codex'),
    [string]$CodexRoot = $(if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' })
)

# Informational only: never emit a permission decision or change tool input.
$ErrorActionPreference = 'Stop'
[Console]::InputEncoding = New-Object System.Text.UTF8Encoding($false)
$events = @('SessionStart', 'SessionEnd', 'UserPromptSubmit', 'PreToolUse', 'PermissionRequest', 'PostToolUse', 'Stop', 'Interrupt')
$utf8 = New-Object System.Text.UTF8Encoding($false)

function Write-AtomicJson($file, $value) {
    $temp = "$file.$([guid]::NewGuid().ToString('N')).tmp"
    $backup = "$temp.bak"
    try {
        [IO.File]::WriteAllText($temp, ($value | ConvertTo-Json -Depth 100 -Compress), $utf8)
        if ([IO.File]::Exists($file)) {
            # Windows PowerShell converts a null string argument to an empty path.
            [IO.File]::Replace($temp, $file, $backup)
        } else {
            [IO.File]::Move($temp, $file)
        }
    } finally {
        if ([IO.File]::Exists($temp)) { [IO.File]::Delete($temp) }
        if ([IO.File]::Exists($backup)) { [IO.File]::Delete($backup) }
    }
}

function Clip-Text($value, $limit) {
    $text = ([string]$value) -replace '[\x00-\x1f]', ' '
    if ($text.Length -gt $limit) { return $text.Substring(0, $limit) }
    return $text
}

if ($Install -or $Uninstall) {
    # Merge only our command hooks; preserve all existing hooks and config fields.
    [IO.Directory]::CreateDirectory($CodexRoot) | Out-Null
    $file = Join-Path $CodexRoot 'hooks.json'
    $config = if (Test-Path -LiteralPath $file) {
        Get-Content -LiteralPath $file -Raw | ConvertFrom-Json
    } else { [pscustomobject]@{} }
    if ($null -eq $config -or $config -is [array] -or $config -is [string]) { throw 'Invalid Codex hooks configuration.' }
    if (-not $config.PSObject.Properties['hooks']) { $config | Add-Member -NotePropertyName hooks -NotePropertyValue ([pscustomobject]@{}) }
    if ($null -eq $config.hooks -or $config.hooks -is [array] -or $config.hooks -is [string]) { throw 'Invalid Codex hooks configuration.' }
    $script = $PSCommandPath
    if ($script.Contains('"')) { throw 'Unsupported hook script path.' }
    $command = 'powershell.exe -NoLogo -NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File "' + $script + '"'
    foreach ($name in $events) {
        $kept = @()
        if ($config.hooks.PSObject.Properties[$name]) {
            foreach ($group in @($config.hooks.$name)) {
                $otherHooks = @($group.hooks | Where-Object { $_.command -ne $command })
                if ($otherHooks.Count -gt 0) {
                    $group.hooks = $otherHooks
                    $kept += $group
                }
            }
        }
        if ($Install) { $kept += [pscustomobject]@{ hooks = @([pscustomobject]@{ type = 'command'; command = $command; timeout = 3 }) } }
        if ($config.hooks.PSObject.Properties[$name]) {
            $config.hooks.$name = $kept
        } elseif ($Install) {
            $config.hooks | Add-Member -NotePropertyName $name -NotePropertyValue $kept
        }
    }
    if (Test-Path -LiteralPath $file) {
        Copy-Item -LiteralPath $file -Destination "$file.bloom-backup-$([guid]::NewGuid().ToString('N'))"
    }
    Write-AtomicJson $file $config
    exit 0
}

# A bounded stdin read keeps a broken producer from exhausting memory.
try {
    $buffer = New-Object char[] 4096
    $inputText = New-Object Text.StringBuilder
    while (($count = [Console]::In.Read($buffer, 0, $buffer.Length)) -gt 0) {
        if ($inputText.Length + $count -gt 4194304) { exit 0 }
        [void]$inputText.Append($buffer, 0, $count)
    }
    $event = $inputText.ToString() | ConvertFrom-Json
    $id = [string]$event.session_id
    $name = [string]$event.hook_event_name
    if ($id -notmatch '^[a-zA-Z0-9_-]{1,128}$' -or $name -notin $events) { exit 0 }
    [IO.Directory]::CreateDirectory($StateRoot) | Out-Null
    $mutex = New-Object Threading.Mutex($false, "Local\BloomCodex-$id")
    $acquired = $false
    try {
        try { $acquired = $mutex.WaitOne(1000) } catch [Threading.AbandonedMutexException] { $acquired = $true }
        if (-not $acquired) { exit 0 }
        $file = Join-Path $StateRoot "$id.json"
        $previous = if (Test-Path -LiteralPath $file) { Get-Content -LiteralPath $file -Raw | ConvertFrom-Json } else { $null }
        $pending = @($previous.pending_keys | Where-Object { $_ })
        $turn = [string]$event.turn_id
        if ($turn -and $previous.turn_id -and $turn -ne $previous.turn_id) {
            # Ignore late results from a previous turn after a new prompt arrives.
            if ($name -notin @('UserPromptSubmit', 'SessionStart') -and $previous.status -in @('running', 'awaiting_approval')) { exit 0 }
            $pending = @()
        }
        $status = if ($previous.status) { $previous.status } else { 'idle' }
        $detail = ''
        $tool = Clip-Text $event.tool_name 120
        # Hash arguments solely to pair permission requests with their results.
        # Neither arguments nor prompt/response text are written to disk.
        $hash = [Security.Cryptography.SHA256]::Create()
        try {
            # Bash/file-edit approval payloads can add a description not present
            # on the result. Match their command, not that extra description.
            $identity = if ($tool -in @('Bash', 'apply_patch')) { [string]$event.tool_input.command } else { $event.tool_input | ConvertTo-Json -Depth 100 -Compress }
            $key = [BitConverter]::ToString($hash.ComputeHash($utf8.GetBytes($tool + $identity)))
        } finally { $hash.Dispose() }
        switch ($name) {
            'SessionStart' { $status = 'idle'; $pending = @(); $detail = 'Ready'; $tool = '' }
            'SessionEnd' { $status = 'closed'; $pending = @(); $detail = 'Session ended'; $tool = '' }
            'UserPromptSubmit' { $status = 'running'; $pending = @(); $detail = 'Working'; $tool = '' }
            'PreToolUse' { $status = 'running'; $detail = if ($tool) { "Using $tool" } else { 'Working' } }
            'PermissionRequest' {
                $pending = @($pending + $key)
                $status = 'awaiting_approval'
                $detail = Clip-Text $event.tool_input.description 240
                if (-not $detail) { $detail = 'Approval requested in Codex' }
            }
            'PostToolUse' {
                $remaining = @(); $matched = $false
                foreach ($pendingKey in $pending) {
                    if (-not $matched -and $pendingKey -eq $key) { $matched = $true } else { $remaining += $pendingKey }
                }
                $pending = $remaining; $status = 'running'; $detail = 'Working'
            }
            'Stop' { $status = 'completed'; $pending = @(); $detail = 'Turn finished'; $tool = '' }
            'Interrupt' { $status = 'interrupted'; $pending = @(); $detail = 'Interrupted'; $tool = '' }
        }
        if ($pending.Count -gt 0) {
            $status = 'awaiting_approval'
            if ($name -ne 'PermissionRequest') { $detail = 'Approval requested in Codex' }
        }
        Write-AtomicJson $file ([ordered]@{
            schema = 1; session_id = $id; cwd = (Clip-Text $event.cwd 1024)
            status = $status; detail = $detail; tool_name = $tool
            turn_id = $(if ($turn) { $turn } else { [string]$previous.turn_id })
            pending_keys = $pending; updated_at = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        })
    } finally {
        if ($acquired) { $mutex.ReleaseMutex() }
        $mutex.Dispose()
    }
} catch {
    # Status reporting must never block or approve a Codex action.
    exit 0
}
