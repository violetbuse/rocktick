use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use replace_err::ReplaceErr;
use reqwest::Client;
use tokio::{select, sync::mpsc};
use tokio_stream::{StreamExt, wrappers::ReceiverStream};
use tonic::Request;

use crate::{
    broker::grpc::{self, broker_client::BrokerClient},
    drone::{DroneState, util::resolve_public_ip},
};

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

    let response = req.send().await.map_err(|err| {
        if err.is_timeout() {
            return format!("Request timed out: {err:?}");
        }

        format!("Error sending request {err:?}")
    })?;

    Ok(response)
}

async fn run_job(job: grpc::JobSpec, state: DroneState) {
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
    if remaining > 5000 {
        millis_until = 5000;
    } else if remaining > 0 {
        millis_until = remaining as u64
    }

    millis_until += rand::random_range(0..450);

    tokio::time::sleep(Duration::from_millis(millis_until)).await;

    println!("Executing job {}", job.job_id);
    let executed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time before unix expoch")
        .as_secs() as i64;
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

            grpc::JobExecution {
                job_id: job.job_id,
                success,
                lock_nonce: job.lock_nonce,
                response: Some(grpc::Response {
                    status,
                    headers,
                    body: text,
                }),
                response_error: None,
                req_method: job.method,
                req_url: job.url,
                req_headers: job.headers,
                req_body: job.body,
                executed_at,
            }
        }
        Err(error) => grpc::JobExecution {
            job_id: job.job_id,
            success: false,
            lock_nonce: job.lock_nonce,
            response: None,
            response_error: Some(error),
            req_method: job.method,
            req_url: job.url,
            req_headers: job.headers,
            req_body: job.body,
            executed_at,
        },
    };

    state.exec_results.lock().await.push(execution);
}

async fn fetch_and_start_jobs(state: DroneState) -> anyhow::Result<()> {
    let mut client = BrokerClient::connect(state.broker_url.clone()).await?;
    let mut jobs_stream = client
        .get_jobs(Request::new(grpc::GetJobsRequest {
            region: state.region.clone(),
        }))
        .await?
        .into_inner();

    tokio::spawn(async move {
        loop {
            match jobs_stream.message().await {
                Err(status) => {
                    let _ = state.error_tx.send(status.into()).await;
                    break;
                }
                Ok(None) => {
                    break;
                }
                Ok(Some(job)) => {
                    tokio::spawn(run_job(job, state.clone()));
                }
            }
        }
    });

    Ok(())
}

async fn poll_jobs_loop(state: DroneState) -> anyhow::Result<()> {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        fetch_and_start_jobs(state.clone()).await?;
    }
}

async fn submit_job_results(state: DroneState) -> anyhow::Result<()> {
    let mut client = BrokerClient::connect(state.broker_url.clone()).await?;
    let execution_results: Vec<grpc::JobExecution> =
        state.exec_results.lock().await.drain(..).collect();

    if !execution_results.is_empty() {
        let (tx, rx) = mpsc::channel(1);

        let iter_state = state.clone();
        tokio::spawn(async move {
            let mut remaining = Vec::new();

            for item in execution_results {
                if tx.send(item.clone()).await.is_err() {
                    remaining.push(item);
                }
            }

            if !remaining.is_empty() {
                iter_state.exec_results.lock().await.append(&mut remaining);
            }
        });

        let submission_state = state.clone();
        tokio::spawn(async move {
            let req = Request::new(ReceiverStream::new(rx));

            if let Err(e) = client.record_execution(req).await {
                eprintln!("Error submitting job results {e:?}");
                let _ = submission_state.error_tx.send(e.into()).await;
            }
        });
    }

    Ok(())
}

async fn submit_job_results_loop(state: DroneState) -> anyhow::Result<()> {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        submit_job_results(state.clone()).await?;
    }
}

pub async fn start_job_executor(state: DroneState) -> anyhow::Result<()> {
    select! {
      poll_res = poll_jobs_loop(state.clone()) => {poll_res?;},
      submit_res = submit_job_results_loop(state.clone()) => {submit_res?;},
    }

    Ok(())
}
