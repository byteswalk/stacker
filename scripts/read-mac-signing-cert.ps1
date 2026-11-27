# Prints Stacker's macOS signing certificate for build-mac.sh on stdout: two lines, the
# base64 PKCS#12 and its password. Pipe it straight into the build so it never lands on the
# build Mac's disk:
#   powershell -NoProfile -File scripts/read-mac-signing-cert.ps1 | ssh mac 'cd ~/work/stacker && STACKER_SIGN=stdin scripts/build-mac.sh aarch64-apple-darwin'
# See new-mac-signing-cert.ps1 for where it comes from.
$ErrorActionPreference = "Stop"
$target = "StackerRelease/macOS Signing Certificate"
$api = Add-Type -Name Cred -Namespace StackerMacCertRead -PassThru -MemberDefinition @'
[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct CREDENTIAL { public int Flags; public int Type; public string TargetName; public string Comment;
  public long LastWritten; public int CredentialBlobSize; public IntPtr CredentialBlob; public int Persist;
  public int AttributeCount; public IntPtr Attributes; public string TargetAlias; public string UserName; }
[DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
public static extern bool CredReadW(string target, int type, int flags, out IntPtr cred);
[DllImport("advapi32.dll")] public static extern void CredFree(IntPtr cred);
'@ | Where-Object { $_.Name -eq "Cred" }
$pointer = [IntPtr]::Zero
if (-not $api::CredReadW($target, 1, 0, [ref]$pointer)) { throw "$target not found; run new-mac-signing-cert.ps1 first" }
try {
    $credential = [Runtime.InteropServices.Marshal]::PtrToStructure($pointer, [type]"StackerMacCertRead.Cred+CREDENTIAL")
    $blob = New-Object byte[] $credential.CredentialBlobSize
    [Runtime.InteropServices.Marshal]::Copy($credential.CredentialBlob, $blob, 0, $blob.Length)
} finally {
    $api::CredFree($pointer)
}
$stored = [Text.Encoding]::UTF8.GetString($blob) | ConvertFrom-Json
[Array]::Clear($blob, 0, $blob.Length)
[Console]::Out.Write("$($stored.p12)`n$($stored.password)`n")
