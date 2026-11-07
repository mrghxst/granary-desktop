param([int]$Vk, [switch]$Shift)
# Bring the Granary window to the front and press one key through the real keyboard input path (layout applies).
Add-Type -Namespace K -Name I -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetForegroundWindow(System.IntPtr h);
[DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, System.UIntPtr extra);
[DllImport("user32.dll")] public static extern uint MapVirtualKeyW(uint code, uint type);
[DllImport("user32.dll")] public static extern System.IntPtr GetForegroundWindow();
'@
$p = Get-Process granary -ErrorAction Stop | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if ([K.I]::GetForegroundWindow() -ne $p.MainWindowHandle) {
  # A tapped Alt lets a background process take the foreground (foreground lock). Never while Granary is in front:
  # there it would open keyboard menu mode and swallow the next key.
  [K.I]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [K.I]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)
  $null = [K.I]::SetForegroundWindow($p.MainWindowHandle)
}
Start-Sleep -Milliseconds 300
$fg = [K.I]::GetForegroundWindow() -eq $p.MainWindowHandle
function Tap([byte]$v, [uint32]$f) { [K.I]::keybd_event($v, [byte][K.I]::MapVirtualKeyW($v, 0), $f, [UIntPtr]::Zero) }
if ($Shift) { Tap 0x10 0 }
Tap ([byte]$Vk) 0
Tap ([byte]$Vk) 2
if ($Shift) { Tap 0x10 2 }
"sent vk=0x{0:x2} shift={1} foreground={2}" -f $Vk, [bool]$Shift, $fg
