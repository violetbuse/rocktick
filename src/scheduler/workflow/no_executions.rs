use std::collections::HashSet;

use sqlx::{Postgres, Transaction};

use crate::{
    id,
    scheduler::{Scheduler, SchedulerContext},
    util::workflow::{DbDependency, DbExecution, ReturnedData, WorkflowContext},
};

pub struct NoExecutionScheduler {}

#[async_trait::async_trait]
impl Scheduler for NoExecutionScheduler {
    async fn run_once(ctx: &SchedulerContext, reached_end: &mut bool) -> anyhow::Result<()> {
        let mut tx = ctx.pool.begin().await?;

        let workflow = sqlx::query!(
            r#"
        SELECT workflow.* from workflows workflow
        WHERE NOT EXISTS (
          SELECT 1
          FROM workflow_executions exec
          WHERE exec.workflow_id = workflow.id
            AND exec.status NOT IN ('completed', 'failed')
        )
        LIMIT 1 FOR UPDATE SKIP LOCKED;
        "#
        )
        .fetch_optional(&mut *tx)
        .await?;

        if workflow.is_none() {
            *reached_end = true;
            return Ok(());
        }

        let workflow = workflow.unwrap();

        let executions = sqlx::query_as!(
            DbExecution,
            r#"
            SELECT * FROM workflow_executions
            WHERE workflow_id = $1
            ORDER BY execution_index ASC
            FOR UPDATE
          "#,
            workflow.id.clone()
        )
        .fetch_all(&mut *tx)
        .await?;

        if executions.is_empty() {
            let _new_execution = create_workflow_execution(
                NewExecution {
                    region: workflow.region,
                    workflow_id: workflow.id,
                    execution_index: 0,
                    tenant_id: workflow.tenant_id,
                    is_retry: false,
                },
                &mut tx,
            )
            .await?;

            return Ok(());
        }

        let dependencies = sqlx::query_as!(
            DbDependency,
            r#"
            SELECT
              dep.*,
              child.result as child_result,
              child.error as child_error,
              dep.wait_until < now() as wait_complete
            FROM workflow_dependencies dep
            JOIN workflow_executions exec
              ON exec.id = dep.workflow_execution_id
            LEFT JOIN workflows child
              ON child.id = dep.child_workflow_id
            WHERE
              exec.workflow_id = $1
            ORDER BY dep.id ASC
            FOR UPDATE OF dep
          "#,
            workflow.id.clone()
        )
        .fetch_all(&mut *tx)
        .await?;

        let mut retry_count = 0;
        let mut context = WorkflowContext::new(workflow.input.clone());

        let mut new_dependencies: HashSet<String> = HashSet::new();

        for execution in executions.iter() {
            context.ingest_execution(execution);

            if execution.is_retry {
                retry_count += 1;
            }

            if let Some(json) = execution.result_json.clone() {
                let returned: Result<ReturnedData, _> = serde_json::from_value(json);

                if let Ok(returned_data) = returned {
                    let new_children = returned_data.new_children();
                    let new_waits = returned_data.new_waits();

                    new_dependencies.extend(new_children.keys().cloned());
                    new_dependencies.extend(new_waits.keys().cloned());
                }
            }
        }

        for dependency in dependencies.iter() {
            context.ingest_dependency(dependency);

            if let Some(name) = dependency.wait_name.as_ref() {
                new_dependencies.remove(name);
            }

            if let Some(name) = dependency.child_workflow_name.as_ref() {
                new_dependencies.remove(name);
            }
        }

        if let Some(latest_execution) = executions.last()
            && let Some(result) = latest_execution.result_json.clone()
        {
            return finalize_workflow_success(&workflow.id, &result, &context, &mut tx).await;
        }

        if let Some(latest_execution) = executions.last()
            && let Some(error) = latest_execution.failure_reason.clone()
        {
            if retry_count == workflow.max_retries {
                return finalize_workflow_error(&workflow.id, &error, &context, &mut tx).await;
            } else {
                let index = latest_execution.execution_index + 1;

                let _new_execution = create_workflow_execution(
                    NewExecution {
                        region: workflow.region,
                        workflow_id: workflow.id,
                        execution_index: index,
                        tenant_id: workflow.tenant_id,
                        is_retry: true,
                    },
                    &mut tx,
                )
                .await?;

                return Ok(());
            }
        }

        // if no new dependencies were instantiated in the workflow, it counts
        // as a retry, even if no error was returned.
        let is_retry = new_dependencies.is_empty();

        if is_retry && retry_count == workflow.max_retries {
            return finalize_workflow_error(
                &workflow.id,
                &format!(
                    "Cannot retry execution more than {} times",
                    workflow.max_retries
                ),
                &context,
                &mut tx,
            )
            .await;
        }

        let new_index = executions
            .last()
            .map(|exe| exe.execution_index)
            .unwrap_or(0)
            + 1;

        let _new_execution = create_workflow_execution(
            NewExecution {
                region: workflow.region,
                workflow_id: workflow.id.clone(),
                execution_index: new_index,
                tenant_id: workflow.tenant_id,
                is_retry,
            },
            &mut tx,
        )
        .await?;

        tx.commit().await?;

        Ok(())
    }
}

struct NewExecution {
    region: String,
    workflow_id: String,
    execution_index: i32,
    tenant_id: Option<String>,
    is_retry: bool,
}

async fn create_workflow_execution(
    execution: NewExecution,
    tx: &mut Transaction<'_, Postgres>,
) -> anyhow::Result<String> {
    let id = id::generate("workflow_execution");

    sqlx::query!(
        r#"
      INSERT INTO workflow_executions
        (id, region, workflow_id, execution_index, tenant_id, status, is_retry)
      VALUES
        ($1, $2, $3, $4, $5, 'pending', $6);
    "#,
        id,
        execution.region,
        execution.workflow_id,
        execution.execution_index,
        execution.tenant_id,
        execution.is_retry
    )
    .execute(&mut **tx)
    .await?;

    Ok(id)
}

async fn finalize_workflow_error(
    id: &String,
    error: &str,
    context: &WorkflowContext,
    tx: &mut Transaction<'_, Postgres>,
) -> anyhow::Result<()> {
    let context = serde_json::to_value(context)?;

    sqlx::query!(
        r#"
    UPDATE workflows
    SET
      status = 'failed',
      error = $2,
      context = $3
    WHERE id = $1
  "#,
        id,
        error,
        context
    )
    .execute(&mut **tx)
    .await?;

    Ok(())
}

async fn finalize_workflow_success(
    id: &String,
    result: &serde_json::Value,
    context: &WorkflowContext,
    tx: &mut Transaction<'_, Postgres>,
) -> anyhow::Result<()> {
    let context = serde_json::to_value(context)?;

    sqlx::query!(
        r#"
    UPDATE workflows
    SET
      status = 'completed',
      result = $2,
      context = $3
    WHERE id = $1
  "#,
        id,
        result,
        context
    )
    .execute(&mut **tx)
    .await?;

    Ok(())
}
