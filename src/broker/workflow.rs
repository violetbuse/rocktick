use sqlx::{Executor, Postgres, Transaction};

pub async fn handle_workflow_execution_side_effect(
    scheduled_job_id: String,
    tx: &mut Transaction<'_, Postgres>,
) -> Result<(), sqlx::Error> {
    todo!("actually handle the workflow execution")
}
