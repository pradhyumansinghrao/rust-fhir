use crate::server::handlers::{
    create_resource, delete_resource, health, history_instance, history_system, history_type,
    metadata, process_bundle, read_resource, search_type, update_resource, validate_resource,
    vread_resource, AppState,
};
use axum::{
    http::header,
    routing::{get, post},
    Router,
};
use tower_http::cors::{Any, CorsLayer};

pub fn create_router(app_state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            header::ACCEPT,
            header::IF_MATCH,
            header::IF_NONE_MATCH,
        ])
        .expose_headers([
            header::ETAG,
            header::LOCATION,
            header::LAST_MODIFIED,
            header::CONTENT_TYPE,
        ]);

    Router::new()
        .route("/health", get(health))
        .route("/metadata", get(metadata))
        .route("/", post(process_bundle))
        .route("/_history", get(history_system))
        .route("/{resource_type}", post(create_resource).get(search_type))
        .route("/{resource_type}/_search", post(search_type))
        .route("/{resource_type}/_history", get(history_type))
        .route("/{resource_type}/$validate", post(validate_resource))
        .route(
            "/{resource_type}/{id}",
            get(read_resource)
                .put(update_resource)
                .delete(delete_resource),
        )
        .route("/{resource_type}/{id}/_history", get(history_instance))
        .route(
            "/{resource_type}/{id}/_history/{version_id}",
            get(vread_resource),
        )
        .layer(cors)
        .with_state(app_state)
}
