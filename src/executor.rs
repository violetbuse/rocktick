use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use replace_err::ReplaceErr;
use reqwest::Client;
use tokio::{
    net::lookup_host,
    select,
    sync::{Mutex, mpsc},
};
use tokio_stream::{StreamExt, wrappers::ReceiverStream};
use tonic::Request;

use crate::{
    ExecutorOptions, GLOBAL_CONFIG,
    broker::{self, GetJobsRequest, JobExecution, JobSpec, broker_client::BrokerClient},
};

#[derive(Debug, Clone)]
pub struct Config {
    broker_url: String,
    region: String,
}

impl Config {
    pub async fn from_cli(options: ExecutorOptions) -> Self {
        Self {
            broker_url: options.broker_url,
            region: options.region,
        }
    }
}

#[derive(Debug, Clone)]
struct ExecutorState {
    exec_results: Arc<Mutex<Vec<JobExecution>>>,
    broker_url: String,
    region: String,
}

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => ipv4.is_private() || ipv4.is_loopback() || ipv4.is_link_local(),
        IpAddr::V6(ipv6) => {
            // Loopback, unspecified, unique local (fc00::/7)
            if ipv6.is_loopback() || ipv6.is_unspecified() {
                return true;
            }

            // Check specifically for fdaa::/16
            let segments = ipv6.segments(); // 8 u16 segments
            if segments[0] == 0xfdaa {
                return true;
            }

            // Also block the general unique local addresses (fc00::/7)
            ipv6.is_unique_local()
        }
    }
}

async fn resolve_public_ip(url: &str) -> Option<SocketAddr> {
    let url = url::Url::parse(url).ok()?;

    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }

    let host = url.host_str()?;
    let port = url.port_or_known_default().unwrap_or(80);

    let addrs = lookup_host((host, port)).await.ok()?;

    let mut public_addr = None;
    let allow_private_addrs = GLOBAL_CONFIG.get().unwrap().is_dev;

    for addr in addrs {
        if allow_private_addrs {
            public_addr = Some(addr);
            break;
        }

        if !is_private_ip(&addr.ip()) {
            public_addr = Some(addr);
            break;
        }
    }

    public_addr
}

async fn send_request_to_ip(
    job_id: &str,
    url: &str,
    ip_addr: SocketAddr,
    method: String,
    headers: HashMap<String, String>,
    body: Option<String>,
    timeout_ms: u64,
) -> Result<reqwest::Response, String> {
    let url = url::Url::parse(url).replace_err("Invalid URL")?;
    let host = url.host_str().ok_or("Invalid host.")?;

    let client = Client::builder()
        .resolve(host, ip_addr)
        .timeout(Duration::from_millis(timeout_ms))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .replace_err("Unable to build client.")?;

    let method = method.parse().replace_err("Invalid method.")?;

    let mut req = client.request(method, url);

    for (header_name, value) in headers {
        req = req.header(header_name, value);
    }

    req = req.header("Rocktick-Job-Id", job_id);

    if let Some(body) = body {
        req = req.body(body);
    }

    let response = req
        .send()
        .await
        .map_err(|err| format!("Error sending request {err:?}"))?;

    Ok(response)
}

async fn run_job(job: JobSpec, state: ExecutorState) {
    // check if the ip address is unallowed
    let public_addr = resolve_public_ip(&job.url)
        .await
        .ok_or("Unable to resolve a public ip address.");

    let mut millis_until = 0;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time before unix expoch")
        .as_millis() as i64;
    let remaining = job.scheduled_at * 1000 - now;
    if remaining > 0 {
        millis_until = remaining as u64
    }

    millis_until += rand::random_range(0..350);

    tokio::time::sleep(Duration::from_millis(millis_until)).await;

    println!("Executing job {}", job.job_id);
    let response = match public_addr {
        Ok(addr) => {
            send_request_to_ip(
                &job.job_id,
                &job.url,
                addr,
                job.method.clone(),
                job.headers.clone(),
                job.body.clone(),
                job.timeout_ms as u64,
            )
            .await
        }
        Err(err) => Err(err.to_string()),
    };

    let execution = match response {
        Ok(res) => {
            let success = res.status().is_success();
            let status = res.status().as_u16() as i64;
            let headers = res
                .headers()
                .iter()
                .filter_map(|(k, v)| Some((k.to_string(), v.to_str().ok()?.to_string())))
                .collect();

            let mut body_bytes = Vec::new();
            let mut stream = res.bytes_stream();

            while let Some(item) = stream.next().await {
                if let Ok(chunk) = item {
                    let limit_left = job.max_response_bytes as usize - body_bytes.len();

                    if limit_left == 0 {
                        break;
                    }

                    if chunk.len() > limit_left {
                        body_bytes.extend_from_slice(&chunk[..limit_left]);
                        break;
                    } else {
                        body_bytes.extend_from_slice(&chunk);
                    }
                } else {
                    break;
                }
            }

            let text = String::from_utf8_lossy(&body_bytes).to_string();

            JobExecution {
                job_id: job.job_id,
                success,
                lock_nonce: job.lock_nonce,
                response: Some(broker::Response {
                    status,
                    headers,
                    body: text,
                }),
                response_error: None,
                req_method: job.method,
                req_url: job.url,
                req_headers: job.headers,
                req_body: job.body,
            }
        }
        Err(error) => JobExecution {
            job_id: job.job_id,
            success: false,
            lock_nonce: job.lock_nonce,
            response: None,
            response_error: Some(error),
            req_method: job.method,
            req_url: job.url,
            req_headers: job.headers,
            req_body: job.body,
        },
    };

    state.exec_results.lock().await.push(execution);
}

async fn fetch_and_start_jobs(state: ExecutorState) -> anyhow::Result<()> {
    let mut client = BrokerClient::connect(state.broker_url.clone()).await?;
    let mut jobs_stream = client
        .get_jobs(Request::new(GetJobsRequest {
            region: state.region.clone(),
        }))
        .await?
        .into_inner();

    while let Some(job) = jobs_stream.message().await? {
        tokio::spawn(run_job(job, state.clone()));
    }

    Ok(())
}

async fn poll_jobs_loop(state: ExecutorState) -> anyhow::Result<()> {
    loop {
        tokio::time::sleep(Duration::from_secs(3)).await;
        fetch_and_start_jobs(state.clone()).await?;
    }
}

async fn submit_job_results(state: ExecutorState) -> anyhow::Result<()> {
    let mut client = BrokerClient::connect(state.broker_url.clone()).await?;
    let execution_results: Vec<JobExecution> = state.exec_results.lock().await.drain(..).collect();

    if !execution_results.is_empty() {
        let (tx, rx) = mpsc::channel(1);

        tokio::spawn(async move {
            let mut remaining = Vec::new();

            for item in execution_results {
                if tx.send(item.clone()).await.is_err() {
                    remaining.push(item);
                }
            }

            if !remaining.is_empty() {
                state.exec_results.lock().await.append(&mut remaining);
            }
        });

        let req = Request::new(ReceiverStream::new(rx));

        if let Err(e) = client.record_execution(req).await {
            eprintln!("Error submitting job results {e:?}");
        }
    }

    Ok(())
}

async fn submit_job_results_loop(state: ExecutorState) -> anyhow::Result<()> {
    loop {
        tokio::time::sleep(Duration::from_secs(5)).await;
        submit_job_results(state.clone()).await?;
    }
}

pub async fn start(config: Config) -> anyhow::Result<()> {
    tokio::time::sleep(Duration::from_secs(rand::random_range(0..4))).await;

    let state = ExecutorState {
        exec_results: Arc::new(Mutex::new(Vec::new())),
        broker_url: config.broker_url.clone(),
        region: config.region.clone(),
    };

    select! {
      poll_res = poll_jobs_loop(state.clone()) => {poll_res?;},
      submit_res = submit_job_results_loop(state.clone()) => {submit_res?;},
    }

    Ok(())
}
