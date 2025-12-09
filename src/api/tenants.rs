use axum::extract::{Path, State};
use chrono::TimeDelta;
use serde::Deserialize;
use sqlx::postgres::types::PgInterval;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    api::{ApiError, Context, JsonBody, TenantId, models::Tenant},
    id,
};

#[derive(Debug, Clone, Deserialize, ToSchema)]
struct CreateTenant {
    max_tokens: i32,
    tok_per_day: i32,
    max_timeout: i32,
    default_retries: i32,
    max_max_response_bytes: i32,
    max_request_bytes: i32,
}

#[utoipa::path(
  post,
  path = "/api/tenants",
  request_body = CreateTenant,
  responses(
    (status = 200, body =  Tenant),
    (status = "4XX", body =  ApiError),
    (status = "5XX", body =  ApiError)),
  tag = "tenants"
)]
async fn create_tenant(
    State(ctx): State<Context>,
    TenantId(tenant_id): TenantId,
    JsonBody(create_opts): JsonBody<CreateTenant>,
) -> Result<Tenant, ApiError> {
    if tenant_id.is_some() {
        return Err(ApiError::tenant_not_allowed());
    }

    let (period, increment) = compute_incr_and_period(create_opts.tok_per_day)?;
    let new_id = id::generate("tenant");
    let starting_tokens = create_opts.tok_per_day;
    let new_tenant = sqlx::query!(
        r#"
    INSERT INTO tenants
      (id, tokens, max_tokens, increment, period, max_timeout, default_retries, max_max_response_bytes, max_request_bytes)
    VALUES ($1, $2 ,$3, $4, $5, $6, $7, $8, $9) RETURNING *;
    "#,
        new_id,
        starting_tokens,
        create_opts.max_tokens,
        increment,
        period,
        create_opts.max_timeout,
        create_opts.default_retries,
        create_opts.max_max_response_bytes,
        create_opts.max_request_bytes
    )
    .fetch_one(&ctx.pool)
    .await?;

    let tenant = Tenant {
        id: new_tenant.id,
        tokens: new_tenant.tokens,
        max_tokens: new_tenant.max_tokens,
        tok_per_day: create_opts.tok_per_day,
        max_timeout: new_tenant.max_timeout,
        default_retries: new_tenant.default_retries,
        max_max_response_bytes: new_tenant.max_max_response_bytes,
        max_request_bytes: new_tenant.max_request_bytes,
    };

    Ok(tenant)
}

#[utoipa::path(
  get,
  path = "/api/tenants/{tenant_id}",
  params(("tenant_id", description = "Id of the tenant")),
  responses((status = 200, body =  Tenant),
    (status = "4XX", body =  ApiError),
    (status = "5XX", body =  ApiError)),
  tag = "tenants"
)]
async fn get_tenant(
    State(ctx): State<Context>,
    TenantId(requesting_tenant_id): TenantId,
    Path(tenant_id): Path<String>,
) -> Result<Tenant, ApiError> {
    if let Some(requesting_tenant_id) = requesting_tenant_id
        && requesting_tenant_id != tenant_id
    {
        return Err(ApiError::tenant_not_allowed());
    }

    let tenant = sqlx::query!(
        r#"
    SELECT *
    FROM tenants
    WHERE id = $1;
    "#,
        tenant_id
    )
    .fetch_optional(&ctx.pool)
    .await?;

    if tenant.is_none() {
        return Err(ApiError::not_found());
    }

    let tenant = tenant.unwrap();

    let period_secs = TimeDelta::microseconds(tenant.period.microseconds).as_seconds_f64();
    let tok_per_sec = tenant.increment as f64 / period_secs;
    let tok_per_day = tok_per_sec * 60. * 60. * 24.;

    let res = Tenant {
        id: tenant.id,
        tokens: tenant.tokens,
        max_tokens: tenant.max_tokens,
        tok_per_day: tok_per_day as i32,
        max_timeout: tenant.max_timeout,
        default_retries: tenant.default_retries,
        max_max_response_bytes: tenant.max_max_response_bytes,
        max_request_bytes: tenant.max_request_bytes,
    };

    Ok(res)
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
struct UpdateTenant {
    tokens: Option<i32>,
    max_tokens: Option<i32>,
    tok_per_day: Option<i32>,
    max_timeout: Option<i32>,
    default_retries: Option<i32>,
    max_max_response_bytes: Option<i32>,
    max_request_bytes: Option<i32>,
}

#[utoipa::path(
  post,
  path = "/api/tenants/{tenant_id}",
  params(("tenant_id", description = "Id of the tenant")),
  request_body = UpdateTenant,
  responses(
    (status = 200, body =  Tenant),
    (status = "4XX", body = ApiError),
    (status = "5XX", body = ApiError)),
  tag = "tenants"
)]
async fn update_tenant(
    State(ctx): State<Context>,
    Path(tenant_id): Path<String>,
    TenantId(requesting_tenant_id): TenantId,
    JsonBody(update_opts): JsonBody<UpdateTenant>,
) -> Result<Tenant, ApiError> {
    if requesting_tenant_id.is_some() {
        return Err(ApiError::tenant_not_allowed());
    }

    let (period, increment) = if let Some(tok_per_day) = update_opts.tok_per_day {
        Some(compute_incr_and_period(tok_per_day)?)
    } else {
        None
    }
    .unzip();

    let new_tenant = sqlx::query!(
        r#"
      UPDATE tenants
      SET
        tokens = COALESCE($1, tokens),
        max_tokens = COALESCE($2, max_tokens),
        period = COALESCE($3, period),
        increment = COALESCE($4, increment),
        max_timeout = COALESCE($5, max_timeout),
        default_retries = COALESCE($6, default_retries),
        max_max_response_bytes = COALESCE($7, max_max_response_bytes),
        max_request_bytes = COALESCE($8, max_request_bytes)
      WHERE id = $9 RETURNING *
      "#,
        update_opts.tokens,
        update_opts.max_tokens,
        period,
        increment,
        update_opts.max_timeout,
        update_opts.default_retries,
        update_opts.max_max_response_bytes,
        update_opts.max_request_bytes,
        tenant_id
    )
    .fetch_optional(&ctx.pool)
    .await?;

    if new_tenant.is_none() {
        return Err(ApiError::not_found());
    }

    let tenant = new_tenant.unwrap();

    let period_secs = TimeDelta::microseconds(tenant.period.microseconds).as_seconds_f64();
    let tok_per_sec = tenant.increment as f64 / period_secs;
    let tok_per_day = tok_per_sec * 60. * 60. * 24.;

    let res = Tenant {
        id: tenant.id,
        tokens: tenant.tokens,
        max_tokens: tenant.max_tokens,
        tok_per_day: tok_per_day as i32,
        max_timeout: tenant.max_timeout,
        default_retries: tenant.default_retries,
        max_max_response_bytes: tenant.max_max_response_bytes,
        max_request_bytes: tenant.max_request_bytes,
    };

    Ok(res)
}

pub fn init_router() -> OpenApiRouter<Context> {
    OpenApiRouter::new()
        .routes(routes!(create_tenant))
        .routes(routes!(get_tenant, update_tenant))
}

const MIN_PERIOD_MS: f32 = 60_000.;
const DAY_MS: f32 = 24. * 60. * 60. * 1000.;

fn compute_incr_and_period(tokens_per_day: i32) -> Result<(PgInterval, i32), String> {
    let base_period = DAY_MS / tokens_per_day as f32;
    let base_factor = MIN_PERIOD_MS / base_period;

    let factor = base_factor.ceil();
    let increment = factor as i32;
    let period = base_period * factor;

    let micros = TimeDelta::milliseconds(period as i64)
        .num_microseconds()
        .ok_or("Failed to compute period, micros overflow.")?;

    let interval = PgInterval {
        months: 0,
        days: 0,
        microseconds: micros,
    };

    Ok((interval, increment))
}
