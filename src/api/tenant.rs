use anyhow::anyhow;
use chrono::TimeDelta;
use nanoid::nanoid;
use poem_openapi::{
    ApiRequest, ApiResponse, NewType, Object, OpenApi, ResponseContent, payload::Json,
};
use replace_err::ReplaceErr;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Postgres, postgres::types::PgInterval};

use crate::api::public;

#[derive(Object, ResponseContent)]
pub struct Tenant {
    id: String,
    tokens: i32,
    max_tokens: i32,
    tokens_per_day: i32,
}

#[derive(ApiRequest)]
pub enum CreateTenantRequest {
    CreateWithJson(Json<CreateTenantData>),
}

#[derive(ApiResponse)]
pub enum CreateTenantResponse {
    #[oai(status = 201)]
    Created(Tenant),
    #[oai(status = 500)]
    InternalServerError,
}

#[derive(Debug, Clone, Serialize, Deserialize, Object)]
pub struct CreateTenantData {
    max_tokens: i32,
    tokens_per_day: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateTenantRequest {
    tokens_per_day: Option<i32>,
    max_tokens: Option<i32>,
    tokens: Option<i32>,
}

pub async fn create_tenant(
    req: CreateTenantRequest,
    pool: &Pool<Postgres>,
) -> CreateTenantResponse {
    let id = format!("tenant_{}", nanoid!());
    let CreateTenantRequest::CreateWithJson(data) = req;

    let (period, increment) = compute_increment_and_period(data.tokens_per_day);

    let created = sqlx::query!(
        r#"
        INSERT INTO tenants
          (id, tokens, max_tokens, increment, period)
        VALUES
          ($1, $2, $3, $4, make_interval(secs => $5))
        "#,
        id,
        data.max_tokens,
        data.max_tokens,
        increment,
        period.as_seconds_f64()
    )
    .execute(pool)
    .await;

    if created.is_err() {
        return CreateTenantResponse::InternalServerError;
    }

    CreateTenantResponse::Created
}

pub async fn update_tenant(
    id: String,
    request: UpdateTenantRequest,
    pool: Pool<Postgres>,
) -> anyhow::Result<Tenant> {
    let (period, increment) = request
        .tokens_per_day
        .map(compute_increment_and_period)
        .unzip();

    let pg_period: Option<PgInterval> = period.and_then(|delta| delta.try_into().ok());

    let tenant = sqlx::query!(
        r#"
        UPDATE tenants
          SET tokens = COALESCE($1, tokens),
              max_tokens = COALESCE($2, max_tokens),
              increment = COALESCE($3, increment),
              period = COALESCE($4, period),
              next_increment = now()
        WHERE id = $5
        RETURNING *
        "#,
        request.tokens,
        request.max_tokens,
        increment,
        pg_period,
        id
    )
    .fetch_one(&pool)
    .await?;

    let period = TimeDelta::days(tenant.period.days as i64)
        + TimeDelta::microseconds(tenant.period.microseconds);
    let tps = tenant.increment as f64 / period.as_seconds_f64();
    let tok_per_day = tps * 24. * 60. * 60.;

    Ok(Tenant {
        id,
        tokens: tenant.tokens,
        max_tokens: tenant.max_tokens,
        tokens_per_day: tok_per_day.round() as i32,
    })
}

const MIN_PERIOD_MS: f32 = 60_000.;

fn compute_increment_and_period(tokens_per_day: i32) -> (TimeDelta, i32) {
    let day_ms = { 24 * 60 * 60 * 1000 } as f32;
    let base_period = day_ms / tokens_per_day as f32;

    let base_factor = MIN_PERIOD_MS / base_period;

    let increment = base_factor.round();
    let period = base_period * increment;

    (TimeDelta::milliseconds(period as i64), increment as i32)
}
