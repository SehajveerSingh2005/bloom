param([ValidateSet('claude', 'codex')][string]$Agent)
# Bloom-owned hook. Always advisory: no approval decisions and no agent output.
$ErrorActionPreference = 'Stop'
try {
    [System.Net.ServicePointManager]::Expect100Continue = $false
    $eventData = [Console]::In.ReadToEnd() | ConvertFrom-Json
    $activity = $null
    $kind = switch ($eventData.hook_event_name) {
        'Stop' { 'ready' }
        'PermissionRequest' {
            # This fires before the approval decision, including automatic review.
            # Keep the label neutral: this hook does not identify who reviews the request
            # or signal when the command starts executing.
            if ($Agent -eq 'codex') { $activity = 'working'; 'working' } else { 'attention' }
        }
        'Notification' {
            if ($eventData.notification_type -in @('permission_prompt', 'elicitation_dialog')) { 'attention' }
        }
        'StopFailure' { 'error' }
        'UserPromptSubmit' { $activity = 'thinking'; 'working' }
        'PreToolUse' {
            $activity = switch -Regex ([string]$eventData.tool_name) {
                '^(Edit|Write|apply_patch|edit|write)$' { 'editing'; break }
                '^(Read|Glob|Grep|read_file|list_dir|search|read)$' { 'reading'; break }
                '^(Bash|PowerShell|exec_command|bash)$' { 'running'; break }
                default { 'thinking' }
            }
            'working'
        }
        'PostToolUse' { $activity = 'thinking'; 'working' }
        'PostToolUseFailure' {
            if ($eventData.is_interrupt) { 'stopped' } else { $activity = 'thinking'; 'working' }
        }
        'Interrupt' { 'stopped' }
        'SessionEnd' { 'stopped' }
    }
    if ($kind) {
        $bridge = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'bridge.json') -Raw | ConvertFrom-Json
        $project = Split-Path -Leaf ($eventData.cwd -replace '[\\/]+$', '')
        $session = $eventData.session_id
        if (-not $session) { $session = $eventData.thread_id }
        if ($session) {
            $payload = @{ agent = $Agent; session = [string]$session; project = [string]$project; kind = $kind }
            if ($activity) { $payload.activity = $activity }
            $body = $payload | ConvertTo-Json -Compress
            $bytes = [Text.Encoding]::UTF8.GetBytes($body)
            $request = [Net.HttpWebRequest]::Create("http://127.0.0.1:$($bridge.port)/agent-alert")
            $request.Proxy = $null
            $request.Method = 'POST'
            $request.ContentType = 'application/json'
            $request.ContentLength = $bytes.Length
            $request.Headers['Authorization'] = "Bearer $($bridge.token)"
            $request.Timeout = 2000
            $request.ReadWriteTimeout = 2000
            $request.KeepAlive = $false
            $stream = $request.GetRequestStream()
            try { $stream.Write($bytes, 0, $bytes.Length) } finally { $stream.Dispose() }
            $response = $request.GetResponse()
            $response.Dispose()
        }
    }
} catch {
    # Bloom closed or unavailable: the agent should continue normally.
}
# Codex Stop requires valid JSON on stdout; {} makes no lifecycle decisions.
[Console]::Out.Write('{}')
exit 0
