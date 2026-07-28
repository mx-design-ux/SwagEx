#[cfg(target_os = "windows")]
use std::collections::HashMap;

#[cfg(any(target_os = "windows", test))]
const HOSTS_BLOCK_BEGIN: &str = "# BEGIN SwagEx Steam";
#[cfg(any(target_os = "windows", test))]
const HOSTS_BLOCK_END: &str = "# END SwagEx Steam";

#[cfg(any(target_os = "windows", test))]
const STEAM_HOSTS: [(&str, [u8; 4]); 6] = [
    ("summonerswar-eu-lb.qpyou.cn", [127, 11, 12, 13]),
    ("summonerswar-gb-lb.qpyou.cn", [127, 11, 12, 14]),
    ("summonerswar-sea-lb.qpyou.cn", [127, 11, 12, 15]),
    ("summonerswar-jp-lb.qpyou.cn", [127, 11, 12, 16]),
    ("summonerswar-kr-lb.qpyou.cn", [127, 11, 12, 17]),
    ("summonerswar-cn-lb.qpyou.cn", [127, 11, 12, 18]),
];

#[derive(Default)]
pub struct SteamRouteState {
    #[cfg(target_os = "windows")]
    hosts_active: bool,
}

impl SteamRouteState {
    pub fn stop(&mut self) -> anyhow::Result<()> {
        #[cfg(target_os = "windows")]
        {
            let restored_stale_route = restore_hosts_file()?;
            if self.hosts_active || restored_stale_route {
                flush_dns_cache()?;
            }
            self.hosts_active = false;
        }

        Ok(())
    }
}

impl Drop for SteamRouteState {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(target_os = "windows")]
pub struct PreparedSteamRoute {
    listeners: Vec<SteamEndpoint>,
    upstream_hosts: HashMap<String, std::net::IpAddr>,
}

#[cfg(target_os = "windows")]
impl PreparedSteamRoute {
    pub fn upstream_hosts(&self) -> HashMap<String, std::net::IpAddr> {
        self.upstream_hosts.clone()
    }
}

#[cfg(target_os = "windows")]
struct SteamEndpoint {
    hostname: &'static str,
    listener: tokio::net::TcpListener,
}

pub fn recover_stale_route() -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    if restore_hosts_file()? {
        flush_dns_cache()?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
pub async fn prepare() -> anyhow::Result<PreparedSteamRoute> {
    use std::net::{IpAddr, Ipv4Addr};

    // A previous forced shutdown can leave the owned hosts block behind. Clear
    // only that marked block before resolving the real game endpoints.
    restore_hosts_file()?;
    flush_dns_cache()?;
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    let mut upstream_hosts = HashMap::new();
    let mut listeners = Vec::with_capacity(STEAM_HOSTS.len());

    for (hostname, loopback_octets) in STEAM_HOSTS {
        let upstream = resolve_upstream(hostname).await?;
        upstream_hosts.insert(hostname.to_string(), upstream);

        let loopback = Ipv4Addr::from(loopback_octets);
        let listener = tokio::net::TcpListener::bind((loopback, 443))
            .await
            .map_err(|error| {
                anyhow::anyhow!(
                    "impossible de préparer le relais Steam sur {loopback}:443 : {error}"
                )
            })?;
        listeners.push(SteamEndpoint { hostname, listener });
    }

    // Keep the type explicit so a future IPv6 endpoint cannot silently enter
    // the Windows hosts-file route without a corresponding transparent relay.
    debug_assert!(
        upstream_hosts
            .values()
            .all(|address| matches!(address, IpAddr::V4(_) | IpAddr::V6(_)))
    );

    Ok(PreparedSteamRoute {
        listeners,
        upstream_hosts,
    })
}

#[cfg(target_os = "windows")]
async fn resolve_upstream(hostname: &str) -> anyhow::Result<std::net::IpAddr> {
    const MAX_ATTEMPTS: usize = 20;
    const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

    let mut last_error = None;
    for attempt in 0..MAX_ATTEMPTS {
        match tokio::net::lookup_host((hostname, 443)).await {
            Ok(addresses) => {
                if let Some(address) = first_remote_address(addresses) {
                    return Ok(address);
                }
            }
            Err(error) => last_error = Some(error),
        }

        if attempt + 1 < MAX_ATTEMPTS {
            tokio::time::sleep(RETRY_DELAY).await;
        }
    }

    let _ = flush_dns_cache();
    if let Some(error) = last_error {
        anyhow::bail!("Windows ne parvient pas à résoudre {hostname} : {error}");
    }
    anyhow::bail!(
        "Windows conserve une ancienne redirection réseau pour {hostname}. Fermez Summoners War puis cliquez sur Réessayer."
    )
}

#[cfg(any(target_os = "windows", test))]
fn first_remote_address(
    addresses: impl IntoIterator<Item = std::net::SocketAddr>,
) -> Option<std::net::IpAddr> {
    addresses
        .into_iter()
        .map(|address| address.ip())
        .find(|address| !address.is_loopback())
}

#[cfg(target_os = "windows")]
pub fn activate(
    state: &mut SteamRouteState,
    prepared: PreparedSteamRoute,
    proxy_address: std::net::SocketAddr,
    cancel: tokio_util::sync::CancellationToken,
) -> anyhow::Result<()> {
    install_hosts_block()?;
    state.hosts_active = true;
    if let Err(error) = flush_dns_cache() {
        let _ = state.stop();
        return Err(error);
    }

    for endpoint in prepared.listeners {
        let endpoint_cancel = cancel.clone();
        tauri::async_runtime::spawn(run_transparent_listener(
            endpoint,
            proxy_address,
            endpoint_cancel,
        ));
    }

    Ok(())
}

#[cfg(target_os = "windows")]
async fn run_transparent_listener(
    endpoint: SteamEndpoint,
    proxy_address: std::net::SocketAddr,
    cancel: tokio_util::sync::CancellationToken,
) {
    loop {
        let accepted = tokio::select! {
            _ = cancel.cancelled() => break,
            accepted = endpoint.listener.accept() => accepted,
        };

        let Ok((downstream, _)) = accepted else {
            break;
        };
        let hostname = endpoint.hostname;
        tauri::async_runtime::spawn(async move {
            let _ = relay_through_http_proxy(downstream, proxy_address, hostname).await;
        });
    }
}

#[cfg(target_os = "windows")]
async fn relay_through_http_proxy(
    mut downstream: tokio::net::TcpStream,
    proxy_address: std::net::SocketAddr,
    hostname: &str,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut upstream = tokio::net::TcpStream::connect(proxy_address).await?;
    upstream
        .write_all(
            format!("CONNECT {hostname}:443 HTTP/1.1\r\nHost: {hostname}:443\r\n\r\n").as_bytes(),
        )
        .await?;

    let mut response = Vec::with_capacity(256);
    let mut byte = [0_u8; 1];
    while response.len() < 8192 {
        upstream.read_exact(&mut byte).await?;
        response.push(byte[0]);
        if response.ends_with(b"\r\n\r\n") {
            break;
        }
    }

    if !response.ends_with(b"\r\n\r\n") {
        anyhow::bail!("réponse CONNECT Steam incomplète");
    }
    let status_line = std::str::from_utf8(&response)?
        .lines()
        .next()
        .unwrap_or_default();
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or_default();
    if !(200..300).contains(&status_code) {
        anyhow::bail!("le proxy Steam a refusé la connexion : {status_line}");
    }

    tokio::io::copy_bidirectional(&mut downstream, &mut upstream).await?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn hosts_file_path() -> std::path::PathBuf {
    let windows_directory =
        std::env::var_os("SystemRoot").unwrap_or_else(|| std::ffi::OsString::from(r"C:\Windows"));
    std::path::PathBuf::from(windows_directory)
        .join("System32")
        .join("drivers")
        .join("etc")
        .join("hosts")
}

#[cfg(target_os = "windows")]
fn restore_hosts_file() -> anyhow::Result<bool> {
    let path = hosts_file_path();
    let contents = std::fs::read_to_string(&path)?;
    let cleaned = without_owned_hosts_block(&contents);
    let restored = cleaned != contents;
    if restored {
        std::fs::write(&path, cleaned)?;
    }
    Ok(restored)
}

#[cfg(target_os = "windows")]
fn install_hosts_block() -> anyhow::Result<()> {
    let path = hosts_file_path();
    let contents = std::fs::read_to_string(&path)?;
    let line_ending = if contents.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut updated = without_owned_hosts_block(&contents);
    if !updated.is_empty() && !updated.ends_with(line_ending) {
        updated.push_str(line_ending);
    }
    updated.push_str(HOSTS_BLOCK_BEGIN);
    updated.push_str(line_ending);
    for (hostname, loopback_octets) in STEAM_HOSTS {
        updated.push_str(&format!(
            "{}.{}.{}.{} {hostname}{line_ending}",
            loopback_octets[0], loopback_octets[1], loopback_octets[2], loopback_octets[3]
        ));
    }
    updated.push_str(HOSTS_BLOCK_END);
    updated.push_str(line_ending);
    std::fs::write(path, updated)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn flush_dns_cache() -> anyhow::Result<()> {
    let status = std::process::Command::new("ipconfig")
        .arg("/flushdns")
        .status()?;
    if !status.success() {
        anyhow::bail!("Windows n’a pas pu actualiser son cache DNS");
    }
    Ok(())
}

#[cfg(any(target_os = "windows", test))]
fn without_owned_hosts_block(contents: &str) -> String {
    let line_ending = if contents.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let had_trailing_newline = contents.ends_with('\n');
    let mut inside_owned_block = false;
    let mut kept = Vec::new();

    for line in contents.lines() {
        if line.trim() == HOSTS_BLOCK_BEGIN {
            inside_owned_block = true;
            continue;
        }
        if line.trim() == HOSTS_BLOCK_END {
            inside_owned_block = false;
            continue;
        }
        if !inside_owned_block {
            kept.push(line);
        }
    }

    let mut cleaned = kept.join(line_ending);
    if had_trailing_newline && !cleaned.is_empty() {
        cleaned.push_str(line_ending);
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_only_the_owned_hosts_block() {
        let original = concat!(
            "127.0.0.1 localhost\r\n",
            "# BEGIN SwagEx Steam\r\n",
            "127.11.12.13 summonerswar-eu-lb.qpyou.cn\r\n",
            "# END SwagEx Steam\r\n",
            "10.0.0.2 intranet\r\n",
        );

        assert_eq!(
            without_owned_hosts_block(original),
            "127.0.0.1 localhost\r\n10.0.0.2 intranet\r\n"
        );
    }

    #[test]
    fn leaves_an_unrelated_hosts_file_unchanged() {
        let original = "127.0.0.1 localhost\n10.0.0.2 intranet\n";
        assert_eq!(without_owned_hosts_block(original), original);
    }

    #[test]
    fn steam_endpoints_keep_distinct_loopback_addresses() {
        let mut addresses = STEAM_HOSTS
            .iter()
            .map(|(_, address)| *address)
            .collect::<Vec<_>>();
        addresses.sort_unstable();
        addresses.dedup();
        assert_eq!(addresses.len(), STEAM_HOSTS.len());
    }

    #[test]
    fn upstream_resolution_ignores_stale_loopback_addresses() {
        let addresses = [
            "127.11.12.13:443".parse().unwrap(),
            "34.160.156.240:443".parse().unwrap(),
        ];

        assert_eq!(
            first_remote_address(addresses),
            Some("34.160.156.240".parse().unwrap())
        );
    }
}
