use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};

use crate::util::workflow::ReturnedData;

enum WorkflowExecutionResult {
    Failure(String),
    FailureWithJson(String, serde_json::Value),
    Success(serde_json::Value),
}

impl WorkflowExecutionResult {
    fn from_body(body: Result<String, String>) -> Self {
        if let Err(error) = body {
            return Self::Failure(error);
        }

        let body = body.unwrap();

        let json_value: Result<serde_json::Value, _> = serde_json::from_str(&body);

        if let Err(error) = json_value {
            let failure_reason = format!("Failed to parse as json: {:?}: \n {}", error, body);
            return Self::Failure(failure_reason);
        }

        let json_value = json_value.unwrap();

        let returned_data: Result<ReturnedData, _> = serde_json::from_value(json_value.clone());

        if let Err(error) = returned_data {
            let failure_reason = format!(
                "Returned data failed to match the expected format: {:?}",
                error
            );
            return Self::FailureWithJson(failure_reason, json_value);
        }

        let returned_data = returned_data.unwrap();

        let error = returned_data.error();

        if let Some(error) = error {
            let failure_reason =
                format!("Workflow implementation returned an error: \"{}\"", error);
            return Self::FailureWithJson(failure_reason, json_value);
        }

        Self::Success(json_value)
    }

    fn to_data(&self) -> (String, Option<serde_json::Value>, Option<String>) {
        match self {
            WorkflowExecutionResult::Failure(failure_reason) => {
                ("failed".to_string(), None, Some(failure_reason.clone()))
            }
            WorkflowExecutionResult::FailureWithJson(failure_reason, value) => (
                "failed".to_string(),
                Some(value.clone()),
                Some(failure_reason.clone()),
            ),
            WorkflowExecutionResult::Success(value) => {
                ("completed".to_string(), Some(value.clone()), None)
            }
        }
    }
}

pub async fn handle_workflow_execution_side_effect(
    workflow_execution_id: String,
    body: Result<String, String>,
    executed_at: DateTime<Utc>,
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), sqlx::Error> {
    let locked_workflow = sqlx::query!(
        r#"
      SELECT workflows.id FROM workflows
      JOIN workflow_executions
        ON workflows.id = workflow_executions.workflow_id
      WHERE workflow_executions.id = $1
      FOR UPDATE;
      "#,
        workflow_execution_id.clone()
    )
    .fetch_one(&mut **tx)
    .await?;

    let _locked_executions = sqlx::query!(
        r#"
      SELECT id FROM workflow_executions
      WHERE workflow_id = $1
      ORDER BY execution_index ASC
      FOR UPDATE
      "#,
        locked_workflow.id
    )
    .fetch_all(&mut **tx)
    .await?;

    let (status, result_json, failure_reason) = WorkflowExecutionResult::from_body(body).to_data();

    sqlx::query!(
        r#"
        UPDATE workflow_executions
        SET
          status = $1,
          executed_at = $2,
          result_json = $3,
          failure_reason = $4
        WHERE id = $5
      "#,
        status,
        executed_at,
        result_json,
        failure_reason,
        workflow_execution_id
    )
    .execute(&mut **tx)
    .await?;

    Ok(())
}
