#![cfg(windows)]
//! Q8 (X-http-transport): WinHTTP certificate validation is NOT disabled. A local TLS server (a `powershell.exe`
//! child started and killed by the test, in-memory self-signed certificate `CN=localhost`, no certificate store is
//! touched, nothing is trusted) must make `WinHttpTransport::get` fail with a secure-connection error and never hand
//! out a response. The positive half (a locally TRUSTED certificate: HTTPS to HTTP downgrade, TLS happy path) needs a
//! root added to the Windows trust store, a security setting this test never changes: it stays an attended rig.
//! C#: `HttpClient` default `SslOptions` validates the chain (no `ServerCertificateCustomValidationCallback`).
use pr_core::sync::CancelToken;
use pr_core::updates::checker::{ACCEPT, HttpError, HttpRequest, HttpRequestError, HttpTransport, USER_AGENT};
use pr_platform_win::http::WinHttpTransport;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const SERVER_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Security
$rsa = [System.Security.Cryptography.RSA]::Create(2048)
$req = New-Object System.Security.Cryptography.X509Certificates.CertificateRequest('CN=localhost', $rsa, [System.Security.Cryptography.HashAlgorithmName]::SHA256, [System.Security.Cryptography.RSASignaturePadding]::Pkcs1)
$cert = $req.CreateSelfSigned([DateTimeOffset]::UtcNow.AddDays(-1), [DateTimeOffset]::UtcNow.AddDays(1))
$pfx = $cert.Export([System.Security.Cryptography.X509Certificates.X509ContentType]::Pfx, 'x')
$server = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($pfx, 'x', [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::Exportable)
$listener = New-Object System.Net.Sockets.TcpListener([System.Net.IPAddress]::Loopback, 0)
$listener.Start()
[Console]::Out.WriteLine('READY ' + $listener.LocalEndpoint.Port)
[Console]::Out.Flush()
while ($true) {
  $client = $listener.AcceptTcpClient()
  try {
    $ssl = New-Object System.Net.Security.SslStream($client.GetStream(), $false)
    $ssl.AuthenticateAsServer($server, $false, [System.Security.Authentication.SslProtocols]::Tls12, $false)
  } catch { }
  $client.Close()
}
"#;

struct Tls {
    child: Child,
    port: u16,
}
impl Tls {
    fn start(dir: &std::path::Path) -> Self {
        let script = dir.join("tls-server.ps1");
        std::fs::write(&script, SERVER_SCRIPT).expect("write script");
        let mut child = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&script)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("start powershell");
        let mut line = String::new();
        BufReader::new(child.stdout.take().expect("stdout"))
            .read_line(&mut line)
            .expect("read READY");
        let port = line
            .trim()
            .strip_prefix("READY ")
            .and_then(|p| p.parse().ok())
            .unwrap_or_else(|| {
                let _ = child.kill();
                panic!("TLS rig did not start: {line:?}");
            });
        Self { child, port }
    }
}
impl Drop for Tls {
    fn drop(&mut self) {
        // Only the process this test started.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
#[ignore = "native: starts a local TLS server through powershell.exe (self-signed, untrusted certificate)"]
fn untrusted_self_signed_certificate_is_rejected_with_a_secure_connection_error() {
    let root = pr_test_support::TempRoot::new("q8-tls");
    let tls = Tls::start(root.path());
    let request = HttpRequest {
        url: format!("https://localhost:{}/update", tls.port),
        user_agent: USER_AGENT.into(),
        accept: ACCEPT.into(),
    };
    let result = WinHttpTransport.get(&request, Duration::from_secs(20), &CancelToken::new());
    assert!(
        matches!(result, Err(HttpError::Request(HttpRequestError::SecureConnectionError))),
        "an untrusted certificate must be a SecureConnectionError, got {:?}",
        result.map(|r| r.status)
    );
}
