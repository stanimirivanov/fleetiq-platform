//! Pinned real S3 fixture; all commands are bounded and resources positively owned.
use object_store::{
    aws::{AwsAuthorizer, AwsCredential},
    client::{HttpRequest, HttpRequestBody},
};
use std::{
    process::{Command, Output},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
const IMAGE: &str = include_str!("../fixtures/s3-image.txt");
const ACCESS: &str = "archive-fixture-access";
const SECRET: &str = "fixture-secret";
const OWNER: &str = "com.fleetiq.archive.fixture";
pub struct Provider {
    host: String,
    name: String,
    network: String,
    active: bool,
    pub endpoint: String,
}
fn command(host: Option<&str>, args: &[&str]) -> Output {
    let mut cmd = Command::new("timeout");
    cmd.args(["--kill-after=5", "120", "docker"]);
    if let Some(host) = host {
        cmd.args(["--host", host])
            .env_remove("DOCKER_HOST")
            .env_remove("DOCKER_CONTEXT");
    }
    cmd.args(args)
        .env("RUSTFS_ACCESS_KEY", ACCESS)
        .env("RUSTFS_SECRET_KEY", SECRET)
        .output()
        .expect("Docker fixture prerequisite unavailable")
}
fn checked(host: Option<&str>, args: &[&str]) -> String {
    let output = command(host, args);
    assert!(
        output.status.success(),
        "owned S3 fixture command failed: {} status {:?}",
        args[0],
        output.status.code()
    );
    String::from_utf8(output.stdout)
        .expect("fixture metadata must be UTF-8")
        .trim()
        .to_owned()
}
impl Provider {
    pub async fn start() -> Self {
        let host = checked(
            None,
            &[
                "context",
                "inspect",
                "--format",
                "{{.Endpoints.docker.Host}}",
            ],
        );
        assert!(
            std::env::var("DOCKER_HOST").ok().is_none_or(|v| v == host),
            "Docker host/context mismatch"
        );
        assert!(
            host.starts_with("unix:///"),
            "local Unix Docker engine required"
        );
        assert_eq!(
            checked(Some(&host), &["info", "--format", "{{.OSType}}"]),
            "linux"
        );
        let name = format!(
            "fleetiq-archive-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let mut fixture = Self {
            host,
            name: name.clone(),
            network: format!("{name}-net"),
            active: true,
            endpoint: String::new(),
        };
        let label = format!("{OWNER}={name}");
        checked(
            Some(&fixture.host),
            &["network", "create", "--label", &label, &fixture.network],
        );
        let image = IMAGE.trim();
        assert!(
            image.starts_with("rustfs/rustfs:1.0.4@sha256:") && image.split_once("@sha256:").is_some_and(|(_,h)|h.len()==64 && h.bytes().all(|b|b.is_ascii_hexdigit())),
            "fixture image must retain reviewed digest"
        );
        if !command(Some(&fixture.host), &["image", "inspect", image])
            .status
            .success()
        {
            checked(Some(&fixture.host), &["pull", image]);
        }
        fixture.launch(&label, image);
        let ready = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                if let Ok(response) = fixture.raw(reqwest::Method::PUT, "", Vec::new()).await
                    && (response.status().is_success()
                        || response.status() == reqwest::StatusCode::CONFLICT)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await;
        assert!(ready.is_ok(), "signed S3 fixture readiness failed");
        fixture
    }
    fn launch(&mut self, label: &str, image: &str) {
        checked(
            Some(&self.host),
            &[
                "run",
                "-d",
                "--platform",
                "linux/amd64",
                "--name",
                &self.name,
                "--label",
                label,
                "--network",
                &self.network,
                "--read-only",
                "--cap-drop",
                "ALL",
                "--security-opt",
                "no-new-privileges:true",
                "--memory",
                "1g",
                "--pids-limit",
                "256",
                "--cpus",
                "2",
                "--tmpfs",
                "/data:rw,size=128m,uid=10001,gid=10001",
                "--tmpfs",
                "/logs:rw,size=32m,uid=10001,gid=10001",
                "--tmpfs",
                "/tmp:rw,size=32m",
                "-p",
                "127.0.0.1::9000",
                "-e",
                "RUSTFS_ACCESS_KEY",
                "-e",
                "RUSTFS_SECRET_KEY",
                "-e",
                "RUSTFS_CONSOLE_ENABLE=false",
                image,
                "/data",
            ],
        );
        let state = checked(
            Some(&self.host),
            &[
                "inspect",
                "--format",
                "{{.State.Status}} {{.State.ExitCode}}",
                &self.name,
            ],
        );
        assert_eq!(
            state, "running 0",
            "provider must start without elevated privileges"
        );
        let port = checked(Some(&self.host), &["port", &self.name, "9000/tcp"]);
        assert!(
            port.starts_with("127.0.0.1:"),
            "fixture listener must remain loopback"
        );
        self.endpoint = format!("http://{port}");
    }
    pub fn config(&self) -> fleetiq_archive::objects::S3ArchiveConfig {
        fleetiq_archive::objects::S3ArchiveConfig::new(
            &self.endpoint,
            "archive-test",
            "us-east-1",
            ACCESS.into(),
            SECRET.into(),
        )
        .unwrap()
    }
    pub async fn raw(
        &self,
        method: reqwest::Method,
        key: &str,
        body: Vec<u8>,
    ) -> Result<reqwest::Response, Box<dyn std::error::Error>> {
        let uri = format!("{}/archive-test/{}", self.endpoint, key);
        let mut request = HttpRequest::new(HttpRequestBody::from(body.clone()));
        *request.method_mut() = method.clone();
        *request.uri_mut() = uri.parse()?;
        let credentials = AwsCredential {
            key_id: ACCESS.into(),
            secret_key: SECRET.into(),
            token: None,
        };
        AwsAuthorizer::new(&credentials, "s3", "us-east-1").try_authorize(&mut request, None)?;
        Ok(reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(2))
            .build()?
            .request(method, uri)
            .headers(request.headers().clone())
            .body(body)
            .send()
            .await?)
    }
    pub fn pause(&self) {
        checked(Some(&self.host), &["pause", &self.name]);
    }
    pub fn resume(&self) {
        checked(Some(&self.host), &["unpause", &self.name]);
    }
    fn cleanup(&mut self) -> bool {
        let format = format!("{{{{.Id}}}}|{{{{index .Config.Labels \"{OWNER}\"}}}}");
        let inspect = command(
            Some(&self.host),
            &["inspect", "--format", &format, &self.name],
        );
        if inspect.status.success() {
            let info = String::from_utf8_lossy(&inspect.stdout);
            let Some((id, owner)) = info.trim().split_once('|') else {
                return false;
            };
            if owner != self.name
                || !command(Some(&self.host), &["rm", "-fv", id])
                    .status
                    .success()
            {
                return false;
            }
        }
        let format = format!("{{{{.Id}}}}|{{{{index .Labels \"{OWNER}\"}}}}");
        let inspect = command(
            Some(&self.host),
            &["network", "inspect", "--format", &format, &self.network],
        );
        if inspect.status.success() {
            let info = String::from_utf8_lossy(&inspect.stdout);
            let Some((id, owner)) = info.trim().split_once('|') else {
                return false;
            };
            if owner != self.name
                || !command(Some(&self.host), &["network", "rm", id])
                    .status
                    .success()
            {
                return false;
            }
        }
        let filter = format!("label={OWNER}={}", self.name);
        let containers = command(
            Some(&self.host),
            &[
                "container",
                "ls",
                "-a",
                "--filter",
                &filter,
                "--format",
                "{{.ID}}",
            ],
        );
        let networks = command(
            Some(&self.host),
            &["network", "ls", "--filter", &filter, "--format", "{{.ID}}"],
        );
        containers.status.success()
            && networks.status.success()
            && containers.stdout.is_empty()
            && networks.stdout.is_empty()
    }
    pub fn close(mut self) {
        assert!(self.cleanup(), "owned fixture cleanup incomplete");
        self.active = false;
    }
}
impl Drop for Provider {
    fn drop(&mut self) {
        if self.active {
            let _ = self.cleanup();
        }
    }
}
