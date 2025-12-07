use utoipa_axum::router::OpenApiRouter;

use crate::api::Context;

pub fn init_router() -> OpenApiRouter<Context> {
    OpenApiRouter::new()
}
