use assert_cmd::prelude::*;
use holochain_cli_sandbox::cli::LaunchInfo;
use holochain_conductor_api::AppResponse;
use holochain_conductor_api::{AdminRequest, AdminResponse, AppAuthenticationRequest, AppRequest};
use holochain_types::app::InstalledAppId;
use holochain_types::prelude::{SerializedBytes, SerializedBytesError};
use holochain_websocket::{
    self as ws, ConnectRequest, WebsocketConfig, WebsocketReceiver, WebsocketSender,
};
use matches::assert_matches;
use std::future::Future;
use std::net::ToSocketAddrs;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdout, Command};
use which::which;

const WEBSOCKET_TIMEOUT: Duration = Duration::from_secs(3);

fn get_hc_built_path() -> &'static PathBuf {
    static HC_BUILT_PATH: OnceLock<PathBuf> = OnceLock::new();

    HC_BUILT_PATH.get_or_init(|| {
        let mut manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_path.push("../hc/Cargo.toml");

        println!("@@ Warning, Building `hc` binary!");

        let out = escargot::CargoBuild::new()
            .bin("hc")
            .current_target()
            .current_release()
            .manifest_path(manifest_path)
            // Not defined on CI
            .target_dir(PathBuf::from(
                option_env!("CARGO_TARGET_DIR").unwrap_or("./target"),
            ))
            .run()
            .unwrap();

        println!("@@ `hc` binary built");

        out.path().to_path_buf()
    })
}

fn get_holochain_built_path() -> &'static PathBuf {
    static HOLOCHAIN_BUILT_PATH: OnceLock<PathBuf> = OnceLock::new();

    HOLOCHAIN_BUILT_PATH.get_or_init(|| {
        let mut manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        manifest_path.push("../holochain/Cargo.toml");

        println!("@@ Warning, Building `holochain` binary!");

        let out = escargot::CargoBuild::new()
            .bin("holochain")
            .current_target()
            .current_release()
            .manifest_path(manifest_path)
            .target_dir(PathBuf::from(
                option_env!("CARGO_TARGET_DIR").unwrap_or("./target"),
            ))
            .run()
            .unwrap();

        println!("@@ `holochain` binary built");

        out.path().to_path_buf()
    })
}

async fn new_websocket_client_for_port<D>(port: u16) -> anyhow::Result<(WebsocketSender, WsPoll)>
where
    D: std::fmt::Debug,
    SerializedBytes: TryInto<D, Error = SerializedBytesError>,
{
    println!("Client for address: {:?}", format!("localhost:{port}"));
    let (tx, rx) = ws::connect(
        Arc::new(WebsocketConfig::CLIENT_DEFAULT),
        ConnectRequest::new(
            format!("localhost:{port}")
                .to_socket_addrs()
                .unwrap()
                .next()
                .unwrap(),
        ),
    )
    .await?;

    Ok((tx, WsPoll::new::<D>(rx)))
}

async fn get_app_info(admin_port: u16, installed_app_id: InstalledAppId, port: u16) {
    tracing::debug!(calling_app_interface = ?port, admin = ?admin_port);

    let (admin_tx, _admin_rx) = new_websocket_client_for_port::<AdminResponse>(admin_port)
        .await
        .unwrap_or_else(|_| panic!("Failed to connect to conductor on port [{}]", admin_port));

    let issue_token_response = admin_tx
        .request(AdminRequest::IssueAppAuthenticationToken(
            installed_app_id.into(),
        ))
        .await
        .unwrap();
    let token = match issue_token_response {
        AdminResponse::AppAuthenticationTokenIssued(issued) => issued.token,
        _ => panic!("Unexpected response {:?}", issue_token_response),
    };

    let (app_tx, _rx) = new_websocket_client_for_port::<AppResponse>(port)
        .await
        .unwrap_or_else(|_| panic!("Failed to connect to conductor on port [{}]", port));
    app_tx
        .authenticate(AppAuthenticationRequest { token })
        .await
        .unwrap();

    let request = AppRequest::AppInfo;
    let response = app_tx.request(request);
    let r: AppResponse = check_timeout(response).await;
    assert_matches!(r, AppResponse::AppInfo(Some(_)));
}

async fn check_timeout<T>(response: impl Future<Output = std::io::Result<T>>) -> T {
    match tokio::time::timeout(WEBSOCKET_TIMEOUT, response).await {
        Ok(response) => response.expect("Calling websocket failed"),
        Err(_) => {
            panic!("Timed out on request after {:?}", WEBSOCKET_TIMEOUT);
        }
    }
}

async fn package_fixture_if_not_packaged() {
    if PathBuf::from("tests/fixtures/my-app/my-fixture-app.happ").exists() {
        return;
    }

    println!("@@ Package Fixture");

    let mut cmd = get_hc_command();

    cmd.arg("dna").arg("pack").arg("tests/fixtures/my-app/dna");

    println!("@@ {cmd:?}");

    cmd.status().await.expect("Failed to pack DNA");

    let mut cmd = get_hc_command();

    cmd.arg("app").arg("pack").arg("tests/fixtures/my-app");

    println!("@@ {cmd:?}");

    cmd.status().await.expect("Failed to pack hApp");

    println!("@@ Package Fixture Complete");
}

async fn clean_sandboxes() {
    println!("@@ Clean");

    let mut cmd = get_sandbox_command();

    cmd.arg("clean");

    println!("@@ {cmd:?}");

    cmd.status().await.unwrap();

    println!("@@ Clean Complete");
}

/// Generates a new sandbox with a single app deployed and tries to get app info
#[tokio::test(flavor = "multi_thread")]
async fn generate_sandbox_and_connect() {
    clean_sandboxes().await;
    package_fixture_if_not_packaged().await;

    holochain_trace::test_run();
    let mut cmd = get_sandbox_command();
    cmd.env("RUST_BACKTRACE", "1")
        .arg(format!(
            "--holochain-path={}",
            get_holochain_bin_path().to_str().unwrap()
        ))
        .arg("--piped")
        .arg("generate")
        .arg("--in-process-lair")
        .arg("--run=0")
        .arg("tests/fixtures/my-app/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    println!("@@ {cmd:?}");

    let mut hc_admin = cmd.spawn().expect("Failed to spawn holochain");

    let mut child_stdin = hc_admin.stdin.take().unwrap();
    child_stdin.write_all(b"test-phrase\n").await.unwrap();
    drop(child_stdin);

    let mut stdout = hc_admin.stdout.take().unwrap();
    let launch_info = get_launch_info(&mut stdout).await;

    // - Make a call to list app info to the port
    get_app_info(
        launch_info.admin_port,
        "test-app".into(),
        *launch_info.app_ports.first().expect("No app ports found"),
    )
    .await;
}

/// Generates a new sandbox with a single app deployed and tries to list DNA
#[tokio::test(flavor = "multi_thread")]
async fn generate_sandbox_and_call_list_dna() {
    clean_sandboxes().await;
    package_fixture_if_not_packaged().await;

    holochain_trace::test_run();
    let mut cmd = get_sandbox_command();
    cmd.env("RUST_BACKTRACE", "1")
        .arg(format!(
            "--holochain-path={}",
            get_holochain_bin_path().to_str().unwrap()
        ))
        .arg("--piped")
        .arg("generate")
        .arg("--in-process-lair")
        .arg("--run=0")
        .arg("tests/fixtures/my-app/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let mut hc_admin = cmd.spawn().expect("Failed to spawn holochain");
    let mut child_stdin = hc_admin.stdin.take().unwrap();
    child_stdin.write_all(b"test-phrase\n").await.unwrap();
    drop(child_stdin);

    let mut stdout = hc_admin.stdout.take().unwrap();
    let launch_info = get_launch_info(&mut stdout).await;

    let mut cmd = get_sandbox_command();
    cmd.env("RUST_BACKTRACE", "1")
        .arg("call")
        .arg(format!("--running={}", launch_info.admin_port))
        .arg("list-dnas")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    let mut hc_call = cmd.spawn().expect("Failed to spawn holochain");

    let exit_code = hc_call.wait().await.unwrap();
    assert!(exit_code.success());
}

fn get_hc_command() -> Command {
    Command::new(which("hc").unwrap_or_else(|_| get_hc_built_path().clone()))
}

fn get_holochain_bin_path() -> PathBuf {
    which("holochain").unwrap_or_else(|_| get_holochain_built_path().clone())
}

fn get_sandbox_command() -> Command {
    match which("hc-sandbox") {
        Ok(p) => Command::new(p),
        Err(_) => Command::from(std::process::Command::cargo_bin("hc-sandbox").unwrap()),
    }
}

async fn get_launch_info(stdout: &mut ChildStdout) -> LaunchInfo {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        println!("@@@@@-{line}-@@@@@");
        if let Some(index) = line.find("#!0") {
            let launch_info_str = &line[index + 3..].trim();
            return serde_json::from_str::<LaunchInfo>(launch_info_str).unwrap();
        }
    }

    panic!("Unable to find launch info in sandbox output");
}

struct WsPoll(tokio::task::JoinHandle<()>);
impl Drop for WsPoll {
    fn drop(&mut self) {
        self.0.abort();
    }
}
impl WsPoll {
    fn new<D>(mut rx: WebsocketReceiver) -> Self
    where
        D: std::fmt::Debug,
        SerializedBytes: TryInto<D, Error = SerializedBytesError>,
    {
        WsPoll(tokio::task::spawn(async move {
            while rx.recv::<D>().await.is_ok() {}
            println!("Poller exiting");
        }))
    }
}
