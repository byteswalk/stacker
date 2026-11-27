# Creates Stacker's macOS code-signing certificate once, on the Windows machine that keeps
# the release keys, and stores it in Windows Credential Manager (generic credential
# "StackerRelease/macOS Signing Certificate") as JSON
# {"p12": <base64 PKCS#12>, "password": <its password>, "sha1": <cert SHA-1>}.
#
# Why self-signed: without an Apple Developer ID a build is signed ad hoc, and each build
# then counts as a different program to macOS: permissions given to Stacker (Full Disk
# Access and the like) are asked for again after every update. Every version signed with
# this certificate has the same designated requirement (identifier + certificate leaf).
# Gatekeeper still treats it as an unidentified developer, as with ad hoc.
#
# The name does not start with "Stacker:": the key vault removes every credential under
# that prefix it does not hold itself.
#
# Refuses to overwrite an existing certificate: a new one breaks the continuity above for
# every installed copy. Back it up.
$ErrorActionPreference = "Stop"
$target = "StackerRelease/macOS Signing Certificate"
$api = Add-Type -Name Cred -Namespace StackerMacCert -PassThru -MemberDefinition @'
[StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
public struct CREDENTIAL { public int Flags; public int Type; public string TargetName; public string Comment;
  public long LastWritten; public int CredentialBlobSize; public IntPtr CredentialBlob; public int Persist;
  public int AttributeCount; public IntPtr Attributes; public string TargetAlias; public string UserName; }
[DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
public static extern bool CredReadW(string target, int type, int flags, out IntPtr cred);
[DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
public static extern bool CredWriteW(ref CREDENTIAL cred, int flags);
[DllImport("advapi32.dll")] public static extern void CredFree(IntPtr cred);
'@ | Where-Object { $_.Name -eq "Cred" }
$existing = [IntPtr]::Zero
if ($api::CredReadW($target, 1, 0, [ref]$existing)) { $api::CredFree($existing); throw "$target already exists; not replacing it" }

$openssl = Join-Path $env:ProgramFiles "Git\usr\bin\openssl.exe"
if (-not (Test-Path $openssl)) { throw "openssl not found at $openssl (it ships with Git for Windows)" }

$work = Join-Path ([IO.Path]::GetTempPath()) ("stacker-cert-" + [Guid]::NewGuid())
New-Item -ItemType Directory -Path $work | Out-Null
try {
    $config = Join-Path $work "cert.cnf"
    Set-Content -LiteralPath $config -Encoding ascii -Value @"
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = Stacker Code Signing
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
subjectKeyIdentifier = hash
"@
    $key = Join-Path $work "key.pem"
    $cert = Join-Path $work "cert.pem"
    $p12 = Join-Path $work "id.p12"
    $bytes = New-Object byte[] 24
    [Security.Cryptography.RandomNumberGenerator]::Create().GetBytes($bytes)
    $password = [Convert]::ToBase64String($bytes) -replace '[+/=]', ''

    & $openssl ecparam -name prime256v1 -genkey -noout -out $key
    if ($LASTEXITCODE -ne 0) { throw "openssl ecparam failed" }
    # 30 years: the certificate outlives any release that will be signed with it.
    & $openssl req -new -x509 -key $key -out $cert -days 10950 -config $config
    if ($LASTEXITCODE -ne 0) { throw "openssl req failed" }
    # SHA1-3DES protection: what macOS `security import` reads on every version.
    & $openssl pkcs12 -export -inkey $key -in $cert -out $p12 -name "Stacker Code Signing" `
        -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1 -passout "pass:$password"
    if ($LASTEXITCODE -ne 0) { throw "openssl pkcs12 failed" }
    $sha1 = ((& $openssl x509 -in $cert -noout -fingerprint -sha1) -replace '^.*=', '' -replace ':', '').ToUpperInvariant()

    $json = @{ p12 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($p12)); password = $password; sha1 = $sha1 } | ConvertTo-Json -Compress
    $blob = [Text.Encoding]::UTF8.GetBytes($json)
    if ($blob.Length -gt 2560) { throw "certificate too large for Credential Manager ($($blob.Length) bytes)" }
    $pointer = [Runtime.InteropServices.Marshal]::AllocHGlobal($blob.Length)
    try {
        [Runtime.InteropServices.Marshal]::Copy($blob, 0, $pointer, $blob.Length)
        $credential = New-Object StackerMacCert.Cred+CREDENTIAL
        $credential.Type = 1; $credential.TargetName = $target; $credential.Persist = 2
        $credential.CredentialBlobSize = $blob.Length; $credential.CredentialBlob = $pointer
        $credential.UserName = "stacker-releases"
        if (-not $api::CredWriteW([ref]$credential, 0)) { throw "CredWriteW failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
    } finally {
        [Runtime.InteropServices.Marshal]::Copy((New-Object byte[] $blob.Length), 0, $pointer, $blob.Length)
        [Runtime.InteropServices.Marshal]::FreeHGlobal($pointer)
        [Array]::Clear($blob, 0, $blob.Length)
    }
} finally {
    # The key exists only in Credential Manager from here on.
    Remove-Item -LiteralPath $work -Recurse -Force
}
Write-Host "Stored the certificate as $target. Back it up (e.g. in the key vault) - it cannot be recovered."
Write-Host "CERT_SHA1=$sha1"
