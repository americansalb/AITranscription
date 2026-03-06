param(
    [string]$ProjectDir = $PSScriptRoot,
    [string[]]$Roles = @("manager", "architect", "developer", "developer", "tester", "tester")
)

foreach ($role in $Roles) {
    $scriptContent = @"
Set-Location "$ProjectDir"
claude --dangerously-skip-permissions "Join this project as a $role using the mcp vaak project_join tool with role $role. Then call project_wait in a loop to stay available for messages."
"@
    $tempScript = [System.IO.Path]::Combine($env:TEMP, "vaak-launch-$role-$(Get-Random).ps1")
    $scriptContent | Out-File -FilePath $tempScript -Encoding UTF8

    Start-Process powershell -WorkingDirectory $ProjectDir -ArgumentList '-NoExit', '-ExecutionPolicy', 'Bypass', '-File', $tempScript

    Start-Sleep -Seconds 2
}

Write-Host "Launched $($Roles.Count) team members: $($Roles -join ', ')"
