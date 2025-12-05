use poem_openapi::{OpenApi, payload::PlainText};
use sqlx::{Pool, Postgres};

use crate::api::tenant::{self, CreateTenantRequest, CreateTenantResponse, Tenant};

pub struct PublicApi {
    pub pool: Pool<Postgres>,
}

#[OpenApi(prefix_path = "/api/v1")]
impl PublicApi {
    #[oai(path = "/", method = "get")]
    async fn index(&self) -> PlainText<&'static str> {
        PlainText("Hello World")
    }

    #[oai(path = "/tenants", method = "post")]
    async fn create_tenant(&self, req: CreateTenantRequest) -> CreateTenantResponse {
        tenant::create_tenant(req, &self.pool).await
    }
}
