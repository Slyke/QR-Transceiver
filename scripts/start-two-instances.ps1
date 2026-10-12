param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [Parameter(Mandatory = $true)][string]$SenderState,
    [Parameter(Mandatory = $true)][string]$ReceiverState,
    [ValidateRange(1, 65535)][int]$SenderMcpPort = 47631,
    [ValidateRange(1, 65535)][int]$ReceiverMcpPort = 47632,
    [switch]$EnableMcp
)
$ErrorActionPreference = 'Stop'
$exePath = (Resolve-Path -LiteralPath $Executable).Path
$senderPath = [IO.Path]::GetFullPath($SenderState)
$receiverPath = [IO.Path]::GetFullPath($ReceiverState)
if ($senderPath -eq $receiverPath -or $SenderMcpPort -eq $ReceiverMcpPort) {
    throw 'Use different state directories and MCP ports for the two instances.'
}
$mcpArgs = if ($EnableMcp) { @('--enable-mcp') } else { @() }
$sender = Start-Process -FilePath $exePath -ArgumentList (@('--state', ('"' + $senderPath + '"'), '--mcp-port', $SenderMcpPort) + $mcpArgs) -WindowStyle Normal -PassThru
$receiver = Start-Process -FilePath $exePath -ArgumentList (@('--state', ('"' + $receiverPath + '"'), '--mcp-port', $ReceiverMcpPort) + $mcpArgs) -WindowStyle Normal -PassThru
Write-Output "Sender PID: $($sender.Id); MCP port: $SenderMcpPort"
Write-Output "Receiver PID: $($receiver.Id); MCP port: $ReceiverMcpPort"
Write-Output 'Each window uses the profile saved in its state directory. Choose Send/Receive when creating fresh state.'

Write-Output 'MCP requires -EnableMcp or a saved enabled setting; a port alone leaves it off.'
