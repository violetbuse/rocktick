use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc, serde::ts_seconds};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum WaitDefinition {
    V1Struct {
        #[serde(with = "ts_seconds")]
        wait_until: DateTime<Utc>,
    },
    V1Tuple(#[serde(with = "ts_seconds")] DateTime<Utc>),
}

impl WaitDefinition {
    pub fn wait_until(&self) -> DateTime<Utc> {
        match self {
            WaitDefinition::V1Struct { wait_until, .. } => *wait_until,
            WaitDefinition::V1Tuple(wait_until) => *wait_until,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ChildDefinition {
    V1Struct {
        url: Url,
        input: serde_json::Value,
        max_retries: Option<i32>,
    },
    V1Tuple(Url, serde_json::Value),
}

impl ChildDefinition {
    pub fn url(&self) -> Url {
        match self {
            ChildDefinition::V1Struct { url, .. } => url.clone(),
            ChildDefinition::V1Tuple(url, _) => url.clone(),
        }
    }

    pub fn input(&self) -> serde_json::Value {
        match self {
            ChildDefinition::V1Struct { input, .. } => input.clone(),
            ChildDefinition::V1Tuple(_, input) => input.clone(),
        }
    }

    pub fn max_retries(&self) -> i32 {
        match self {
            ChildDefinition::V1Struct { max_retries, .. } => max_retries.unwrap_or(9),
            ChildDefinition::V1Tuple(_, _) => 9,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ReturnedData {
    V1 {
        new_steps: Option<HashMap<String, serde_json::Value>>,
        new_children: Option<HashMap<String, ChildDefinition>>,
        new_waits: Option<HashMap<String, WaitDefinition>>,
        result: Option<serde_json::Value>,
        error: Option<String>,
    },
}

impl ReturnedData {
    pub fn new_steps(&self) -> HashMap<String, serde_json::Value> {
        match self {
            ReturnedData::V1 { new_steps, .. } => new_steps.clone().unwrap_or(HashMap::new()),
        }
    }

    pub fn new_children(&self) -> HashMap<String, ChildDefinition> {
        match self {
            ReturnedData::V1 { new_children, .. } => new_children.clone().unwrap_or(HashMap::new()),
        }
    }

    pub fn new_waits(&self) -> HashMap<String, WaitDefinition> {
        match self {
            ReturnedData::V1 { new_waits, .. } => new_waits.clone().unwrap_or(HashMap::new()),
        }
    }

    pub fn result(&self) -> Option<&serde_json::Value> {
        match self {
            ReturnedData::V1 { result, .. } => result.as_ref(),
        }
    }

    pub fn error(&self) -> Option<&String> {
        match self {
            ReturnedData::V1 { error, .. } => error.as_ref(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChildWorkflowResult {
    Success { data: serde_json::Value },
    Failure { error: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct PreviousError {
    #[serde(with = "ts_seconds")]
    timestamp: DateTime<Utc>,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowContext {
    input: serde_json::Value,
    steps: HashMap<String, serde_json::Value>,
    child_workflows: HashMap<String, ChildWorkflowResult>,
    completed_waits: HashSet<String>,
    prev_errors: Vec<PreviousError>,
}

impl WorkflowContext {
    pub fn new(input: serde_json::Value) -> Self {
        Self {
            input,
            steps: HashMap::new(),
            child_workflows: HashMap::new(),
            completed_waits: HashSet::new(),
            prev_errors: Vec::new(),
        }
    }

    pub fn ingest_execution(&mut self, exec: &DbExecution) {
        if let Some(error) = exec.failure_reason.clone()
            && let Some(timestamp) = exec.executed_at
        {
            self.prev_errors.push(PreviousError {
                timestamp,
                message: error,
            });

            return;
        }

        if let Some(json) = exec.result_json.clone() {
            let data: Result<ReturnedData, _> = serde_json::from_value(json);
            match data {
                Ok(data) => {
                    let new_steps = data.new_steps();

                    for (name, result) in new_steps {
                        self.steps.insert(name, result);
                    }
                }
                Err(parse_error) => {
                    self.prev_errors.push(PreviousError {
                        timestamp: exec.executed_at.unwrap_or(Utc::now()),
                        message: format!(
                            "Implementation returned poorly formed data: {:?}",
                            parse_error
                        ),
                    });
                }
            }
        }
    }

    pub fn ingest_dependency(&mut self, dep: &DbDependency) {
        if let Some(name) = dep.wait_name.as_ref()
            && let Some(is_waited) = dep.wait_complete
            && is_waited
        {
            self.completed_waits.insert(name.clone());
        } else if let Some(name) = dep.child_workflow_name.as_ref()
            && let Some(result) = dep.child_result.as_ref()
        {
            self.child_workflows.insert(
                name.clone(),
                ChildWorkflowResult::Success {
                    data: result.clone(),
                },
            );
        } else if let Some(name) = dep.child_workflow_name.as_ref()
            && let Some(error) = dep.child_error.as_ref()
        {
            self.child_workflows.insert(
                name.clone(),
                ChildWorkflowResult::Failure {
                    error: error.clone(),
                },
            );
        }
    }
}

pub struct DbExecution {
    pub id: String,
    pub region: String,
    pub workflow_id: String,
    pub execution_index: i32,
    pub tenant_id: Option<String>,
    pub status: String,
    pub is_retry: bool,
    pub executed_at: Option<DateTime<Utc>>,
    pub result_json: Option<serde_json::Value>,
    pub failure_reason: Option<String>,
}

pub struct DbDependency {
    pub id: String,
    pub workflow_execution_id: String,
    pub child_workflow_name: Option<String>,
    pub child_workflow_id: Option<String>,
    pub wait_name: Option<String>,
    pub wait_until: Option<DateTime<Utc>>,
    pub child_result: Option<serde_json::Value>,
    pub child_error: Option<String>,
    pub wait_complete: Option<bool>,
}
