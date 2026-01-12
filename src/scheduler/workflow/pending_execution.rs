use std::collections::HashMap;

use crate::{
    id,
    scheduler::{Scheduler, SchedulerContext},
    util::workflow::{
        ChildDefinition, DbDependency, DbExecution, ReturnedData, WaitDefinition, WorkflowContext,
    },
};

pub struct PendingExecutionScheduler;

#[async_trait::async_trait]
impl Scheduler for PendingExecutionScheduler {
    #[tracing::instrument(name = "PendingExecutionScheduler::run_once")]
    async fn run_once(ctx: &SchedulerContext, reached_end: &mut bool) -> anyhow::Result<()> {
        let mut tx = ctx.pool.begin().await?;

        let workflow = sqlx::query!(
            r#"
          SELECT workflow.*
          FROM workflows workflow
          WHERE
            EXISTS (
              SELECT 1
              FROM workflow_executions exec
              WHERE exec.workflow_id = workflow.id
                AND exec.status = 'pending'
            )
            AND NOT EXISTS (
              SELECT 1
              FROM workflow_executions exec
              WHERE exec.workflow_id = workflow.id
                AND exec.status NOT IN ('pending', 'completed', 'failed')
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

        let pending_execution = sqlx::query!(
            r#"
          SELECT * FROM workflow_executions
          WHERE status = 'pending' AND workflow_id = $1
          LIMIT 1
        "#,
            workflow.id.clone()
        )
        .fetch_one(&mut *tx)
        .await?;

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
            WHERE exec.workflow_id = $1
            ORDER BY dep.id ASC
            FOR UPDATE OF dep
          "#,
            workflow.id.clone()
        )
        .fetch_all(&mut *tx)
        .await?;

        let mut context = WorkflowContext::new(workflow.input);
        let mut execution_results: Vec<ReturnedData> = Vec::new();

        for execution in executions.iter() {
            context.ingest_execution(execution);

            if let Some(json) = execution.result_json.as_ref()
                && let Ok(result) = serde_json::from_value(json.clone())
            {
                execution_results.push(result);
            }
        }

        for dependency in dependencies.iter() {
            context.ingest_dependency(dependency);
        }

        let mut child_workflows: HashMap<String, ChildDefinition> = HashMap::new();
        let mut waits: HashMap<String, WaitDefinition> = HashMap::new();

        for returned in execution_results.iter() {
            let new_child_workflows = returned.new_children();
            let new_waits = returned.new_waits();

            for (name, definition) in new_child_workflows {
                child_workflows.insert(name, definition);
            }

            for (name, definition) in new_waits {
                waits.insert(name, definition);
            }
        }

        for dependency in dependencies.iter() {
            if let Some(child_workflow_name) = dependency.child_workflow_name.as_ref() {
                child_workflows.remove(child_workflow_name);
            }

            if let Some(wait_name) = dependency.wait_name.as_ref() {
                waits.remove(wait_name);
            }
        }

        for (name, definition) in child_workflows.iter() {
            let new_workflow_id = id::generate("workflow");
            sqlx::query!(
                r#"
              INSERT INTO workflows
                (id, region, tenant_id, implementation_url, input, status, max_retries)
              VALUES
                ($1, $2, $3, $4, $5, 'pending', $6)
            "#,
                new_workflow_id,
                workflow.region,
                workflow.tenant_id,
                definition.url().to_string(),
                definition.input(),
                definition.max_retries()
            )
            .execute(&mut *tx)
            .await?;

            let new_dependency_id = id::generate("workflow_dependency");
            sqlx::query!(
                r#"
                INSERT INTO workflow_dependencies
                  (id, workflow_execution_id, child_workflow_name, child_workflow_id)
                VALUES
                  ($1, $2, $3, $4)
              "#,
                new_dependency_id,
                pending_execution.id.clone(),
                name,
                new_workflow_id
            )
            .execute(&mut *tx)
            .await?;
        }

        for (name, definition) in waits.iter() {
            let new_dependency_id = id::generate("workflow_dependency");
            sqlx::query!(
                r#"
              INSERT INTO workflow_dependencies
                (id, workflow_execution_id, wait_name, wait_until)
              VALUES
                ($1, $2, $3, $4)
            "#,
                new_dependency_id,
                pending_execution.id.clone(),
                name,
                definition.wait_until()
            )
            .execute(&mut *tx)
            .await?;
        }

        sqlx::query!(
            r#"
            UPDATE workflow_executions
            SET
              status = 'waiting'
            WHERE id = $1
          "#,
            pending_execution.id.clone()
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }
}
