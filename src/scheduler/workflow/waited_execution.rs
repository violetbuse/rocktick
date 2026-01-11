use std::{
    hash::{DefaultHasher, Hash, Hasher},
    time::Duration,
};

use chrono::Utc;

use crate::{
    id,
    scheduler::{Scheduler, SchedulerContext},
    util::workflow::{DbDependency, DbExecution, WorkflowContext},
};

pub struct WaitedExecutionScheduler;

#[async_trait::async_trait]
impl Scheduler for WaitedExecutionScheduler {
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
                AND exec.status = 'waiting'
            )
            AND NOT EXISTS (
              SELECT 1
              FROM workflow_executions exec
              WHERE exec.workflow_id = workflow.id
                AND exec.status NOT IN ('waiting', 'completed', 'failed')
            )
            AND NOT EXISTS (
              SELECT 1
              FROM workflow_executions exec
              JOIN workflow_dependencies dep
                ON dep.workflow_execution_id = exec.id
              LEFT JOIN workflows child_workflow
                ON child_workflow.id = dep.child_workflow_id
              WHERE exec.workflow_id = workflow.id
                AND (
                  -- Blocking Condition A: a wait timer that hasn't elapsed yet
                  (dep.wait_until IS NOT NULL AND dep.wait_until > now()) OR
                  -- Blocking Condition B: a child workflow that isn't completed or failed
                  (dep.child_workflow_id IS NOT NULL AND child_workflow.status = 'pending')
                )
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

        let last_execution = sqlx::query!(
            r#"
          SELECT * FROM workflow_executions
          WHERE status = 'waiting' AND workflow_id = $1
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
        let mut retry_count = 0;

        for execution in executions.iter() {
            context.ingest_execution(execution);

            if execution.is_retry {
                retry_count += 1;
            }
        }

        for dependency in dependencies.iter() {
            context.ingest_dependency(dependency);
        }

        let context_json = serde_json::to_value(context)?;

        sqlx::query!(
            r#"
            UPDATE workflows
            SET context = $2
            WHERE id = $1
          "#,
            workflow.id.clone(),
            &context_json
        )
        .execute(&mut *tx)
        .await?;

        let json_stringified = context_json.to_string();

        let request_id = id::generate("request");
        sqlx::query!(
            r#"
            INSERT INTO http_requests (id, method, url, headers, body)
            VALUES ($1, $2, $3, $4, $5)
          "#,
            request_id,
            "post",
            workflow.implementation_url,
            &Vec::new(),
            json_stringified
        )
        .execute(&mut *tx)
        .await?;

        let wait_factor = if last_execution.is_retry {
            2 ^ retry_count
        } else {
            1
        };
        let wait_time = Duration::from_mins(3) * wait_factor;
        let scheduled_at = Utc::now() + wait_time;

        let scheduled_job_id = id::gen_for_time("scheduled_job", scheduled_at);

        let mut hasher = DefaultHasher::new();
        scheduled_job_id.hash(&mut hasher);
        let full_hash: u64 = hasher.finish();
        let truncated_hash_u32 = (full_hash & 0xFFFFFFFF) as u32;
        let hash = truncated_hash_u32 as i32;

        sqlx::query!(
            r#"
            INSERT INTO scheduled_jobs
              (id,
              hash,
              region,
              tenant_id,
              workflow_id,
              workflow_execution_id,
              scheduled_at,
              request_id,
              max_retries)
            VALUES
              ($1, $2, $3, $4, $5, $6, $7, $8, 0)
          "#,
            scheduled_job_id,
            hash,
            workflow.region,
            workflow.tenant_id,
            workflow.id,
            last_execution.id,
            scheduled_at,
            request_id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            UPDATE workflow_executions
            SET status = 'scheduled'
            WHERE id = $1
          "#,
            last_execution.id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }
}
