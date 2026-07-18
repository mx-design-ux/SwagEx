use crate::{certificate::ensure_certificate, export::write_profile, protocol::decode_profile};
use flate2::read::{GzDecoder, ZlibDecoder};
use http_body_util::BodyExt;
use hudsucker::{
    Body, HttpContext, HttpHandler, Proxy, RequestOrResponse,
    certificate_authority::RcgenAuthority,
    hyper::{Method, Request, Response, StatusCode, header},
    rustls::crypto::aws_lc_rs,
};
use serde::Serialize;
use std::{
    io::Read,
    net::{IpAddr, Ipv4Addr, UdpSocket},
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Manager, State};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

const PREFERRED_PROXY_PORT: u16 = 8080;
const PROFILE_PATH: &str = "/api/gateway_c2.php";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusSnapshot {
    pub phase: String,
    pub message: String,
    pub local_ip: Option<String>,
    pub port: Option<u16>,
    pub certificate_url: Option<String>,
    pub certificate_was_created: bool,
    pub export_path: Option<String>,
    pub profile_name: Option<String>,
}

impl StatusSnapshot {
    fn idle() -> Self {
        Self {
            phase: "idle".into(),
            message: "Prêt à exporter votre compte.".into(),
            local_ip: None,
            port: None,
            certificate_url: None,
            certificate_was_created: false,
            export_path: None,
            profile_name: None,
        }
    }
}

struct SharedState {
    status: Mutex<StatusSnapshot>,
}

impl SharedState {
    fn snapshot(&self) -> StatusSnapshot {
        self.status.lock().expect("status mutex poisoned").clone()
    }

    fn replace(&self, status: StatusSnapshot) {
        *self.status.lock().expect("status mutex poisoned") = status;
    }

    fn record_export(&self, path: PathBuf, display_name: String) {
        let mut status = self.status.lock().expect("status mutex poisoned");
        status.phase = "captured".into();
        status.message = "Le JSON a été créé.".into();
        status.export_path = Some(path.to_string_lossy().into_owned());
        status.profile_name = Some(display_name);
    }

    fn record_proxy_error(&self, error: impl std::fmt::Display) {
        let mut status = self.status.lock().expect("status mutex poisoned");
        if status.phase != "captured" {
            status.phase = "error".into();
            status.message = format!("Le proxy s'est arrêté : {error}");
        }
    }
}

pub struct AppState {
    shared: Arc<SharedState>,
    cancel: Mutex<Option<CancellationToken>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            shared: Arc::new(SharedState {
                status: Mutex::new(StatusSnapshot::idle()),
            }),
            cancel: Mutex::new(None),
        }
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.lock().ok().and_then(|mut value| value.take()) {
            cancel.cancel();
        }
    }
}

#[derive(Clone)]
struct CaptureHandler {
    is_profile_request: bool,
    certificate_der: Arc<Vec<u8>>,
    output_directory: Arc<PathBuf>,
    shared: Arc<SharedState>,
    cancel: CancellationToken,
}

impl CaptureHandler {
    fn certificate_response(&self) -> Response<Body> {
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/x-x509-ca-cert")
            .header(
                header::CONTENT_DISPOSITION,
                "attachment; filename=SwagEx-CA.cer",
            )
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(self.certificate_der.as_ref().clone()))
            .expect("valid certificate response")
    }
}

impl HttpHandler for CaptureHandler {
    async fn handle_request(
        &mut self,
        _context: &HttpContext,
        request: Request<Body>,
    ) -> RequestOrResponse {
        if request.method() == Method::GET
            && matches!(request.uri().path(), "/certificate" | "/SwagEx-CA.cer")
        {
            return self.certificate_response().into();
        }

        self.is_profile_request = request.uri().path() == PROFILE_PATH;
        request.into()
    }

    async fn handle_response(
        &mut self,
        _context: &HttpContext,
        response: Response<Body>,
    ) -> Response<Body> {
        if !self.is_profile_request {
            return response;
        }

        let (parts, body) = response.into_parts();
        let collected = match body.collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(_) => return Response::from_parts(parts, Body::empty()),
        };
        let forwarded_body = collected.clone();
        let capture_body =
            decode_http_body(&parts.headers, &collected).unwrap_or_else(|| collected.to_vec());

        let output_directory = Arc::clone(&self.output_directory);
        let result = tokio::task::spawn_blocking(move || {
            let profile = decode_profile(&capture_body).ok()?;
            let command = profile.get("command").and_then(serde_json::Value::as_str)?;
            if !matches!(command, "HubUserLogin" | "GuestLogin") {
                return None;
            }
            Some(write_profile(&profile, &output_directory))
        })
        .await;

        if let Ok(Some(Ok(exported))) = result {
            self.shared
                .record_export(exported.path, exported.display_name);
            let cancel = self.cancel.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
                cancel.cancel();
            });
        }

        Response::from_parts(parts, Body::from(forwarded_body))
    }

    async fn should_intercept_connect(
        &mut self,
        _context: &HttpContext,
        request: &Request<Body>,
    ) -> bool {
        request
            .uri()
            .authority()
            .is_some_and(|authority| is_game_host(authority.host()))
    }

    async fn should_intercept_tls(
        &mut self,
        _context: &HttpContext,
        client_hello: hudsucker::rustls::server::ClientHello<'_>,
    ) -> bool {
        client_hello.server_name().is_some_and(is_game_host)
    }
}

fn is_game_host(host: &str) -> bool {
    host == "qpyou.cn" || host.ends_with(".qpyou.cn")
}

fn decode_http_body(headers: &header::HeaderMap, body: &[u8]) -> Option<Vec<u8>> {
    let encoding = headers
        .get(header::CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())?
        .trim()
        .to_ascii_lowercase();

    let mut decoded = Vec::new();
    match encoding.as_str() {
        "gzip" | "x-gzip" => GzDecoder::new(body).read_to_end(&mut decoded).ok()?,
        "deflate" => ZlibDecoder::new(body).read_to_end(&mut decoded).ok()?,
        "identity" => return Some(body.to_vec()),
        _ => return None,
    };
    Some(decoded)
}

async fn bind_listener(address: Ipv4Addr) -> anyhow::Result<TcpListener> {
    match TcpListener::bind((address, PREFERRED_PROXY_PORT)).await {
        Ok(listener) => Ok(listener),
        Err(_) => Ok(TcpListener::bind((address, 0)).await?),
    }
}

fn local_ipv4() -> anyhow::Result<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.connect("1.1.1.1:80")?;
    match socket.local_addr()?.ip() {
        IpAddr::V4(address) if !address.is_loopback() => Ok(address),
        _ => anyhow::bail!("aucune adresse Wi-Fi locale n'a été trouvée"),
    }
}

#[tauri::command]
pub fn export_status(state: State<'_, AppState>) -> StatusSnapshot {
    state.shared.snapshot()
}

#[tauri::command]
pub async fn start_export(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StatusSnapshot, String> {
    if let Some(previous) = state
        .cancel
        .lock()
        .map_err(|_| "état interne indisponible")?
        .take()
    {
        previous.cancel();
    }

    let result = async {
        let app_data = app.path().app_data_dir()?;
        let output_directory = app.path().download_dir()?;
        let local_ip = local_ipv4()?;
        let certificate = ensure_certificate(&app_data.join("certificate"))?;
        let certificate_was_created = certificate.was_created;
        let certificate_der = Arc::new(certificate.der);
        let listener = bind_listener(local_ip).await?;
        let port = listener.local_addr()?.port();
        let certificate_url = format!("http://{local_ip}:{port}/certificate");
        let cancel = CancellationToken::new();

        let handler = CaptureHandler {
            is_profile_request: false,
            certificate_der,
            output_directory: Arc::new(output_directory),
            shared: Arc::clone(&state.shared),
            cancel: cancel.clone(),
        };
        let authority = RcgenAuthority::new(certificate.issuer, 128, aws_lc_rs::default_provider());
        let proxy = Proxy::builder()
            .with_listener(listener)
            .with_ca(authority)
            .with_rustls_connector(aws_lc_rs::default_provider())
            .with_http_handler(handler)
            .with_graceful_shutdown(cancel.clone().cancelled_owned())
            .build()?;

        let status = StatusSnapshot {
            phase: "listening".into(),
            message: "SwagEx attend la connexion du jeu.".into(),
            local_ip: Some(local_ip.to_string()),
            port: Some(port),
            certificate_url: Some(certificate_url),
            certificate_was_created,
            export_path: None,
            profile_name: None,
        };
        state.shared.replace(status.clone());
        *state
            .cancel
            .lock()
            .map_err(|_| anyhow::anyhow!("état interne indisponible"))? = Some(cancel);

        let shared = Arc::clone(&state.shared);
        tauri::async_runtime::spawn(async move {
            if let Err(error) = proxy.start().await {
                shared.record_proxy_error(error);
            }
        });

        anyhow::Ok(status)
    }
    .await;

    result.map_err(|error| {
        let message = error.to_string();
        state.shared.replace(StatusSnapshot {
            phase: "error".into(),
            message: message.clone(),
            ..StatusSnapshot::idle()
        });
        message
    })
}

#[tauri::command]
pub fn cancel_export(state: State<'_, AppState>) -> Result<StatusSnapshot, String> {
    if let Some(cancel) = state
        .cancel
        .lock()
        .map_err(|_| "état interne indisponible")?
        .take()
    {
        cancel.cancel();
    }
    let status = StatusSnapshot::idle();
    state.shared.replace(status.clone());
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_intercepts_the_game_domain() {
        assert!(is_game_host("summonerswar-eu-lb.qpyou.cn"));
        assert!(is_game_host("qpyou.cn"));
        assert!(!is_game_host("example.com"));
        assert!(!is_game_host("qpyou.cn.example.com"));
    }
}
